//! Taking over the window, and the passes that put a frame on it.
//!
//! What goes into the frame is laid out elsewhere (`lay`); what is here is
//! the device, the pictures the glass reads, and the order they are drawn
//! into.

use std::time::Instant;

use anyhow::{Context, Result};
use obelus_font::Fonts;

use super::*;
use crate::{
    grid::{Going, Page, Said, Spelling},
    motion::Moving,
};

/// What one pane's glass reads: what is behind the pane, as a picture of
/// the window; the same picture blurred; and the bindings that read both.
///
/// The blur is a picture of its own rather than worked out by the glass,
/// because a blur worked out a pixel at a time is a handful of taps, and a
/// handful of taps over text is a handful of faint copies of it -- a
/// picture that looks enlarged from one a fraction of its size. Blurred
/// one way and then the other, which is what a Gaussian allows and is a
/// few dozen samples a pixel where doing it at once would be several
/// hundred.
pub(super) struct Seen {
    backdrop: wgpu::TextureView,
    blurred: wgpu::TextureView,
    bindings: wgpu::BindGroup,
}

impl Seen {
    /// Both pictures: the sharp one the size of the window, and the blurred
    /// one half of it each way -- see `halved`.
    fn made(binder: &Binder<'_>, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let backdrop = made_to_draw_into(binder.device, format, width, height);
        let blurred = made_to_draw_into(binder.device, format, halved(width), halved(height));
        let bindings = binder.bound(&backdrop, &blurred);
        Self {
            backdrop,
            blurred,
            bindings,
        }
    }
}

/// A picture of the whole window, and the bindings that read it.
pub(super) struct Whole {
    view: wgpu::TextureView,
    bindings: wgpu::BindGroup,
}

