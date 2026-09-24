//! Pictures, where the terminal can draw them.
//!
//! A terminal that can show an image does it by being handed the pixels in
//! an escape sequence -- kitty's graphics protocol, iTerm2's inline images,
//! or sixels -- and every one of those is a different encoding of the same
//! request. `ratatui-image` does the encoding and the asking; what is here
//! is Obelus's answer to the two questions it leaves open: how big is the
//! space, and what happens when the terminal cannot do it at all.
//!
//! The answer to the second one is a Nerd Font glyph, which is why this
//! module reports [`Images::available`] rather than quietly falling back to
//! the half-block rendering `ratatui-image` also offers. A sixteen-pixel
//! mark pressed into two half-block cells is four coloured squares; the
//! glyph is a drawing of the thing. Half-blocks are for photographs, and
//! Obelus has no photographs.
//!
//! Detection happens once, before the alternate screen: it writes a query
//! to the terminal and reads the reply, which cannot be done from inside a
//! frame. A terminal that does not answer is a terminal that gets glyphs.

use std::collections::HashMap;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Rect, Size},
    style::Color,
    widgets::Widget,
};
use ratatui_image::{
    Image, Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};

/// How much room a mark gets, in cells.
///
/// Two by one, which is where a glyph would have gone. A cell is about
/// eight pixels by sixteen in every terminal font anybody reads code in, so
/// two of them side by side is the sixteen-pixel square these drawings are
/// drawn as -- the one size at which they need no scaling and look like
/// themselves.
pub const SLOT: Size = Size {
    width: 2,
    height: 1,
};

/// What a drawing was prepared for: whose it is, and whether its card had
/// the focus.
///
/// The focus is part of the key because the pixels behind the drawing are
/// the card's own background, and the focused card's background is a
/// different colour. Not for the look of it: sixels have no transparency,
/// so a mark drawn for one background is wrong on the other.
type Key = (String, bool);

/// The colours a mark is drawn with: what to ink it in, and the two
/// backgrounds a card can have.
///
/// Carried together because the whole cache depends on all three -- a
/// terminal is handed pixels, not a colour scheme, so a theme change means
/// every mark has to be drawn again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The colour the mark itself is drawn in.
    pub ink: Color,
    /// What is behind an ordinary card.
    pub paper: Color,
    /// What is behind the focused one.
    pub selected: Color,
}

/// The marks, encoded for whatever the terminal turned out to be.
pub struct Images {
    /// How to encode, and how big a cell is. `None` when the terminal
    /// cannot show a picture, which is most of the time.
    picker: Option<Picker>,
    /// What has already been encoded. Cheap to keep -- a sixteen-pixel
    /// square is a kilobyte of pixels -- and encoding is the only part of
    /// this that is not free.
    encoded: HashMap<Key, Protocol>,
    /// The colours everything in `encoded` was drawn with, so that a theme
    /// change throws it away rather than leaving marks inked for the theme
    /// before it.
    palette: Option<Palette>,
}

impl std::fmt::Debug for Images {
    /// Written out by hand because encoded pixels are not something to
    /// print: what a reader of a log wants is which protocol and how many.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Images")
            .field("protocol", &self.picker.as_ref().map(Picker::protocol_type))
            .field("encoded", &self.encoded.len())
            .finish()
    }
}

impl Default for Images {
    fn default() -> Self {
        Self::none()
    }
}

impl Images {
    /// No pictures. What the tests and every terminal without a graphics
    /// protocol get.
    #[must_use]
    pub fn none() -> Self {
        Self {
            picker: None,
            encoded: HashMap::new(),
            palette: None,
        }
    }

