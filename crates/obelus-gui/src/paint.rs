//! The window's pixels: one texture of glyphs, and a quad for everything.
//!
//! Everything on the screen is a rectangle with a colour and, sometimes, a
//! picture -- a cell's background, a letter, the caret. So there is one
//! pipeline, one buffer of instances, and one texture that every glyph this
//! session has drawn lives in. The order they are written in is the order
//! they are drawn in, which is what puts a letter over its background
//! without a second pass to arrange it.
//!
//! What is *not* here is any idea of what the cells mean. This file is
//! handed a page and a set of faces and turns them into rectangles.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use cosmic_text::{CacheKey, SwashContent};
use obelus_app::app::Caret;
use obelus_ui::image::{Palette, SLOT};
use ratatui::style::{Color, Modifier};
use winit::window::Window;

use crate::{
    font::{CellSize, Fonts},
    grid::{Capped, Look, Marked, Page, Spelling},
    motion::Moving,
};

/// How far a cap is held off the rows either side of it, as a part of a
/// cell's height.
const INSET: f32 = 0.08;
/// And off the cells either side, as a part of a cell's width.
///
/// The same idea and the same reason: a cap that reached the edge of the
/// blank it was given would touch whatever is in the next cell, and one
/// place -- the welcome screen, where a picture sits against the key --
/// has only that one blank to share. A fraction of a cell rather than a
/// whole one, because the blank *is* the cap's; what this holds off is
/// what is on the other side of it.
const SIDE: f32 = 0.15;
/// How thick the lip under it is, by the same measure.
///
/// What says the key is raised. Thicker than the outline on the other
/// three sides, because a key is lit from above and a real one's bottom
/// edge is the part of it you can see.
const LIP: f32 = 0.12;
/// How round its corners are, as a part of its height.
const ROUNDING: f32 = 0.22;

/// How wide the bar caret is, as a part of a cell.
///
/// Thin enough to stand between two characters rather than on one, which is
/// the whole of what it says.
const BAR: f32 = 0.15;

/// And how thick the line under what is being spelled is.
const UNDERLINE: f32 = 0.06;

/// How big the glyph texture starts, in pixels each way.
///
/// A screenful of code is a few hundred distinct glyphs, and this holds
/// thousands. It grows by being emptied rather than by being enlarged: a
/// reader who has filled it has changed the size of the text, and the
/// glyphs in it are the old size.
const ATLAS: u32 = 1024;

/// What Obelus draws on.
pub(crate) struct Painter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    configured: wgpu::SurfaceConfiguration,
    /// The format the surface is *viewed* as, which is the format it was
    /// given with any sRGB conversion taken off. Colours in a theme are
    /// written the way a terminal means them, and a surface that converts
    /// them on the way out draws a different colour from the one the reader
    /// chose.
    view: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    atlas: Atlas,
    /// What each mark is drawn from, until it has been drawn.
    ///
    /// The text rather than the pixels: how many pixels a mark is depends
    /// on how big a cell is, which the reader changes.
    drawings: HashMap<(String, bool), String>,
    /// The colours those drawings are inked in. A theme change is a new
    /// palette, and every mark already drawn is the old theme's.
    palette: Option<Palette>,
    /// The instances of the frame being built, kept so that a screenful of
    /// rectangles is allocated once rather than once a frame.
    quads: Vec<Quad>,
    instances: wgpu::Buffer,
}

/// One rectangle, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Quad {
    /// Left, top, width, height, in real pixels.
    rect: [f32; 4],
    /// Left, top, right, bottom in the atlas, from zero to one.
    uv: [f32; 4],
    colour: [f32; 4],
    flags: u32,
    /// How far its corners are rounded, in pixels. Read only where
    /// `ROUNDED` is set.
    radius: f32,
    /// The hardware wants the whole thing aligned; nothing reads these.
    padding: [u32; 2],
}

/// A rectangle with no picture: its colour is the whole of it.
const SOLID: u32 = 1;
/// A picture with colours of its own, which is an emoji.
const COLOURFUL: u32 = 2;
/// A solid whose corners are taken off, which is a key's cap.
const ROUNDED: u32 = 4;

/// What the shader needs to know about the window.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Screen {
    size: [f32; 2],
    padding: [f32; 2],
}

/// Where every glyph drawn this session is kept.
struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    allocator: etagere::AtlasAllocator,
    /// Where each glyph landed, or that the face had no picture for it --
    /// which is worth remembering too, or a missing glyph is rasterised
    /// again on every frame that asks for it.
    spots: HashMap<CacheKey, Option<Spot>>,
    /// And where each agent's mark landed, by the same rule.
    ///
    /// A second map rather than a second texture: a mark is a picture of
    /// about sixteen pixels square, which is a large glyph and nothing
    /// more. What makes it a map of its own is that it is thrown away for
    /// a different reason -- a theme, rather than a size.
    marks: HashMap<(String, bool), Option<Spot>>,
    /// One opaque pixel, so that a rectangle with no picture can go through
    /// the same pipeline as one with.
    white: [f32; 4],
}

