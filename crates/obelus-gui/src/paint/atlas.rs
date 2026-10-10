//! Where every glyph is kept: two textures of layers, and the room left in
//! them.

use cosmic_text::SwashContent;
use obelus_font::Fonts;

use super::*;

impl Atlas {
    /// Both textures, with nothing in them but the white pixel.
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut atlas = Self {
            device: device.clone(),
            queue: queue.clone(),
            // Not sRGB: what is in here is coverage and emoji, and the
            // colours it is multiplied by are the theme's own.
            letters: Layers::new(device, wgpu::TextureFormat::R8Unorm),
            pictures: Layers::new(device, wgpu::TextureFormat::Rgba8Unorm),
            remade: false,
            overflowed: false,
            spots: HashMap::new(),
            marks: HashMap::new(),
            white: [0.0; 4],
        };
        atlas.whiten();
        atlas
    }

    /// Puts the one opaque pixel in.
    ///
    /// First, so that it is there before anything asks and lands on the
    /// first layer, and through the room like everything else so that
    /// nothing is ever put on top of it.
    fn whiten(&mut self) {
        let white = self
            .place(1, 1, &[0xff], false)
            .expect("an empty texture has room for one pixel");
        // Half a pixel in, so that no filtering can reach a neighbour.
        let middle = [
            f32::midpoint(white.uv[0], white.uv[2]),
            f32::midpoint(white.uv[1], white.uv[3]),
        ];
        self.white = [middle[0], middle[1], middle[0], middle[1]];
    }

    /// Everything in it is the wrong size now.
    ///
    /// New textures rather than the old ones with their maps cleared: what
    /// the old size took is room nothing would ever be put in again.
    pub(super) fn empty(&mut self) {
        self.letters = Layers::new(&self.device, self.letters.format);
        self.pictures = Layers::new(&self.device, self.pictures.format);
        self.remade = true;
        self.spots.clear();
        self.marks.clear();
        self.whiten();
    }

    /// The marks are the wrong colour now, which a theme change makes them.
    pub(super) fn forget_the_marks(&mut self) {
        self.marks.clear();
    }

    /// Where a mark is, if it has been drawn at this size and in these
    /// colours.
    pub(super) fn mark(&self, key: &(String, bool)) -> Option<Spot> {
        self.marks.get(key).copied().flatten()
    }

    /// Remembers where one landed.
    pub(super) fn marked(&mut self, key: (String, bool), spot: Option<Spot>) {
        self.marks.insert(key, spot);
    }

    /// And remembers that one cannot be drawn at all.
    pub(super) fn no_mark(&mut self, key: (String, bool)) {
        self.marks.insert(key, None);
    }

    /// Puts pixels in, and says where they went.
    ///
    /// The one piece of code that writes to a texture: a glyph and a mark
    /// differ in where their pixels come from and in nothing else. A byte a
    /// pixel where the picture is not `colourful`, and four where it is.
    pub(super) fn place(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
        colourful: bool,
    ) -> Option<Spot> {
        if width == 0 || height == 0 {
            return None;
        }
        let layers = match colourful {
            true => &mut self.pictures,
            false => &mut self.letters,
        };
        let had = layers.room.layers.len();
        let most = self.device.limits().max_texture_array_layers;
        let Some((layer, [x, y])) = layers.room.find(width, height, most) else {
            // Full, where it would fit in a layer -- see `overflowed`. Said
            // once, rather than for every glyph the frame goes without.
            if width <= ATLAS && height <= ATLAS && !self.overflowed {
                tracing::warn!(most, "the glyph texture has as many layers as it can");
                self.overflowed = true;
            }
            return None;
        };
        if layers.room.layers.len() > had {
            layers.grow(&self.device, &self.queue, had);
            self.remade = true;
        }
        let bytes = match colourful {
            true => 4,
            false => 1,
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &layers.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes),
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
            reason = "a layer is a thousand pixels across"
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
            layer,
            width: width as f32,
            height: height as f32,
            left: 0.0,
            top: 0.0,
            colourful,
        })
    }

    /// Where a glyph is, putting it in if this is the first time it has
    /// been asked for.
    pub(super) fn spot(&mut self, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        if let Some(known) = self.spots.get(&key) {
            return *known;
        }
        let spot = self.rasterise(fonts, key);
        self.spots.insert(key, spot);
        spot
    }

    /// Draws one glyph and finds it a place.
    fn rasterise(&mut self, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        let picture = fonts.picture(key)?;
        let width = picture.placement.width;
        let height = picture.placement.height;
        let colourful = matches!(picture.content, SwashContent::Color);
        let pixels = match picture.content {
            // A letter's coverage, which is the whole of it: the colour
            // comes from the instance. And an emoji's colours, as they are.
            SwashContent::Mask | SwashContent::Color => picture.data.clone(),
            // Three coverages, one per subpixel. Obelus does not draw
            // subpixel text -- it would be wrong on a rotated screen and on
            // every screen that is not RGB -- so the middle one is taken as
            // the coverage.
            SwashContent::SubpixelMask => picture
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|texel| texel[1])
                .collect::<Vec<u8>>(),
        };
        // Where the glyph sits against the pen, which is the one thing a
        // mark has no use for: a mark is put where it was told.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a glyph is offset by a few dozen pixels"
        )]
        let (left, top) = (picture.placement.left as f32, picture.placement.top as f32);
        let spot = self.place(width, height, &pixels, colourful)?;
        Some(Spot { left, top, ..spot })
    }
}