    /// Asks the terminal what it can do.
    ///
    /// Must be called before the alternate screen is entered, and never
    /// from inside a frame: it writes a query to stdout and waits for the
    /// terminal to answer on stdin.
    ///
    /// Half-blocks -- what `ratatui-image` guesses when nothing answers --
    /// count as "cannot": see the module's own note.
    #[must_use]
    pub fn detect() -> Self {
        match Picker::from_query_stdio() {
            Ok(picker) if picker.protocol_type() != ProtocolType::Halfblocks => {
                tracing::info!(protocol = ?picker.protocol_type(), "the terminal can show pictures");
                Self {
                    picker: Some(picker),
                    encoded: HashMap::new(),
                    palette: None,
                }
            }
            Ok(_) => Self::none(),
            Err(error) => {
                tracing::debug!(%error, "no picture protocol");
                Self::none()
            }
        }
    }

    /// Whether there is any point preparing a picture.
    #[must_use]
    pub fn available(&self) -> bool {
        self.picker.is_some()
    }

    /// Encodes one mark, unless it already has been.
    ///
    /// The palette is the view's, and a palette Obelus has not drawn with
    /// before empties the cache: pixels cannot be recoloured after the fact.
    pub fn prepare(&mut self, id: &str, svg: &str, focused: bool, palette: Palette) {
        let Some(picker) = &self.picker else { return };
        if self.palette != Some(palette) {
            self.encoded.clear();
            self.palette = Some(palette);
        }
        let key = (id.to_string(), focused);
        if self.encoded.contains_key(&key) {
            return;
        }
        let size = picker.font_size();
        let pixels = (
            u32::from(size.width) * u32::from(SLOT.width),
            u32::from(size.height) * u32::from(SLOT.height),
        );
        let paper = if focused {
            palette.selected
        } else {
            palette.paper
        };
        let Some(image) = raster(svg, pixels, palette.ink, paper) else {
            return;
        };
        match picker.new_protocol(image, SLOT, Resize::Fit(None)) {
            Ok(protocol) => {
                self.encoded.insert(key, protocol);
            }
            Err(error) => tracing::debug!(id, %error, "not encoding an icon"),
        }
    }

    /// Draws a prepared mark, and says whether there was one.
    ///
    /// `false` means the caller should draw its glyph: either the terminal
    /// cannot show pictures, or this agent's drawing has not arrived yet.
    pub fn draw(&self, cells: &mut CellBuffer, x: u16, y: u16, id: &str, focused: bool) -> bool {
        let Some(protocol) = self.encoded.get(&(id.to_string(), focused)) else {
            return false;
        };
        let area = Rect {
            x,
            y,
            width: SLOT.width,
            height: SLOT.height,
        };
        // Refused rather than clipped when what is on screen is narrower
        // than the mark. A picture is handed to the terminal as pixels at a
        // position, so half of one hanging off the edge is not half a
        // picture -- it is a mark drawn over whatever is out there. The
        // caller draws its glyph instead, which does clip.
        if cells.area().intersection(area) != area {
            return false;
        }
        Image::new(protocol).render(area, cells);
        true
    }
}

/// Draws an SVG into pixels of exactly this size.
///
/// The registry's marks are drawn in `currentColor`, meaning "whatever
/// colour the text around me is" -- a colour that exists in a browser and
/// not in a file. So it is substituted before parsing: the mark is inked in
/// `ink` and laid on `paper`, opaquely, because a sixel has no alpha and a
/// mark has to look the same in all three protocols.
#[must_use]
pub fn raster(
    svg: &str,
    pixels: (u32, u32),
    ink: Color,
    paper: Color,
) -> Option<image::DynamicImage> {
    let (width, height) = pixels;
    if width == 0 || height == 0 {
        return None;
    }
    let inked = svg.replace("currentColor", &hex(ink));
    let tree = resvg::usvg::Tree::from_str(&inked, &resvg::usvg::Options::default()).ok()?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let (red, green, blue) = rgb(paper);
    pixmap.fill(resvg::tiny_skia::Color::from_rgba8(red, green, blue, 255));

    // Fitted, not stretched: these are square today, but a registry that
    // one day carries a wide mark should show it the right shape.
    let drawn = tree.size();
    let scale = (width as f32 / drawn.width()).min(height as f32 / drawn.height());
    let transform = resvg::tiny_skia::Transform::from_translate(
        (width as f32 - drawn.width() * scale) / 2.0,
        (height as f32 - drawn.height() * scale) / 2.0,
    )
    .pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Opaque throughout, because the background was filled first -- so the
    // premultiplied pixels tiny-skia produces are already the plain ones
    // `image` expects.
    let buffer = image::RgbaImage::from_raw(width, height, pixmap.take())?;
    Some(image::DynamicImage::ImageRgba8(buffer))
}