/// Where one glyph is, and how it sits against its cell.
#[derive(Clone, Copy, Debug)]
struct Spot {
    uv: [f32; 4],
    width: f32,
    height: f32,
    /// How far right of the pen the picture starts.
    left: f32,
    /// And how far above the baseline its top is.
    top: f32,
    colourful: bool,
}

impl Painter {
    /// Takes over the window.
    ///
    /// Asking for an adapter and a device are the only two `await`s in the
    /// whole binary, and they happen once: blocked on rather than given a
    /// runtime, which would be a runtime for two calls.
    pub(crate) fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(
            // From the environment, so that `WGPU_BACKEND=gl` on a machine
            // whose Vulkan driver is the problem is a thing the reader can
            // try without a rebuild.
            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        );
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("no surface to draw on")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // A grid of text is not work for a discrete card, and asking
            // for one on a laptop is asking for the fan and the battery.
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .context("no graphics adapter this window can be drawn with")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("obelus"),
            required_features: wgpu::Features::empty(),
            // The floor every desktop adapter clears, so that what is asked
            // for is what a window needs rather than what this machine
            // happens to have.
            // What this machine offers, not a floor. The floor allows a
            // texture of 2048 pixels, and a window is a texture: a
            // maximised Obelus on a tall screen asked for 1882 by 2052 and
            // the surface refused it -- which is a panic on a resize, not
            // a degraded picture.
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .context("the graphics adapter would not open a device")?;

        let most = device.limits().max_texture_dimension_2d;
        let mut configured = surface
            .get_default_config(
                &adapter,
                size.width.clamp(1, most),
                size.height.clamp(1, most),
            )
            .context("the surface offers no way to be drawn on")?;
        let view = configured.format.remove_srgb_suffix();
        // The surface is drawn through a view that does not convert, and
        // the view has to be declared before the surface is configured.
        configured.view_formats = vec![view];
        // Waiting for the screen rather than racing it: Obelus draws when
        // something happened, so there is never a frame to throw away.
        configured.present_mode = wgpu::PresentMode::AutoVsync;
        // Said, because the default is whatever the platform would rather
        // do and on a Wayland compositor that is to honour the alpha
        // channel: a page drawn in a theme's own dark background came out
        // with the wallpaper showing through it. Obelus's window is not a
        // transparent window.
        let capabilities = surface.get_capabilities(&adapter);
        if capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
        {
            configured.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        surface.configure(&device, &configured);
        // What the window turned out to be drawn with, which is the line to
        // read when a colour or a shape is not what the theme says.
        tracing::info!(
            adapter = adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            format = ?configured.format,
            view = ?view,
            alpha = ?configured.alpha_mode,
            offered = ?capabilities.alpha_modes,
            "drawing with"
        );

        let atlas = Atlas::new(&device, &queue);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("obelus screen"),
            size: std::mem::size_of::<Screen>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("obelus glyphs"),
            // Nearest, because a glyph is rasterised at the size it is drawn
            // at: there is nothing to interpolate, and interpolating it
            // would be blurring text that was sharp.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("obelus"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("obelus"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("obelus"),
            source: wgpu::ShaderSource::Wgsl(include_str!("paint.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("obelus"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("obelus"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Quad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x4,
                        2 => Float32x4,
                        3 => Uint32,
                        4 => Float32,
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("obelus quads"),
            size: (std::mem::size_of::<Quad>() * 4096) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            configured,
            view,
            pipeline,
            uniforms,
            bindings,
            atlas,
            drawings: HashMap::new(),
            palette: None,
            quads: Vec::new(),
            instances,
        })
    }

    /// The window changed size, so the surface has to.
    pub(crate) fn resized(&mut self, width: u32, height: u32) {
        // Clamped, because a surface bigger than the largest texture this
        // device can make is not a picture that comes out wrong -- it is a
        // validation error, which is to say a panic while the reader drags
        // a corner.
        let most = self.device.limits().max_texture_dimension_2d;
        if width > most || height > most {
            // Said, because what it does instead of failing is draw a
            // surface smaller than the window and let the compositor
            // stretch it: the picture goes soft, and a reader who cannot
            // see why would have nothing to go on.
            tracing::warn!(
                width,
                height,
                most,
                "the window is larger than this device can draw in one texture"
            );
        }
        self.configured.width = width.clamp(1, most);
        self.configured.height = height.clamp(1, most);
        self.surface.configure(&self.device, &self.configured);
    }