impl Whole {
    fn made(
        binder: &Binder<'_>,
        nothing: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let view = made_to_draw_into(binder.device, format, width, height);
        let bindings = binder.bound(&view, nothing);
        Self { view, bindings }
    }
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
            // Small blocks, because the default reserves for a game: blocks
            // that start at 128 MiB on the card and 64 MiB in the machine's
            // own memory, where a window of 1882 by 2052 was measured using
            // 89 MiB in all. Several Obelus windows open at once is the
            // normal case, and on NVIDIA that window went from 506 MiB to
            // 156. Vulkan and DX12 are told; Metal and GL take no notice.
            //
            // The smallest there is, and fixed, rather than `MemoryUsage`,
            // whose blocks double up to 64 MiB and still kept 246: the
            // pictures the size of the window are each bigger than a block,
            // so each gets memory of its own and gives it back whole. What
            // that costs is a resize -- about 3ms where it was 1.5 -- and
            // nothing a frame was seen to.
            memory_hints: wgpu::MemoryHints::Manual {
                suballocated_device_memory_block_size: (4 << 20)..(4 << 20),
            },
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
        let capabilities = surface.get_capabilities(&adapter);
        // Waiting for the screen rather than racing it: Obelus draws when
        // something happened, so there is never a frame to throw away.
        //
        // Except on Wayland, where FIFO is the compositor's to release, and
        // Hyprland on NVIDIA was seen not to: every acquire ran out its
        // second, so a window drew a frame a second for most of a minute
        // with nothing busy on either side -- with the driver's explicit
        // sync turned off as well. Mailbox never waits on that, and the
        // pacing FIFO gave an animation comes from the frame callback
        // instead, which `pre_present_notify` asks winit for. Only there,
        // because elsewhere that call paces nothing and Mailbox would draw
        // an animation as fast as the card can.
        configured.present_mode = if on_wayland(&window)
            && capabilities
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };
        // Said, because the default is whatever the platform would rather
        // do and on a Wayland compositor that is to honour the alpha
        // channel: a page drawn in a theme's own dark background came out
        // with the wallpaper showing through it. Obelus's window is not a
        // transparent window.
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
            present = ?configured.present_mode,
            presents = ?capabilities.present_modes,
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
                    // The fragment stage reads it too, since the glass:
                    // what it samples is the window, so it has to know how
                    // big the window is.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        // Smooth, unlike the glyphs': what this samples is a picture being
        // bent, and bending it a pixel at a time is what a staircase is.
        let smooth = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("obelus behind"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let (width, height) = (configured.width, configured.height);
        let binder = Binder {
            device: &device,
            layout: &layout,
            uniforms: &uniforms,
            letters: &atlas.letters.view,
            pictures: &atlas.pictures.view,
            sampler: &sampler,
            smooth: &smooth,
        };
        let nothing = made_to_draw_into(&device, view, 1, 1);
        let levels = vec![Seen::made(&binder, view, width, height)];
        let scratch = made_to_draw_into(&device, view, halved(width), halved(height));
        let scratch_bindings = binder.bound(&scratch, &nothing);
        let plain_bindings = binder.bound(&nothing, &nothing);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("obelus"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../paint.wgsl").into()),
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
                        5 => Uint32,
                        6 => Float32,
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
            window,
            surface,
            device,
            queue,
            configured,
            view,
            pipeline,
            uniforms,
            atlas,
            drawings: HashMap::new(),
            palette: None,
            ground: Color::Reset,
            grid: [0.0, 0.0],
            margin: [0.0, 0.0],
            titled: 0.0,
            holding: (Color::Reset, Color::Reset),
            quads: Vec::new(),
            placed: Placed::default(),
            instances,
            layout,
            sampler,
            smooth,
            levels,
            scratch,
            scratch_bindings,
            picture: None,
            left: None,
            gone: Vec::new(),
            plain_bindings,
            nothing,
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
        // Every picture is a picture of the window, so it is the size of
        // the window: one that stayed the old size would be sampled at the
        // wrong place for every pixel of the glass.
        let (width, height) = (self.configured.width, self.configured.height);
        let binder = self.binder();
        let levels = (0..self.levels.len())
            .map(|_| Seen::made(&binder, self.view, width, height))
            .collect();
        let scratch = made_to_draw_into(&self.device, self.view, halved(width), halved(height));
        let scratch_bindings = binder.bound(&scratch, &self.nothing);
        let picture = self
            .picture
            .as_ref()
            .map(|_| Whole::made(&binder, &self.nothing, self.view, width, height));
        self.levels = levels;
        (self.scratch, self.scratch_bindings) = (scratch, scratch_bindings);
        self.picture = picture;
        // A picture of the window at its old size is a pane leaving from the
        // wrong place, so what was leaving is simply gone.
        self.left = None;
        self.gone.clear();
    }

    /// A picture of the whole window, at the size it is now.
    fn whole(&self) -> Whole {
        let (width, height) = (self.configured.width, self.configured.height);
        Whole::made(&self.binder(), &self.nothing, self.view, width, height)
    }

    /// Makes the picture of the frame when the frame just laid out may
    /// draw into it, and lets it go when it may not.
    ///
    /// Kept for as long as anything is over the page rather than only while
    /// something moves, because the band scrolling under a pane wants it at
    /// every press of a key that scrolls: made at the press, it would be a
    /// picture the size of the window made and let go again at every one.
    ///
    /// What making it costs is the first frame of a pane coming in: 1.6ms
    /// where it was 1.25, measured over a dozen openings, for 31 MiB of a
    /// 1882 by 2052 window given back while nothing is open.
    fn picture_while_wanted(&mut self) {
        let placed = &self.placed;
        let wanted = placed.composed || placed.under.is_some() || !placed.levels.is_empty();
        match (wanted, self.picture.is_some()) {
            (true, false) => self.picture = Some(self.whole()),
            (false, true) => self.picture = None,
            _ => {}
        }
    }

    /// Every bind group again, over the same pictures, because the atlas
    /// they read is a different texture now.
    pub(super) fn bound_again(&mut self) {
        let binder = self.binder();
        let levels: Vec<wgpu::BindGroup> = self
            .levels
            .iter()
            .map(|seen| binder.bound(&seen.backdrop, &seen.blurred))
            .collect();
        let scratch = binder.bound(&self.scratch, &self.nothing);
        let showing = (self.picture.as_ref()).map(|whole| binder.bound(&whole.view, &self.nothing));
        let left = (self.left.as_ref()).map(|whole| binder.bound(&whole.view, &self.nothing));
        let plain = binder.bound(&self.nothing, &self.nothing);
        for (seen, bindings) in self.levels.iter_mut().zip(levels) {
            seen.bindings = bindings;
        }
        self.scratch_bindings = scratch;
        if let (Some(whole), Some(bindings)) = (self.picture.as_mut(), showing) {
            whole.bindings = bindings;
        }
        if let (Some(whole), Some(bindings)) = (self.left.as_mut(), left) {
            whole.bindings = bindings;
        }
        self.plain_bindings = plain;
    }

    /// What every bind group is made of, but the two pictures it reads.
    fn binder(&self) -> Binder<'_> {
        Binder {
            device: &self.device,
            layout: &self.layout,
            uniforms: &self.uniforms,
            letters: &self.atlas.letters.view,
            pictures: &self.atlas.pictures.view,
            sampler: &self.sampler,
            smooth: &self.smooth,
        }
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
        said: Said<'_>,
    ) -> Result<()> {
        self.lay(page, fonts, spelling, moving, said);
        self.picture_while_wanted();
        if self.placed.going.is_none() {
            self.left = None;
        }
        let acquiring = Instant::now();
        let acquired = self.surface.get_current_texture();
        let waited = acquiring.elapsed();
        if waited >= crate::window::SLOW {
            tracing::warn!(?waited, "the surface took this long to give up a frame");
        }
        let frame = match acquired {
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
        self.encode(&mut encoder, &target);
        self.queue.submit([encoder.finish()]);
        // Only where a frame really goes: the callback this asks for comes
        // with a commit, and one asked for with nothing committed is a
        // redraw winit holds back for ever.
        self.window.pre_present_notify();
        let presenting = Instant::now();
        self.queue.present(frame);
        let waited = presenting.elapsed();
        if waited >= crate::window::SLOW {
            tracing::warn!(?waited, "the surface took this long to take a frame");
        }
        Ok(())
    }

    /// The screen as it is, kept as a picture to draw the panes that have
    /// just gone from it leaving out of.
    ///
    /// Drawn once, at the moment they went, from the page and what was said
    /// about it then -- which the window kept, because the frame that took
    /// them away has nothing of them on it. Laid out the way every frame is,
    /// glass and all, so what leaves is what was on the screen; and into a
    /// picture of its own, which nothing else draws into, so that every
    /// frame of the leaving reads the same one.
    pub(crate) fn keep(&mut self, page: &Page, fonts: &mut Fonts, said: Said<'_>, going: &[Going]) {
        let cell = fonts.cell();
        self.lay(page, fonts, None, Moving::still(), said);
        // Cut where its glass is, which is where it was cut from what was
        // under it: the half row above a list's rule is the file, and taken
        // along it would be a strip of the file going down with the list.
        self.gone = going
            .iter()
            .map(|gone| {
                let level = self
                    .placed
                    .levels
                    .iter()
                    .find(|level| level.pane && level.area == gone.area);
                (
                    level.map_or_else(|| box_of(gone.area, cell), |level| level.rect),
                    gone.joined,
                )
            })
            .collect();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("obelus kept"),
            });
        self.picture_while_wanted();
        if self.left.is_none() {
            self.left = Some(self.whole());
        }
        if let Some(left) = &self.left {
            self.encode(&mut encoder, &left.view);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// What the passes that draw the laid-out frame are, into `target`.
    fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let placed = self.placed.clone();
        // What is behind the first pane, into a picture of its own. Only the
        // front of the buffer, which is exactly those cells -- and only
        // where there is a pane at all, so a window with nothing over the
        // page does none of this.
        match placed.under.clone() {
            // A band scrolling under the pane: what is behind it is drawn
            // where it stands into the frame's picture, which nothing has
            // drawn into yet, and put back into the backdrop with the band
            // taken from where it has got to -- the same two steps the
            // screen is put back in, so what the glass shows moves with
            // what is round it.
            Some((above, put_back)) => {
                if let Some(picture) = &self.picture {
                    {
                        let mut pass = self.pass(encoder, "obelus behind, standing", &picture.view);
                        pass.set_bind_group(0, &self.plain_bindings, &[]);
                        drawing(&mut pass, 0..placed.behind());
                        drawing(&mut pass, above);
                    }
                    let mut pass = self.pass(encoder, "obelus behind", &self.levels[0].backdrop);
                    pass.set_bind_group(0, &picture.bindings, &[]);
                    drawing(&mut pass, put_back);
                }
            }
            None if !placed.levels.is_empty() => {
                let mut pass = self.pass(encoder, "obelus behind", &self.levels[0].backdrop);
                self.beneath(&mut pass, 0);
            }
            None => {}
        }
        // And each one over it, in order: a picture of the screen as it was
        // before that one was put over it, which reads the pictures before
        // it and so has to come after them. Reading one while drawing into
        // another, which is two textures and allowed.
        for (at, level) in placed.levels.iter().enumerate() {
            if at > 0 {
                let mut pass =
                    self.pass(encoder, "obelus behind the next", &self.levels[at].backdrop);
                self.beneath(&mut pass, at);
            }
            self.blurring(encoder, level.blur, &self.levels[at]);
        }
        // A pane on its way in: the frame goes into a picture of its own
        // first, and the screen is put together out of it below. Only
        // while one is moving -- an arrived pane is drawn straight to the
        // screen like everything else.
        let composing = self.picture.as_ref().filter(|_| placed.composed);
        if let Some(picture) = composing {
            // The glass is drawn in here, and what it reads is the
            // backdrop -- which this pass is not writing to.
            let mut pass = self.pass(encoder, "obelus frame", &picture.view);
            self.the_frame(&mut pass);
        }
        {
            // Cleared to black, which is never seen: the first quad of
            // every frame is the whole window in the page's own ground,
            // and the cells are drawn over that.
            let mut pass = self.pass(encoder, "obelus", target);
            match composing {
                // The page first, and nothing of the pane: the glass is
                // part of the pane and arrives with it. Left standing in
                // place, it was a pane already open with only its words
                // sliding into it -- which is not what the list does.
                Some(picture) => {
                    // What the pane on top was put over, which is the
                    // picture its glass reads.
                    if let Some(top) = placed.levels.iter().rposition(|level| level.pane) {
                        self.beneath(&mut pass, top);
                    }
                    self.follow(&mut pass, &catching(&placed), &picture.bindings);
                }
                // And a pane that went, over the frame it went from.
                None => {
                    self.the_frame(&mut pass);
                    if let Some(going) = placed.going
                        && let Some(left) = &self.left
                    {
                        pass.set_bind_group(0, &left.bindings, &[]);
                        drawing(&mut pass, going);
                    }
                }
            }
        }
    }

    /// What the page is drawn on, for the margin round the grid.
    ///
    /// Handed over every frame rather than when the application says it:
    /// the painter is built after the application is told who is drawing,
    /// so the first one would have nowhere to land.
    pub(crate) const fn drawn_on(&mut self, ground: Color) {
        self.ground = ground;
    }

    /// Which colours mean the reader has hold of something -- see
    /// `Drawing::holding`. Handed over every frame, like the ground, and
    /// for the same reason.
    pub(crate) const fn holding(&mut self, holding: (Color, Color)) {
        self.holding = holding;
    }

    /// How much of the top of the window is the title bar's. Handed over
    /// every frame, like the ground: the window measures it when it
    /// changes size, and a second copy kept here would be one to forget.
    pub(crate) const fn titled(&mut self, titled: f32) {
        self.titled = titled;
    }

    /// What is behind a pane, blurred: across into the scratch picture,
    /// then down out of it into the pane's own.
    fn blurring(&self, encoder: &mut wgpu::CommandEncoder, first: usize, seen: &Seen) {
        for (target, bindings, quad) in [
            (&self.scratch, &seen.bindings, first),
            (&seen.blurred, &self.scratch_bindings, first + 1),
        ] {
            let mut pass = self.pass(encoder, "obelus blur", target);
            pass.set_bind_group(0, bindings, &[]);
            drawing(&mut pass, quad..quad + 1);
        }
    }

    /// A pass that draws into `target`, cleared, with everything set but
    /// what it reads.
    fn pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        label: &str,
        target: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'e> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
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
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass
    }

    /// The quads the screen draws, each with whatever it reads -- see
    /// `on_the_screen`.
    fn the_frame(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.follow(pass, &on_the_screen(&self.placed), &self.levels[0].bindings);
    }

    /// What one level was put over -- see `put_over`.
    fn beneath(&self, pass: &mut wgpu::RenderPass<'_>, level: usize) {
        self.follow(
            pass,
            &put_over(&self.placed.levels, level),
            &self.plain_bindings,
        );
    }

    /// Draws a plan: each run of quads with the pictures it reads.
    fn follow(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        plan: &[(Range<usize>, Reads)],
        otherwise: &wgpu::BindGroup,
    ) {
        for (quads, reads) in plan {
            let bindings = match reads {
                Reads::Nothing => otherwise,
                Reads::Level(level) => &self.levels[*level].bindings,
                Reads::Left => self.left.as_ref().map_or(otherwise, |left| &left.bindings),
            };
            pass.set_bind_group(0, bindings, &[]);
            drawing(pass, quads.clone());
        }
    }

    /// As many pictures as there are levels to read them.
    pub(super) fn levels_for(&mut self, wanted: usize) {
        while self.levels.len() < wanted {
            let (width, height) = (self.configured.width, self.configured.height);
            let seen = Seen::made(&self.binder(), self.view, width, height);
            self.levels.push(seen);
        }
    }
}