/// A colour as `#rrggbb`, for substituting into an SVG.
fn hex(colour: Color) -> String {
    let (red, green, blue) = rgb(colour);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// The channels of a colour Obelus drew from a theme.
///
/// Themes are written in RGB -- Obelus has `COLORTERM=truecolor` and says
/// so -- so this is a total function over what actually reaches it. The
/// arms for the rest are there because `Color` is somebody else's enum:
/// black for the dark ones and white for the light ones is not a colour
/// scheme, it is a refusal to guess a palette Obelus does not have.
fn rgb(colour: Color) -> (u8, u8, u8) {
    match colour {
        Color::Rgb(red, green, blue) => (red, green, blue),
        Color::White | Color::Gray => (255, 255, 255),
        _ => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;
    use ratatui_image::picker::{Picker, ProtocolType};

    use super::{Images, Palette, SLOT, raster};

    /// The colours a card draws with, near enough.
    const PALETTE: Palette = Palette {
        ink: Color::Rgb(0x9a, 0xa5, 0xb1),
        paper: Color::Rgb(0x11, 0x14, 0x18),
        selected: Color::Rgb(0x28, 0x2f, 0x3a),
    };

    /// A terminal that can draw, of a given kind. What `detect` would have
    /// come back with, without a terminal to ask.
    fn terminal(protocol: ProtocolType) -> Images {
        // `halfblocks` is the only constructor that needs no terminal to
        // ask: it carries a cell size and the protocol Obelus treats as
        // "cannot", so the kind wanted is set over the top of it.
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(protocol);
        Images {
            picker: Some(picker),
            encoded: std::collections::HashMap::new(),
            palette: None,
        }
    }

    /// A mark of the shape the registry publishes: one path, in
    /// `currentColor`, sixteen pixels square.
    const MARK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="currentColor" d="M2 2h12v12H2z"/></svg>"#;

    #[test]
    fn a_mark_is_inked_in_the_colour_it_is_given() {
        let ink = Color::Rgb(0x40, 0x80, 0xc0);
        let paper = Color::Rgb(0x10, 0x10, 0x10);
        let image = raster(MARK, (16, 16), ink, paper).expect("pixels");
        let pixels = image.to_rgba8();
        assert_eq!(pixels.dimensions(), (16, 16));
        // The path covers the middle and not the corner, so the two say
        // which colour went where.
        assert_eq!(pixels.get_pixel(8, 8).0, [0x40, 0x80, 0xc0, 255]);
        assert_eq!(pixels.get_pixel(0, 0).0, [0x10, 0x10, 0x10, 255]);
    }

    #[test]
    fn a_drawing_that_is_not_an_svg_is_no_drawing() {
        assert!(
            raster(
                "<html>not a mark</html>",
                (16, 16),
                Color::White,
                Color::Black
            )
            .is_none()
        );
        assert!(raster(MARK, (0, 16), Color::White, Color::Black).is_none());
    }

    /// The whole path, per protocol: an SVG becomes pixels, the pixels
    /// become that terminal's escape sequence, and the sequence reaches the
    /// cell the card put it in.
    ///
    /// Worth asserting on the bytes because every way this can fail is
    /// silent: `ratatui-image` refuses to draw a picture larger than the
    /// area it is given, and a refusal looks exactly like a card with
    /// nothing on it.
    #[test]
    fn a_mark_becomes_the_terminals_own_escape_sequence() {
        // The two Obelus can be handed: kitty's graphics APC, and a
        // sixel's device control string -- which a sixel arrives behind a
        // clear-this-much of its own, hence `contains` rather than a
        // prefix.
        for (protocol, opening) in [
            (ProtocolType::Kitty, "\u{1b}_G"),
            (ProtocolType::Sixel, "\u{1b}P"),
        ] {
            let mut images = terminal(protocol);
            assert!(images.available());
            images.prepare("claude-acp", MARK, false, PALETTE);
            let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
            assert!(
                images.draw(&mut cells, 3, 0, "claude-acp", false),
                "{protocol:?} drew nothing"
            );
            let symbol = cells[(3, 0)].symbol().to_string();
            assert!(
                symbol.contains(opening),
                "{protocol:?} put {symbol:?} in the cell"
            );
            // Long, because it is a whole picture in one cell: sixteen by
            // sixteen pixels cannot be encoded in a handful of bytes.
            assert!(
                symbol.len() > 100,
                "{protocol:?} encoded {} bytes",
                symbol.len()
            );
        }
    }

    /// A mark is drawn onto the background it will sit on, so the focused
    /// card's and an ordinary card's are two different pictures.
    #[test]
    fn the_focused_card_gets_its_own_pixels() {
        let mut images = terminal(ProtocolType::Kitty);
        images.prepare("claude-acp", MARK, false, PALETTE);
        images.prepare("claude-acp", MARK, true, PALETTE);
        assert_eq!(images.encoded.len(), 2);
        let plain = images.encoded[&("claude-acp".to_string(), false)].size();
        assert_eq!(plain, SLOT);
        let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        assert!(images.draw(&mut cells, 0, 0, "claude-acp", true));
    }

    /// Pixels cannot be recoloured after the fact, so a theme change is a
    /// cache that has to go.
    #[test]
    fn a_new_palette_throws_the_old_pixels_away() {
        let mut images = terminal(ProtocolType::Kitty);
        images.prepare("claude-acp", MARK, false, PALETTE);
        images.prepare("gemini", MARK, false, PALETTE);
        assert_eq!(images.encoded.len(), 2);
        let light = Palette {
            ink: Color::Rgb(0x33, 0x33, 0x33),
            paper: Color::Rgb(0xff, 0xff, 0xff),
            selected: Color::Rgb(0xe0, 0xe0, 0xe0),
        };
        images.prepare("claude-acp", MARK, false, light);
        // The one just asked for, and not the one drawn for the theme
        // before it.
        assert_eq!(images.encoded.len(), 1);
        assert!(
            images
                .encoded
                .contains_key(&("claude-acp".to_string(), false))
        );
    }

    /// A terminal too narrow for the mark gets the glyph, which is what
    /// `false` from here means.
    #[test]
    fn a_mark_that_does_not_fit_is_not_drawn() {
        let mut images = terminal(ProtocolType::Kitty);
        images.prepare("claude-acp", MARK, false, PALETTE);
        let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        // One column short of the two the mark needs.
        assert!(!images.draw(&mut cells, 7, 0, "claude-acp", false));
        // And past the last row.
        assert!(!images.draw(&mut cells, 0, 2, "claude-acp", false));
        assert!(images.draw(&mut cells, 6, 1, "claude-acp", false));
    }

    #[test]
    fn without_a_protocol_nothing_is_prepared_or_drawn() {
        let mut images = Images::none();
        assert!(!images.available());
        images.prepare(
            "claude-acp",
            MARK,
            false,
            Palette {
                ink: Color::White,
                paper: Color::Black,
                selected: Color::Black,
            },
        );
        let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        assert!(!images.draw(&mut cells, 0, 0, "claude-acp", false));
    }
}