    /// The text is a different size now, so nothing kept about its glyphs
    /// -- or about the marks, which are drawn to fit a cell -- is about
    /// this size.
    pub(crate) fn forget_the_glyphs(&mut self) {
        self.atlas.empty();
    }

    /// A mark the window may be asked to draw, and what it is drawn from.
    ///
    /// Kept as the drawing rather than turned into pixels here: what size
    /// to draw it at is known at the moment it is drawn, and a reader who
    /// changes the text's size changes it.
    pub(crate) fn carries(&mut self, id: String, focused: bool, svg: String, palette: Palette) {
        if self.palette != Some(palette) {
            // Pixels cannot be recoloured after the fact, so a new theme
            // is every mark drawn again. The same rule the terminal's side
            // of this follows.
            self.drawings.clear();
            self.atlas.forget_the_marks();
            self.palette = Some(palette);
        }
        tracing::debug!(id, focused, "the window carries a mark");
        self.drawings.insert((id, focused), svg);
    }

    /// Draws a page, the marks on it, and whatever is being spelled over
    /// it.
    pub(crate) fn paint(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        moving: Moving,
        marked: &[Marked],
        capped: &[Capped],
    ) -> Result<()> {
        let cell = fonts.cell();
        self.quads.clear();
        self.backgrounds(page, cell.width, cell.height);
        // Over the ground and under the text: a cap is the shape the cells
        // behind a key are, and the key is written on it.
        self.caps(page, capped, cell);
        self.letters(page, fonts);
        // After the text, over cells the view left empty: a view draws its
        // glyph only where a picture could not be drawn.
        self.marks(marked, cell);
        // Over the cells and under the caret: the word being spelled is
        // going in at the caret, so the caret belongs at the place in it
        // the input method says.
        self.spelling(page, fonts, spelling);
        // Off for half of every cycle, which is the blink. What is under it
        // is drawn either way, by the pass above.
        if moving.caret {
            self.caret(page, fonts, spelling, moving.drift);
        }

        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let screen = Screen {
            size: [self.configured.width as f32, self.configured.height as f32],
            padding: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&screen));

