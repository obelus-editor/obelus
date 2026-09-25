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
use ratatui::style::{Color, Modifier};
use winit::window::Window;

use crate::{
    font::Fonts,
    grid::{Look, Page},
};

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
    /// The hardware wants the whole thing aligned; nothing reads these.
    padding: [u32; 3],
}

/// A rectangle with no picture: its colour is the whole of it.
const SOLID: u32 = 1;
/// A picture with colours of its own, which is an emoji.
const COLOURFUL: u32 = 2;

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
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .context("the graphics adapter would not open a device")?;

        let mut configured = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
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
            quads: Vec::new(),
            instances,
        })
    }

    /// The window changed size, so the surface has to.
    pub(crate) fn resized(&mut self, width: u32, height: u32) {
        self.configured.width = width.max(1);
        self.configured.height = height.max(1);
        self.surface.configure(&self.device, &self.configured);
    }

    /// The text is a different size now, so nothing kept about its glyphs
    /// is about this size.
    pub(crate) fn forget_the_glyphs(&mut self) {
        self.atlas.empty();
    }

    /// Draws a page.
    pub(crate) fn paint(&mut self, page: &Page, fonts: &mut Fonts) -> Result<()> {
        let cell = fonts.cell();
        self.quads.clear();
        self.backgrounds(page, cell.width, cell.height);
        self.letters(page, fonts);
        self.caret(page, fonts);

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
            let mut run: Option<(u16, u16, [f32; 4])> = None;
            let draw = |painter: &mut Self, start: u16, end: u16, colour: [f32; 4]| {
                let wide = match end == page.columns() {
                    true => (right - f32::from(start) * width).max(width),
                    false => f32::from(end - start) * width,
                };
                painter.block(
                    f32::from(start) * width,
                    f32::from(row) * height,
                    wide,
                    tall,
                    colour,
                );
            };
            for column in 0..page.columns() {
                let look = page.look(column, row);
                let colour = rgba(look.background, Ink::Background);
                match run {
                    Some((start, end, running)) if running == colour && end == column => {
                        run = Some((start, column + 1, running));
                    }
                    Some((start, end, running)) => {
                        draw(self, start, end, running);
                        run = Some((column, column + 1, colour));
                    }
                    None => run = Some((column, column + 1, colour)),
                }
            }
            if let Some((start, end, running)) = run {
                draw(self, start, end, running);
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
            padding: [0; 3],
        });
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
        let left = f32::from(column) * cell.width;
        let top = f32::from(row) * cell.height;
        let bold = look.modifier.contains(Modifier::BOLD);
        let italic = look.modifier.contains(Modifier::ITALIC);
        let placed = fonts.glyphs(look.text, bold, italic).to_vec();
        for glyph in placed {
            let Some(spot) = self.atlas.spot(&self.device, &self.queue, fonts, glyph.key) else {
                continue;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "a glyph is offset by pixels, and there are few of them"
            )]
            let (x, y) = (glyph.x as f32, glyph.y as f32);
            self.quads.push(Quad {
                rect: [
                    left + x + spot.left,
                    top + cell.baseline + y - spot.top,
                    spot.width,
                    spot.height,
                ],
                uv: spot.uv,
                colour,
                flags: match spot.colourful {
                    true => COLOURFUL,
                    false => 0,
                },
                padding: [0; 3],
            });
        }
    }

    /// The caret, which is the cell it is in with its colours the other way
    /// round.
    ///
    /// The same thing a terminal does with it, and for the same reason: a
    /// block that hid the character under it would be a caret a reader
    /// cannot read past.
    fn caret(&mut self, page: &Page, fonts: &mut Fonts) {
        let Some(caret) = page.caret() else {
            return;
        };
        let cell = fonts.cell();
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        self.block(
            f32::from(caret.x) * cell.width,
            f32::from(caret.y) * cell.height,
            cell.width,
            cell.height,
            ink,
        );
        let behind = rgba(look.background, Ink::Background);
        if !look.text.trim().is_empty() {
            self.glyphs(caret.x, caret.y, look, behind, fonts);
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
            white: [middle[0], middle[1], middle[0], middle[1]],
        }
    }

    /// Everything in it is the wrong size now.
    fn empty(&mut self) {
        self.spots.clear();
        // The white pixel keeps its place: the allocator is not cleared,
        // because the one thing in it that is not a glyph is still right.
    }

    /// Where a glyph is, putting it in if this is the first time it has
    /// been asked for.
    fn spot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &mut Fonts,
        key: CacheKey,
    ) -> Option<Spot> {
        if let Some(known) = self.spots.get(&key) {
            return *known;
        }
        let spot = self.rasterise(device, queue, fonts, key);
        self.spots.insert(key, spot);
        spot
    }

    /// Draws one glyph and finds it a place.
    fn rasterise(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &mut Fonts,
        key: CacheKey,
    ) -> Option<Spot> {
        let picture = fonts.picture(key)?;
        let width = picture.placement.width;
        let height = picture.placement.height;
        if width == 0 || height == 0 {
            // A space, or a character the face draws as nothing. Worth
            // remembering as "nothing" so that it is not asked again.
            return None;
        }
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
        let left = picture.placement.left;
        let top = picture.placement.top;

        #[expect(
            clippy::cast_possible_wrap,
            reason = "a glyph is smaller than the atlas, which is a thousand pixels"
        )]
        let wanted = etagere::size2(width as i32, height as i32);
        let allocation = match self.allocator.allocate(wanted) {
            Some(allocation) => allocation,
            None => {
                // Full. Everything in it is thrown away and the glyphs that
                // are still wanted are drawn again as they are asked for,
                // which costs one frame and cannot fail twice: a session
                // that filled it did so over thousands of frames.
                tracing::debug!("the glyph atlas is full, and is being started again");
                self.allocator.clear();
                self.spots.clear();
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
            &pixels,
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
        let _ = device;

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
            reason = "a glyph is a few dozen pixels each way"
        )]
        Some(Spot {
            uv,
            width: width as f32,
            height: height as f32,
            left: left as f32,
            top: top as f32,
            colourful,
        })
    }
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
    use super::*;

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
