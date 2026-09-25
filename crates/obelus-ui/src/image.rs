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

/// Where a window's marks go, for the front end that draws its own pixels.
///
/// A terminal is handed a picture as bytes in the middle of a frame, which
/// is why everything above works the way it does. A window has a texture
/// and a place to put quads, and none of that belongs in a view: so the
/// view says the same two things it says to a terminal -- here is a mark,
/// draw it there -- and what is on the other end turns them into pixels.
///
/// The `svg` crosses once per mark and palette, and the placement once per
/// frame it is on screen.
pub trait Marks: Send + Sync {
    /// This mark exists, and is drawn from this text in these colours.
    fn carries(&self, id: &str, svg: &str, focused: bool, palette: Palette);
    /// And it goes here, in the frame being laid out.
    fn draws(&self, id: &str, focused: bool, x: u16, y: u16);
}

/// The marks, encoded for whatever the terminal turned out to be.
pub struct Images {
    /// How to encode, and how big a cell is. `None` when the terminal
    /// cannot show a picture, which is most of the time.
    picker: Option<Picker>,
    /// Where a window's marks go instead, when a window is what is
    /// drawing. Exclusive with `picker`: the three terminal protocols and
    /// a texture are two answers to one question.
    marks: Option<std::sync::Arc<dyn Marks>>,
    /// Which marks the window has already been handed, so that a screenful
    /// of cards does not send the same drawing every frame.
    told: std::collections::HashSet<Key>,
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
            .field("a window's", &self.marks.is_some())
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
            marks: None,
            told: std::collections::HashSet::new(),
            encoded: HashMap::new(),
            palette: None,
        }
    }

    /// The marks, drawn by a window rather than by a terminal.
    ///
    /// Which a window can always do: the three protocols above are ways of
    /// asking a terminal to draw pixels it owns, and a window owns its
    /// own. So there is no detection here and no fallback to a glyph --
    /// the picture is simply drawn.
    #[must_use]
    pub fn drawn_by(marks: std::sync::Arc<dyn Marks>) -> Self {
        Self {
            picker: None,
            marks: Some(marks),
            told: std::collections::HashSet::new(),
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
                    marks: None,
                    told: std::collections::HashSet::new(),
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
    ///
    /// Either end counts. It used to be the terminal's protocol alone,
    /// which is what a window answering `false` here cost: this is also
    /// the gate on *fetching* the drawings, so the marks were never
    /// downloaded, never prepared, and the cards in a window wore the
    /// glyph a terminal without a protocol gets -- while the window was
    /// sitting there able to draw any of them.
    #[must_use]
    pub fn available(&self) -> bool {
        self.picker.is_some() || self.marks.is_some()
    }

    /// Whether a frame writing these has to put the caret out first.
    ///
    /// Sixel and iTerm2 draw at the cursor as their escape sequence
    /// arrives, so a caret the reader can see is a caret they watch jump
    /// into the picture and back. Kitty's virtual placements save and
    /// restore the cursor themselves and carry the pixels once, so hiding
    /// it around them buys nothing and costs a blink.
    #[must_use]
    pub fn requires_hidden_cursor(&self) -> bool {
        self.picker.as_ref().is_some_and(|picker| {
            matches!(
                picker.protocol_type(),
                ProtocolType::Sixel | ProtocolType::Iterm2
            )
        })
    }

    /// Encodes one mark, unless it already has been.
    ///
    /// The palette is the view's, and a palette Obelus has not drawn with
    /// before empties the cache: pixels cannot be recoloured after the fact.
    pub fn prepare(&mut self, id: &str, svg: &str, focused: bool, palette: Palette) {
        if self.palette != Some(palette) {
            self.encoded.clear();
            self.told.clear();
            self.palette = Some(palette);
        }
        if let Some(marks) = self.marks.clone() {
            // Once per mark and palette. The drawing itself is a few
            // kilobytes of text, and what is on screen is a dozen cards
            // redrawn on every keystroke.
            let key = (id.to_string(), focused);
            if self.told.insert(key) {
                marks.carries(id, svg, focused, palette);
            }
            return;
        }
        let Some(picker) = &self.picker else { return };
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
        let area = Rect {
            x,
            y,
            width: SLOT.width,
            height: SLOT.height,
        };
        if let Some(marks) = &self.marks {
            // The same rule as below, for the same reason: a picture is
            // drawn at a place rather than into cells, so one hanging off
            // the edge is a mark over whatever is out there. The glyph the
            // caller draws instead is the one that clips.
            if !self.told.contains(&(id.to_string(), focused))
                || cells.area().intersection(area) != area
            {
                return false;
            }
            marks.draws(id, focused, x, y);
            return true;
        }
        let Some(protocol) = self.encoded.get(&(id.to_string(), focused)) else {
            return false;
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
    use ratatui::style::{Color, Style};
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
            marks: None,
            told: std::collections::HashSet::new(),
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

    #[test]
    fn kitty_does_not_need_the_cursor_hidden() {
        assert!(
            !terminal(ProtocolType::Kitty).requires_hidden_cursor(),
            "kitty restores the cursor around virtual image placement"
        );
        for protocol in [ProtocolType::Sixel, ProtocolType::Iterm2] {
            assert!(
                terminal(protocol).requires_hidden_cursor(),
                "{protocol:?} draws at the cursor"
            );
        }
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
        // Sixel, because its pixels are the cell's own symbol: two
        // drawings that differ are two symbols that differ, and counting
        // the cache's keys would say two whatever was encoded into them.
        let mut images = terminal(ProtocolType::Sixel);
        images.prepare("claude-acp", MARK, false, PALETTE);
        images.prepare("claude-acp", MARK, true, PALETTE);
        assert_eq!(images.encoded.len(), 2);
        let plain = images.encoded[&("claude-acp".to_string(), false)].size();
        assert_eq!(plain, SLOT);

        let area = ratatui::layout::Rect::new(0, 0, 8, 2);
        let mut ordinary = ratatui::buffer::Buffer::empty(area);
        assert!(images.draw(&mut ordinary, 0, 0, "claude-acp", false));
        let mut focused = ratatui::buffer::Buffer::empty(area);
        assert!(images.draw(&mut focused, 0, 0, "claude-acp", true));
        assert_ne!(
            ordinary[(0, 0)].symbol(),
            focused[(0, 0)].symbol(),
            "the focused card was handed an ordinary card's pixels"
        );
    }

    /// A mark leaves the background of the cells it lands in alone.
    ///
    /// The card has already filled its rows with its own colour, and the
    /// mark was drawn onto that same colour -- so writing a background
    /// here would put two cells of the page's colour inside the focused
    /// card's band, a hole in the one mark that says where the reader is.
    #[test]
    fn a_mark_leaves_its_cells_background_alone() {
        let mut images = terminal(ProtocolType::Kitty);
        images.prepare("claude-acp", MARK, true, PALETTE);
        let area = ratatui::layout::Rect::new(0, 0, 8, 2);
        let mut cells = ratatui::buffer::Buffer::empty(area);
        // What the card fills its row with before the mark goes in.
        for column in 0..2 {
            cells[(column, 0)].set_style(Style::new().bg(PALETTE.selected));
        }
        assert!(images.draw(&mut cells, 0, 0, "claude-acp", true));
        for column in 0..2 {
            assert_eq!(
                cells[(column, 0)].style().bg,
                Some(PALETTE.selected),
                "the mark repainted column {column} of the focused card"
            );
        }
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

    /// What a window's end of this records, for a test to look at.
    #[derive(Debug, Default)]
    struct Window {
        carried: std::sync::Mutex<Vec<(String, bool)>>,
        drawn: std::sync::Mutex<Vec<(String, u16, u16)>>,
    }

    impl super::Marks for Window {
        fn carries(&self, id: &str, _svg: &str, focused: bool, _palette: Palette) {
            self.carried
                .lock()
                .expect("what was carried")
                .push((id.to_string(), focused));
        }

        fn draws(&self, id: &str, _focused: bool, x: u16, y: u16) {
            self.drawn
                .lock()
                .expect("what was drawn")
                .push((id.to_string(), x, y));
        }
    }

    /// A window can show a picture, which is what decides whether the
    /// drawings are fetched at all.
    ///
    /// Deliberate break: answering from the terminal's protocol alone --
    /// which is what this said before there was a window -- stops the
    /// marks being downloaded, and every card wears its glyph.
    #[test]
    fn a_window_can_show_a_picture() {
        let images = Images::drawn_by(std::sync::Arc::new(Window::default()));
        assert!(images.available());
        assert!(!Images::none().available());
    }

    /// A window is handed a drawing once, and told where to put it on every
    /// frame.
    ///
    /// The two halves are different questions and the same call answers
    /// them: what a mark is made of crosses once, because it is kilobytes
    /// of text and a screenful of cards is redrawn on every keystroke;
    /// where it goes crosses every frame, because that is what a frame is.
    ///
    /// Deliberate break: taking the `told` check out of `prepare` carries
    /// the drawing on every frame, which this counts.
    #[test]
    fn a_window_is_told_what_a_mark_is_once_and_where_it_goes_always() {
        let window = std::sync::Arc::new(Window::default());
        let mut images = Images::drawn_by(window.clone());
        let palette = Palette {
            ink: Color::Rgb(1, 2, 3),
            paper: Color::Rgb(4, 5, 6),
            selected: Color::Rgb(7, 8, 9),
        };
        let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 20, 4));
        for _ in 0..3 {
            images.prepare("claude", MARK, false, palette);
            assert!(images.draw(&mut cells, 2, 1, "claude", false));
        }
        assert_eq!(
            *window.carried.lock().expect("what was carried"),
            vec![("claude".to_string(), false)]
        );
        assert_eq!(window.drawn.lock().expect("what was drawn").len(), 3);
    }

    /// A theme change is every mark drawn again, because pixels cannot be
    /// recoloured after the fact.
    ///
    /// Deliberate break: leaving `told` alone when the palette changes
    /// leaves the window drawing the old theme's marks for the rest of the
    /// session.
    #[test]
    fn a_new_palette_carries_the_marks_again() {
        let window = std::sync::Arc::new(Window::default());
        let mut images = Images::drawn_by(window.clone());
        let mut palette = Palette {
            ink: Color::Rgb(1, 2, 3),
            paper: Color::Rgb(4, 5, 6),
            selected: Color::Rgb(7, 8, 9),
        };
        images.prepare("claude", MARK, false, palette);
        palette.ink = Color::Rgb(9, 9, 9);
        images.prepare("claude", MARK, false, palette);
        assert_eq!(window.carried.lock().expect("what was carried").len(), 2);
    }

    /// A mark that does not fit is not drawn at all, and says so.
    ///
    /// A picture goes at a place rather than into cells, so half of one
    /// hanging off the edge is a mark over whatever is out there. Saying
    /// `false` is what makes the caller draw its glyph, which clips.
    ///
    /// Deliberate break: telling the window to draw it anyway returns
    /// `true` here, and the caller stops drawing the glyph that fits.
    #[test]
    fn a_mark_that_does_not_fit_is_left_to_the_glyph() {
        let window = std::sync::Arc::new(Window::default());
        let mut images = Images::drawn_by(window.clone());
        let palette = Palette {
            ink: Color::Rgb(1, 2, 3),
            paper: Color::Rgb(4, 5, 6),
            selected: Color::Rgb(7, 8, 9),
        };
        images.prepare("claude", MARK, false, palette);
        let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 3, 1));
        // Two cells wide, starting one from the right edge.
        assert!(!images.draw(&mut cells, 2, 0, "claude", false));
        assert!(window.drawn.lock().expect("what was drawn").is_empty());
    }
}