        let wanted = (std::mem::size_of::<Quad>() * self.quads.len().max(1)) as u64;
        if wanted > self.instances.size() {
            // Doubled, so that a screen that keeps growing does not
            // reallocate on every frame of the resize.
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("obelus quads"),
                size: wanted * 2,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.quads));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // The surface went out from under the frame -- the window was
            // resized, a monitor changed -- which is not an error to stop
            // for: it is put back, and the frame that was being drawn is
            // drawn again by the redraw the resize itself asks for.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.configured);
                return Ok(());
            }
            // Nobody can see it: the window is hidden, or the driver did
            // not answer in time. Both are reasons to skip a frame rather
            // than to stop.
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("the surface refused to give up a frame to draw into")
            }
        };
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("obelus"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("obelus"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Black, which is only ever seen in the strip below
                        // the last whole row: every cell draws its own
                        // background over the rest.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bindings, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..4, 0..self.quads.len() as u32);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    /// The colour behind the text, as few rectangles as it takes.
    ///
    /// A run of cells with the same background is one rectangle: a screen
    /// is mostly the page's own colour, and a quad per cell would be ten
    /// thousand of them to say so.
    fn backgrounds(&mut self, page: &Page, width: f32, height: f32) {
        // A window is not a whole number of cells across, so there is a
        // strip down the right and along the bottom that no cell reaches.
        // The last run of each row is stretched into it, and the last row
        // down into the one below: the alternative is what was there
        // before, which is a black seam beside a page that is not black.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (right, bottom) = (self.configured.width as f32, self.configured.height as f32);
        for row in 0..page.rows() {
            let tall = match row + 1 == page.rows() {
                true => (bottom - f32::from(row) * height).max(height),
                false => height,
            };
            for (start, end, colour) in runs(page, row) {
                let wide = match end == page.columns() {
                    true => (right - f32::from(start) * width).max(width),
                    false => f32::from(end - start) * width,
                };
                self.block(
                    f32::from(start) * width,
                    f32::from(row) * height,
                    wide,
                    tall,
                    rgba(colour, Ink::Background),
                );
            }
        }
    }

    /// One block of colour, in pixels.
    fn block(&mut self, left: f32, top: f32, width: f32, height: f32, colour: [f32; 4]) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID,
            radius: 0.0,
            padding: [0; 2],
        });
    }

    /// One block of colour with its corners taken off.
    fn rounded(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        colour: [f32; 4],
    ) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | ROUNDED,
            radius: radius.max(0.0).min(width.min(height) / 2.0),
            padding: [0; 2],
        });
    }

    /// The caps the keys at the foot of a view are drawn in.
    ///
    /// A terminal's cap is the run of cells behind the key, a shade off
    /// the page, and that is what has already been painted here by
    /// `backgrounds`. What a window can say that a terminal cannot is the
    /// *shape*: so the ground is put back over those cells and the cap is
    /// drawn on it, which is why this runs after the backgrounds and
    /// before the letters that sit on it.
    ///
    /// Three rectangles, and the third is what makes it a key rather than
    /// a rounded box: the face is drawn a pixel inside the outline on
    /// three sides and further in at the bottom, so what is left showing
    /// under it is a lip. Which is the whole of the trick a keyboard's own
    /// keys use.
    fn caps(&mut self, page: &Page, capped: &[Capped], cell: CellSize) {
        for cap in capped {
            // A cap whose cells no longer hold its key belongs to a view
            // that was drawn over inside the frame that said it -- see
            // `Capped::still_said`.
            if !cap.still_said(page) {
                continue;
            }
            let left = f32::from(cap.area.x) * cell.width;
            let top = f32::from(cap.area.y) * cell.height;
            let width = f32::from(cap.area.width) * cell.width;
            let side = (cell.width * SIDE).round();
            // Never thinner than a pixel: a lip that rounds away is a cap
            // that lies flat, and the inset is what keeps a cap off the
            // rows either side of it.
            let inset = (cell.height * INSET).round().max(1.0);
            let lip = (cell.height * LIP).round().max(1.0);
            let height = (cell.height - inset * 2.0).max(1.0);
            let radius = (height * ROUNDING).min(cell.width);
            // What the cells said, put back: the corners this is about to
            // round away are painted in the cap's own ground, and a cap
            // drawn over them would have square shoulders.
            self.block(
                left,
                top,
                width,
                cell.height,
                rgba(cap.page, Ink::Background),
            );
            // Held off the cells either side, which the ground above is
            // not: what was put back is every cell the cap was said
            // about, and what is drawn on it stops short of them.
            let drawn = (width - side * 2.0).max(1.0);
            self.rounded(
                left + side,
                top + inset,
                drawn,
                height,
                radius,
                rgba(cap.edge, Ink::Foreground),
            );
            let face = (height - 1.0 - lip).max(1.0);
            self.rounded(
                left + side + 1.0,
                top + inset + 1.0,
                (drawn - 2.0).max(1.0),
                face,
                (radius - 1.0).max(0.0),
                rgba(cap.cap, Ink::Background),
            );
        }
    }

    /// The text.
    fn letters(&mut self, page: &Page, fonts: &mut Fonts) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                if look.text.trim().is_empty() {
                    continue;
                }
                let colour = rgba(look.foreground, Ink::Foreground);
                self.glyphs(column, row, look, colour, fonts);
            }
        }
    }

    /// One cell's glyphs, put where the grid says rather than where their
    /// own advances would have reached.
    fn glyphs(
        &mut self,
        column: u16,
        row: u16,
        look: Look<'_>,
        colour: [f32; 4],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        self.glyphs_at(
            f32::from(column) * cell.width,
            f32::from(row) * cell.height,
            look,
            colour,
            fonts,
        );
    }

    /// The same, at a place in pixels rather than at a cell.
    ///
    /// Which the caret needs because a caret on its way is not on a cell
    /// boundary, and what a block carries has to be in the same place the
    /// block is.
    fn glyphs_at(
        &mut self,
        left: f32,
        top: f32,
        look: Look<'_>,
        colour: [f32; 4],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let bold = look.modifier.contains(Modifier::BOLD);
        let italic = look.modifier.contains(Modifier::ITALIC);
        let placed = fonts.glyphs(look.text, bold, italic).to_vec();
        for glyph in placed {
            let Some(spot) = self.atlas.spot(&self.queue, fonts, glyph.key) else {
                continue;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "a glyph is offset by pixels, and there are few of them"
            )]
            let (x, y) = (glyph.x as f32, glyph.y as f32);
            // The grid's baseline for writing, and a mark's own for a mark
            // -- see `font::Placed::baseline`.
            let baseline = glyph.baseline.unwrap_or(cell.baseline);
            self.quads.push(Quad {
                rect: [
                    left + x + spot.left,
                    top + baseline + y - spot.top,
                    spot.width,
                    spot.height,
                ],
                uv: spot.uv,
                colour,
                flags: match spot.colourful {
                    true => COLOURFUL,
                    false => 0,
                },
                radius: 0.0,
                padding: [0; 2],
            });
        }
    }

    /// The marks on this frame, each drawn into the two cells it was given.
    ///
    /// Rasterised the first time it is asked for at this size and kept in
    /// the same texture the glyphs are in: a mark is a picture of about
    /// sixteen pixels square, which is a large glyph and nothing more.
    fn marks(&mut self, marked: &[Marked], cell: crate::font::CellSize) {
        for mark in marked {
            let key = (mark.id.clone(), mark.focused);
            let spot = match self.atlas.mark(&key) {
                Some(spot) => spot,
                None => {
                    let (Some(svg), Some(palette)) = (self.drawings.get(&key), self.palette) else {
                        // Asked for before it was carried, which the view
                        // does not do -- but a frame is not the place to
                        // find out.
                        continue;
                    };
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a mark is a few dozen pixels each way"
                    )]
                    let pixels = (
                        (cell.width * f32::from(SLOT.width)).round() as u32,
                        (cell.height * f32::from(SLOT.height)).round() as u32,
                    );
                    let paper = match mark.focused {
                        true => palette.selected,
                        false => palette.paper,
                    };
                    let Some(drawn) = obelus_ui::image::raster(svg, pixels, palette.ink, paper)
                    else {
                        // A drawing this machine's renderer would not read.
                        // Remembered as nothing, so it is not tried again
                        // on every frame -- and said, because a card
                        // wearing a glyph where the others wear pictures
                        // has no other explanation.
                        tracing::warn!(id = key.0, "a mark that would not draw");
                        self.atlas.no_mark(key);
                        continue;
                    };
                    let rgba = drawn.to_rgba8();
                    let (width, height) = (rgba.width(), rgba.height());
                    let spot = self.atlas.place(&self.queue, width, height, &rgba, true);
                    self.atlas.marked(key, spot);
                    match spot {
                        Some(spot) => spot,
                        None => continue,
                    }
                }
            };
            self.quads.push(Quad {
                rect: [
                    f32::from(mark.x) * cell.width,
                    f32::from(mark.y) * cell.height,
                    cell.width * f32::from(SLOT.width),
                    cell.height * f32::from(SLOT.height),
                ],
                uv: spot.uv,
                // The picture carries its own colours, and the instance's
                // alpha is the whole of what it adds.
                colour: [1.0, 1.0, 1.0, 1.0],
                flags: COLOURFUL,
                radius: 0.0,
                padding: [0; 2],
            });
        }
    }

    /// The caret, in the shape that says what the next character will do.
    ///
    /// A bar stands between two characters and says the next one goes
    /// there; a block stands on one and says the next one takes its place.
    /// Which is the mode, and the application is what knows it.
    ///
    /// The block is the cell with its colours the other way round, which is
    /// what a terminal does and for the same reason: a block that hid the
    /// character under it would be a caret a reader cannot read past.
    fn caret(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        drift: (f32, f32),
    ) {
        let Some(caret) = page.caret() else {
            return;
        };
        let cell = fonts.cell();
        // Inside the word being spelled, where the input method says it is:
        // a caret left at the start of it would be a caret in the wrong
        // half of what the reader is typing.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a caret is a few columns into what is being spelled"
        )]
        let along = spelling.map_or(0, |spelling| spelling.columns() as u16);
        let x = caret.x.saturating_add(along);
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        // Where it is drawn, which is where the page put it only once it
        // has got there -- see `motion::Moving::drift`. Everything the
        // caret carries is drawn from these two, so a caret in flight
        // takes the character it is going to cover along with it rather
        // than leaving it behind on the cell.
        let left = (f32::from(x) + drift.0) * cell.width;
        let top = (f32::from(caret.y) + drift.1) * cell.height;
        // Inside a word being spelled the caret is always a bar: what is
        // under it there is the spelling itself, which is being written
        // rather than typed over, and a block would hide the character the
        // reader is in the middle of choosing.
        let shape = match spelling.is_some() {
            true => Caret::Bar,
            false => page.shape(),
        };
        match shape {
            // Narrow, and never thinner than a pixel: a caret that rounds
            // to nothing is a caret nobody can find.
            Caret::Bar => self.block(
                left,
                top,
                (cell.width * BAR).round().max(1.0),
                cell.height,
                ink,
            ),
            Caret::Block => {
                self.block(left, top, cell.width, cell.height, ink);
                let behind = rgba(look.background, Ink::Background);
                // What is under the caret, drawn again in the colour behind
                // it, so that a block does not hide the character it is on.
                if !look.text.trim().is_empty() {
                    self.glyphs_at(left, top, look, behind, fonts);
                }
            }
        }
    }

    /// What an input method is spelling, drawn where the word will go.
    ///
    /// Over the cells to the right of the caret rather than pushing them
    /// along: the file has not changed, and a window that reflowed the line
    /// for a word that may never be committed would be showing the reader a
    /// file that does not exist. Underlined, which is what says these
    /// characters are not in the file yet.
    ///
    /// In the colours of the place it is being typed into, because that is
    /// the only theme the window has: the cells say what the page's ink and
    /// paper are here.
    fn spelling(&mut self, page: &Page, fonts: &mut Fonts, spelling: Option<&Spelling>) {
        let (Some(spelling), Some(caret)) = (spelling, page.caret()) else {
            return;
        };
        let cell = fonts.cell();
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        let paper = rgba(look.background, Ink::Background);
        let mut column = caret.x;
        for character in spelling.text.chars() {
            if column >= page.columns() {
                // Off the edge. Clipped rather than wrapped: the row below
                // belongs to the next line of the file.
                break;
            }
            let written = character.to_string();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the width of one character, which is one or two"
            )]
            let wide = obelus_text::text_width(&written).max(1) as u16;
            let left = f32::from(column) * cell.width;
            let top = f32::from(caret.y) * cell.height;
            let width = f32::from(wide) * cell.width;
            self.block(left, top, width, cell.height, paper);
            let over = Look {
                text: &written,
                foreground: look.foreground,
                background: look.background,
                modifier: look.modifier,
            };
            self.glyphs(column, caret.y, over, ink, fonts);
            // The line under it, which is what every input method's inline
            // spelling wears and what tells it apart from the file.
            let thick = (cell.height * UNDERLINE).round().max(1.0);
            self.block(left, top + cell.height - thick, width, thick, ink);
            column = column.saturating_add(wide);
        }
    }
}