/// A run of quads, drawn; nothing where the run is empty.
fn drawing(pass: &mut wgpu::RenderPass<'_>, quads: std::ops::Range<usize>) {
    if quads.start < quads.end {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a frame is thousands of quads, not billions"
        )]
        pass.draw(0..4, quads.start as u32..quads.end as u32);
    }
}

/// A colour as the hardware wants it.
///
/// Straight through, with no conversion: the surface is viewed without its
/// sRGB conversion for exactly this reason. A theme's `#1e1e2e` is the
/// colour the reader picked, and a pipeline that corrects it draws a
/// different one.
/// How big a blurred picture is, against the window: half, each way.
///
/// A quarter of the memory and a quarter of the work of one the size of the
/// window, and nothing lost to the eye, because what is blurred has no detail
/// a pixel of the window would show -- the spread is five and a half of them,
/// and the glass reads it through a smooth sampler that puts the pixels back
/// in between. The sharp picture stays whole: the rim lets it through, and
/// the rim is where the glass bends the lines behind it.
pub(super) fn halved(pixels: u32) -> u32 {
    pixels.div_ceil(2)
}

/// A texture the size of the window, to draw a frame into and read back.
fn made_to_draw_into(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("obelus behind"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Everything the shader is handed but the two pictures a bind group
/// reads, in one place because the pictures are made again whenever the
/// window changes size and the rest goes with them.
///
/// Seven things that are the same in every one of them, kept together so
/// that what differs between bind groups is all that is written at each.
struct Binder<'a> {
    device: &'a wgpu::Device,
    layout: &'a wgpu::BindGroupLayout,
    uniforms: &'a wgpu::Buffer,
    letters: &'a wgpu::TextureView,
    pictures: &'a wgpu::TextureView,
    sampler: &'a wgpu::Sampler,
    smooth: &'a wgpu::Sampler,
}

impl Binder<'_> {
    /// A bind group reading `backdrop` as what is behind a pane and
    /// `blurred` as the same, blurred.
    fn bound(&self, backdrop: &wgpu::TextureView, blurred: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("obelus"),
            layout: self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(self.letters),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(backdrop),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(self.smooth),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(blurred),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(self.pictures),
                },
            ],
        })
    }
}