impl Layers {
    /// A texture of `LAYERS` empty layers.
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture = layered(device, format, LAYERS);
        Self {
            view: array_of(&texture),
            texture,
            format,
            room: Room::new(LAYERS),
        }
    }

    /// As many layers as the room has opened, with what was in the `had`
    /// before them still where it was.
    ///
    /// A texture cannot be made bigger, so this is a new one with the old
    /// one copied into it -- and every place already handed out, in this
    /// frame or an earlier one, still holds what it held. Submitted at
    /// once, which puts it after every write already made to the old one:
    /// a queue's writes go before the next work handed to it.
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, had: usize) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device allows a few hundred layers"
        )]
        let had = had as u32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device allows a few hundred layers"
        )]
        let has = self.room.layers.len() as u32;
        let texture = layered(device, self.format, has);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("obelus glyphs"),
        });
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            texture.as_image_copy(),
            wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: had,
            },
        );
        queue.submit([encoder.finish()]);
        self.view = array_of(&texture);
        self.texture = texture;
        tracing::debug!(layers = has, format = ?self.format, "a glyph texture took another layer");
    }
}

impl Room {
    /// `layers` layers with nothing in them.
    fn new(layers: u32) -> Self {
        let mut room = Self { layers: Vec::new() };
        for _ in 0..layers {
            room.open();
        }
        room
    }

    /// A place for this, opening another layer where none has room -- so
    /// that what is already in the room stays where it is -- and nothing
    /// where no layer could hold it, or the device allows `most` layers
    /// and a step would take more.
    fn find(&mut self, width: u32, height: u32, most: u32) -> Option<(u32, [u32; 2])> {
        if let Some(taken) = self.take(width, height) {
            return Some(taken);
        }
        // Larger than a layer, which no number of layers has room for.
        if width > ATLAS || height > ATLAS {
            return None;
        }
        // Two short of it, because a step past a multiple of six is two.
        if self.layers.len() + 2 > most as usize {
            return None;
        }
        self.open();
        self.take(width, height)
    }

    /// The first place with room for this, and which layer it is on.
    ///
    /// The first layer first, which is where the white pixel goes.
    fn take(&mut self, width: u32, height: u32) -> Option<(u32, [u32; 2])> {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "what is put in is smaller than a layer, which is a thousand pixels"
        )]
        let wanted = etagere::size2(width as i32, height as i32);
        self.layers
            .iter_mut()
            .enumerate()
            .find_map(|(layer, room)| {
                let corner = room.allocate(wanted)?.rectangle.min;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a device allows a few hundred layers, and an allocation is never negative"
                )]
                Some((layer as u32, [corner.x as u32, corner.y as u32]))
            })
    }

    /// Another layer, with nothing in it -- or two, where one would make the
    /// count a multiple of six.
    ///
    /// Which wgpu's GL backend takes a square texture of to be a cube map,
    /// and a shader reading a cube map as an array reads nothing from it:
    /// on Mesa's software GL, six layers and twelve read nothing where five
    /// and seven were right. A layer nothing is put in yet is the cheaper
    /// way out than layers that are not square.
    fn open(&mut self) {
        loop {
            #[expect(
                clippy::cast_possible_wrap,
                reason = "a layer is a thousand pixels across"
            )]
            self.layers
                .push(etagere::AtlasAllocator::new(etagere::size2(
                    ATLAS as i32,
                    ATLAS as i32,
                )));
            if !self.layers.len().is_multiple_of(6) {
                return;
            }
        }
    }
}