impl Atlas {
    /// A texture with one white pixel in it, which is where every rectangle
    /// that has no picture gets its picture.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("obelus glyphs"),
            size: wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: what is in here is coverage and emoji, and the
            // colours it is multiplied by are the theme's own.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the atlas is a thousand pixels across"
        )]
        let mut allocator =
            etagere::AtlasAllocator::new(etagere::size2(ATLAS as i32, ATLAS as i32));
        // The one opaque pixel. Allocated first so that it is there before
        // anything asks, and through the allocator so that nothing else is
        // ever put on top of it.
        let white = allocator
            .allocate(etagere::size2(1, 1))
            .expect("an empty atlas has room for one pixel");
        let corner = white.rectangle.min;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    #[expect(clippy::cast_sign_loss, reason = "an allocation is never negative")]
                    x: corner.x as u32,
                    #[expect(clippy::cast_sign_loss, reason = "an allocation is never negative")]
                    y: corner.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &[0xff, 0xff, 0xff, 0xff],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        // Half a pixel in, so that no filtering can reach a neighbour.
        #[expect(
            clippy::cast_precision_loss,
            reason = "the atlas is a thousand pixels across"
        )]
        let middle = [
            (corner.x as f32 + 0.5) / ATLAS as f32,
            (corner.y as f32 + 0.5) / ATLAS as f32,
        ];
        Self {
            texture,
            view,
            allocator,
            spots: HashMap::new(),
            marks: HashMap::new(),
            white: [middle[0], middle[1], middle[0], middle[1]],
        }
    }

    /// Everything in it is the wrong size now.
    fn empty(&mut self) {
        self.spots.clear();
        self.marks.clear();
        // The white pixel keeps its place: the allocator is not cleared,
        // because the one thing in it that is not a glyph is still right.
    }

    /// The marks are the wrong colour now, which a theme change makes them.
    fn forget_the_marks(&mut self) {
        self.marks.clear();
    }

    /// Where a mark is, if it has been drawn at this size and in these
    /// colours.
    fn mark(&self, key: &(String, bool)) -> Option<Spot> {
        self.marks.get(key).copied().flatten()
    }

    /// Remembers where one landed.
    fn marked(&mut self, key: (String, bool), spot: Option<Spot>) {
        self.marks.insert(key, spot);
    }

    /// And remembers that one cannot be drawn at all.
    fn no_mark(&mut self, key: (String, bool)) {
        self.marks.insert(key, None);
    }

    /// Puts pixels in, and says where they went.
    ///
    /// The one piece of code that writes to the texture: a glyph and a
    /// mark differ in where their pixels come from and in nothing else.
    fn place(
        &mut self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
        colourful: bool,
    ) -> Option<Spot> {
        if width == 0 || height == 0 {
            return None;
        }
        #[expect(
            clippy::cast_possible_wrap,
            reason = "what is put in is smaller than the atlas, which is a thousand pixels"
        )]
        let wanted = etagere::size2(width as i32, height as i32);
        let allocation = match self.allocator.allocate(wanted) {
            Some(allocation) => allocation,
            None => {
                // Full. Everything in it is thrown away and whatever is
                // still wanted is drawn again as it is asked for, which
                // costs one frame and cannot fail twice: a session that
                // filled it did so over thousands of frames.
                tracing::debug!("the glyph atlas is full, and is being started again");
                self.allocator.clear();
                self.spots.clear();
                self.marks.clear();
                self.allocator.allocate(wanted)?
            }
        };
        let corner = allocation.rectangle.min;
        #[expect(clippy::cast_sign_loss, reason = "an allocation is never negative")]
        let (x, y) = (corner.x as u32, corner.y as u32);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "the atlas is a thousand pixels across"
        )]
        let uv = [
            x as f32 / ATLAS as f32,
            y as f32 / ATLAS as f32,
            (x + width) as f32 / ATLAS as f32,
            (y + height) as f32 / ATLAS as f32,
        ];
        #[expect(
            clippy::cast_precision_loss,
            reason = "what is put in is a few dozen pixels each way"
        )]
        Some(Spot {
            uv,
            width: width as f32,
            height: height as f32,
            left: 0.0,
            top: 0.0,
            colourful,
        })
    }

    /// Where a glyph is, putting it in if this is the first time it has
    /// been asked for.
    fn spot(&mut self, queue: &wgpu::Queue, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        if let Some(known) = self.spots.get(&key) {
            return *known;
        }
        let spot = self.rasterise(queue, fonts, key);
        self.spots.insert(key, spot);
        spot
    }

    /// Draws one glyph and finds it a place.
    fn rasterise(&mut self, queue: &wgpu::Queue, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        let picture = fonts.picture(key)?;
        let width = picture.placement.width;
        let height = picture.placement.height;
        let colourful = matches!(picture.content, SwashContent::Color);
        let pixels = match picture.content {
            // Coverage: the alpha is the whole of it, and the colour comes
            // from the instance.
            SwashContent::Mask => picture
                .data
                .iter()
                .flat_map(|coverage| [0xff, 0xff, 0xff, *coverage])
                .collect::<Vec<u8>>(),
            SwashContent::Color => picture.data.clone(),
            // Three coverages, one per subpixel. Obelus does not draw
            // subpixel text -- it would be wrong on a rotated screen and on
            // every screen that is not RGB -- so the middle one is taken as
            // the coverage.
            SwashContent::SubpixelMask => picture
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|texel| [0xff, 0xff, 0xff, texel[1]])
                .collect::<Vec<u8>>(),
        };
        // Where the glyph sits against the pen, which is the one thing a
        // mark has no use for: a mark is put where it was told.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a glyph is offset by a few dozen pixels"
        )]
        let (left, top) = (picture.placement.left as f32, picture.placement.top as f32);
        let spot = self.place(queue, width, height, &pixels, colourful)?;
        Some(Spot { left, top, ..spot })
    }
}

/// The runs of one colour along a row, as few as they can be said in.
///
/// Two things are settled here. A screen is mostly the page's own colour,
/// so a rectangle per cell would be ten thousand of them to say one thing.
/// And a full-width character owns both of its columns: the second one is a
/// cell `ratatui` has reset, with no text and no colours, which a terminal
/// never draws because it advanced two columns itself. A window draws every
/// cell, so reading that cell's own background painted the default colour
/// behind the right half of every Chinese character -- a line of them came
/// out striped.
fn runs(page: &Page, row: u16) -> Vec<(u16, u16, Color)> {
    let mut runs: Vec<(u16, u16, Color)> = Vec::new();
    // How many columns of the character just seen are still to come.
    let mut rest = 0;
    for column in 0..page.columns() {
        let colour = match rest {
            0 => {
                let look = page.look(column, row);
                rest = look.columns() - 1;
                look.background
            }
            // The rest of a character that was seen already, so there is
            // always a run to take the colour from.
            _ => {
                rest -= 1;
                runs.last().map_or(Color::Reset, |&(_, _, running)| running)
            }
        };
        match runs.last_mut() {
            Some((_, end, running)) if *running == colour && *end == column => *end = column + 1,
            _ => runs.push((column, column + 1, colour)),
        }
    }
    runs
}