/// A texture of `layers` layers of glyphs.
fn layered(device: &wgpu::Device, format: wgpu::TextureFormat, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("obelus glyphs"),
        size: wgpu::Extent3d {
            width: ATLAS,
            height: ATLAS,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // A source as well, because a texture that has to grow is copied
        // into the larger one.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The whole of a texture, as the array the shader reads it as.
fn array_of(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place handed out holds what was put there for as long as the room
    /// does: a full layer opens another, and nothing already in it moves.
    ///
    /// The atlas was emptied when it filled, half way through a frame, and
    /// the glyphs that frame had already placed were read from where the
    /// next ones had just been written: on a doubled screen a page of
    /// Chinese filled it, and a line number came out as a piece of a
    /// character until something drew the frame again.
    ///
    /// Through `find`, which is what `Atlas::place` asks. Deliberate break:
    /// make `find` clear every layer and take again where none has room,
    /// which is what the atlas did -- the next glyph lands on the white
    /// pixel.
    #[test]
    fn a_full_layer_opens_another_and_keeps_what_it_holds() {
        let mut room = Room::new(LAYERS);
        let (layer, [x, y]) = room.find(1, 1, 2048).expect("room for the white pixel");
        assert_eq!(
            layer, 0,
            "the white pixel is on the layer a quad reads by default"
        );
        let mut handed = vec![(layer, [x, y, 1, 1])];
        // Large, so that a few hundred of them fill more than the first two
        // layers.
        let (wide, tall) = (100, 100);
        for _ in 0..400 {
            let (layer, [x, y]) = room.find(wide, tall, 2048).expect("room on some layer");
            handed.push((layer, [x, y, wide, tall]));
        }
        for (at, (layer, [x, y, w, h])) in handed.iter().enumerate() {
            for (other, [ox, oy, ow, oh]) in &handed[..at] {
                let apart = layer != other
                    || x + w <= *ox
                    || ox + ow <= *x
                    || y + h <= *oy
                    || oy + oh <= *y;
                assert!(apart, "place {at} is on top of one handed out before it");
            }
        }
        assert!(
            handed.iter().any(|(layer, _)| *layer >= LAYERS),
            "enough was put in to need another layer"
        );
    }

    /// No count of layers a glyph texture is made with is a multiple of six,
    /// which wgpu's GL backend makes a cube map of -- and a shader reading
    /// that as an array reads nothing, so every letter goes.
    ///
    /// Deliberate break: let `open` push one layer and stop.
    #[test]
    fn a_glyph_texture_is_never_six_layers() {
        let mut room = Room::new(LAYERS);
        let mut counts = vec![room.layers.len()];
        for _ in 0..2000 {
            room.find(100, 100, 2048).expect("room on some layer");
            counts.push(room.layers.len());
        }
        assert!(
            counts.last().is_some_and(|&last| last > 12),
            "enough was put in to pass two multiples of six"
        );
        assert!(
            counts.iter().all(|count| !count.is_multiple_of(6)),
            "a texture was made of {counts:?} layers"
        );
    }

    /// A room at the device's limit says so rather than opening a layer the
    /// device cannot make.
    ///
    /// Deliberate break: drop the check against `most` in `find`.
    #[test]
    fn a_room_stops_at_the_layers_a_device_allows() {
        let mut room = Room::new(LAYERS);
        let mut placed = 0;
        while room.find(512, 512, 4).is_some() {
            placed += 1;
            assert!(placed < 100, "the room kept opening layers");
        }
        assert!(
            room.layers.len() <= 4,
            "{} layers on a device of four",
            room.layers.len()
        );
    }
}