/// Which half of a cell a colour is for, which is the whole of what `Reset`
/// means.
#[derive(Clone, Copy, Debug)]
enum Ink {
    Foreground,
    Background,
}

/// A colour as the hardware wants it.
///
/// Straight through, with no conversion: the surface is viewed without its
/// sRGB conversion for exactly this reason. A theme's `#1e1e2e` is the
/// colour the reader picked, and a pipeline that corrects it draws a
/// different one.
fn rgba(colour: Color, ink: Ink) -> [f32; 4] {
    let (r, g, b) = match colour {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(index) => indexed(index),
        // What the reader's own terminal would have chosen, which here is
        // nobody: a window has no colours of its own, so it has to have an
        // answer. These are the two ends of the default palette below.
        Color::Reset => match ink {
            Ink::Foreground => (0xcc, 0xcc, 0xcc),
            Ink::Background => (0x0c, 0x0c, 0x0c),
        },
        Color::Black => indexed(0),
        Color::Red => indexed(1),
        Color::Green => indexed(2),
        Color::Yellow => indexed(3),
        Color::Blue => indexed(4),
        Color::Magenta => indexed(5),
        Color::Cyan => indexed(6),
        Color::Gray => indexed(7),
        Color::DarkGray => indexed(8),
        Color::LightRed => indexed(9),
        Color::LightGreen => indexed(10),
        Color::LightYellow => indexed(11),
        Color::LightBlue => indexed(12),
        Color::LightMagenta => indexed(13),
        Color::LightCyan => indexed(14),
        Color::White => indexed(15),
    };
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}

/// The palette a terminal would have had.
///
/// Obelus's themes name their colours in full, so this is reached only by a
/// theme that asked for one of the sixteen -- and by anything drawn before
/// a theme is loaded. The sixteen are xterm's, and the rest is the cube and
/// the grey ramp every terminal since has agreed on.
fn indexed(index: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    match index {
        0..=15 => BASE[index as usize],
        16..=231 => {
            let index = index - 16;
            (
                STEPS[(index / 36) as usize],
                STEPS[(index % 36 / 6) as usize],
                STEPS[(index % 6) as usize],
            )
        }
        232..=255 => {
            let grey = 8 + (index - 232) * 10;
            (grey, grey, grey)
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Cell;

    use super::*;
    use crate::grid::Update;

    /// A page with one row of text on it, for asking what would be drawn.
    fn page(text: &str) -> Page {
        let mut page = Page::default();
        page.resized(12, 1);
        let mut column = 0;
        for character in text.chars() {
            let written = character.to_string();
            let mut cell = Cell::default();
            cell.set_symbol(&written);
            cell.bg = Color::Rgb(1, 2, 3);
            let wide = obelus_text::text_width(&written).max(1);
            page.apply(Update::Cell {
                x: column,
                y: 0,
                cell: Box::new(cell),
            });
            // What `ratatui` leaves behind the right half of a wide
            // character: a cell with nothing in it and no colours.
            for rest in 1..wide {
                page.apply(Update::Cell {
                    x: column + rest as u16,
                    y: 0,
                    cell: Box::new(Cell::default()),
                });
            }
            column += wide as u16;
        }
        page
    }

    /// A full-width character's colour covers both of its columns.
    ///
    /// Deliberate break: reading each cell's own background -- which is
    /// what a terminal's front end can do, because a terminal draws the
    /// second half itself -- puts a run of the default colour between every
    /// pair of Chinese characters, and this counts them.
    #[test]
    fn a_wide_character_owns_the_colour_of_both_its_cells() {
        let page = page("\u{4e2d}\u{6587}");
        let coloured: Vec<_> = runs(&page, 0)
            .into_iter()
            .filter(|&(_, _, colour)| colour == Color::Rgb(1, 2, 3))
            .collect();
        assert_eq!(coloured, vec![(0, 4, Color::Rgb(1, 2, 3))]);
    }

    /// And the rest of the row is still said in as few runs as it can be.
    ///
    /// Deliberate break: a rectangle per cell -- the obvious way to write
    /// this -- makes it twelve.
    #[test]
    fn a_row_is_as_few_runs_as_it_can_be() {
        assert_eq!(runs(&page("ab"), 0).len(), 2);
    }

    /// The colours a theme names are the colours that are drawn.
    ///
    /// Deliberate break: converting from sRGB to linear on the way through
    /// -- which is what a pipeline drawing to an sRGB surface has to do --
    /// moves every one of these.
    #[test]
    fn a_colour_goes_through_as_it_was_written() {
        let [r, g, b, a] = rgba(Color::Rgb(0x1e, 0x1e, 0x2e), Ink::Background);
        assert!((r - 30.0 / 255.0).abs() < f32::EPSILON);
        assert!((g - 30.0 / 255.0).abs() < f32::EPSILON);
        assert!((b - 46.0 / 255.0).abs() < f32::EPSILON);
        assert!((a - 1.0).abs() < f32::EPSILON);
    }

    /// The cube and the grey ramp are the ones every terminal agrees on.
    ///
    /// Deliberate break: stepping the cube evenly from zero to 255, which
    /// is the obvious thing and is not what a terminal does.
    ///
    /// Which is why the middle of the cube is what this asserts on. The
    /// ends of it were here first and are worth nothing: `21` and `196` are
    /// made of the first and last step, and those two are the same number
    /// in both -- so the even ramp passed, and a test that passes with the
    /// thing it covers broken is not a test.
    #[test]
    fn the_palette_is_the_one_a_terminal_has() {
        assert_eq!(indexed(0), (0x00, 0x00, 0x00));
        assert_eq!(indexed(16), (0x00, 0x00, 0x00));
        // One step along each axis of the cube: 16 + 36 + 2 * 6 + 3.
        assert_eq!(indexed(67), (0x5f, 0x87, 0xaf));
        assert_eq!(indexed(196), (0xff, 0x00, 0x00));
        assert_eq!(indexed(232), (0x08, 0x08, 0x08));
        assert_eq!(indexed(255), (0xee, 0xee, 0xee));
    }
}
