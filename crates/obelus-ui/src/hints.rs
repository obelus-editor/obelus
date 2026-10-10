//! The keys at the foot of a view, the card they can be drawn on, a panel,
//! and what a list says when it has nothing in it.

use super::*;

/// One key, and what it does here.
///
/// The word is optional because some keys are their own explanation. The
/// arrows walk the tabs and there is nothing to add to an arrow; `alt+f`
/// means nothing at all until something says "fold".
#[derive(Clone, Copy, Debug)]
pub struct Hint {
    /// The key, spelled by the key table so that a reader who rebound it
    /// sees what they bound.
    pub chord: obelus_keymap::KeyChord,
    /// A second key that does the same thing, for a pair that shares a word:
    /// `alt+up` and `alt+down` are one act in two directions, and two rows
    /// saying "move it up" and "move it down" is the same sentence twice.
    pub and_also: Option<obelus_keymap::KeyChord>,
    /// What it does, in the one word the foot has room for.
    pub does: Option<&'static str>,
    /// The same thing said properly, for the card, which has room for it.
    ///
    /// `None` where the word is the whole of it. Two forms rather than one
    /// because the two places are not the same place: a foot is a row shared
    /// by everything, and a card is a page about one thing.
    pub said: Option<&'static str>,
    /// Whether it goes at the foot, or waits in the list of them all.
    ///
    /// The foot is one row over the reader's work, so what goes there is
    /// what they reach for without thinking. Everything else is a keypress
    /// away and is not lost -- it is in the card, where there is room to say
    /// what it does in words rather than in one.
    pub common: bool,
    /// Which way it is set, for a key whose whole job is a switch.
    ///
    /// The word says what the key switches; this says what it is switched
    /// to. Without it a switch on a key is a coin toss: the reader presses
    /// it to find out which way it was, which is the one thing a switch
    /// must never make them do.
    ///
    /// `None` for a key that does something rather than sets something.
    pub switched: Option<bool>,
    /// Whether it does anything *now*.
    ///
    /// Worked out per frame by whoever knows: a note about the project has
    /// nowhere to go, so there is no "go there" on the foot while the
    /// selection is on one. The foot draws what can be pressed, and what it
    /// was told to keep ([`Hint::kept`]); the card draws all of them and
    /// greys this one out, so what a reader learns is that the view has
    /// eight keys rather than that its keys come and go.
    pub usable: bool,
    /// Whether the foot keeps it, greyed, where it does nothing.
    ///
    /// For the one key a view has: dropped from a foot with nothing else
    /// on it, it takes the whole foot with it, and the view grows and
    /// shrinks by two rows as the reader walks past the rows it works on.
    pub kept: bool,
}

impl Hint {
    /// A key that goes at the foot.
    #[must_use]
    pub const fn common(chord: obelus_keymap::KeyChord, does: &'static str) -> Self {
        Self {
            chord,
            and_also: None,
            does: Some(does),
            said: None,
            switched: None,
            common: true,
            usable: true,
            kept: false,
        }
    }

    /// One that waits in the card.
    #[must_use]
    pub const fn rare(chord: obelus_keymap::KeyChord, does: &'static str) -> Self {
        Self {
            common: false,
            ..Self::common(chord, does)
        }
    }

    /// What it does, at length, for the card.
    #[must_use]
    pub const fn saying(mut self, said: &'static str) -> Self {
        self.said = Some(said);
        self
    }

    /// The same act in the other direction, on a key of its own.
    #[must_use]
    pub const fn or(mut self, chord: obelus_keymap::KeyChord) -> Self {
        self.and_also = Some(chord);
        self
    }

    /// Says whether it does anything at the moment.
    #[must_use]
    pub const fn when(mut self, usable: bool) -> Self {
        self.usable = usable;
        self
    }

    /// Says the foot keeps it, greyed, while it does nothing.
    #[must_use]
    pub const fn kept(mut self) -> Self {
        self.kept = true;
        self
    }

    /// Says it is a switch, and which way it is set.
    #[must_use]
    pub const fn set(mut self, on: bool) -> Self {
        self.switched = Some(on);
        self
    }

    /// How it is written: the key, or the pair of them.
    #[must_use]
    pub fn keys(self) -> String {
        match self.and_also {
            Some(also) => format!("{} {}", self.chord.label(), also.label()),
            None => self.chord.label(),
        }
    }
}

/// How wide a tick is, in cells.
///
/// Two: a Nerd Font draws its glyphs over two columns while the terminal
/// allocates one, so the blank after it belongs to it.
pub const TICK_WIDTH: u16 = 2;

/// What a tick looks like, set and not.
///
/// The plain ones where there is no Nerd Font. The same box in both, with
/// a mark in the second -- a pair that changed shape would put a jog in a
/// column read straight down.
///
/// This was a slider: a four-cell track with a knob at one end of it. What
/// a slider says is *which way it is*, by a position the eye has to measure
/// against a track two cells longer than the knob -- and Obelus draws it in
/// a row of text, at a size where that measurement is a guess. A box is
/// either marked or it is not, which is the same question answered in a
/// glyph.
#[must_use]
pub fn tick(on: bool) -> char {
    match (obelus_icons::enabled(), on) {
        (true, false) => obelus_icons::ui::TODO,
        (true, true) => obelus_icons::ui::TODO_DONE,
        (false, false) => '\u{25a1}',
        (false, true) => '\u{2611}',
    }
}

/// Draws one, and says where whatever follows it goes.
pub fn ticked(cells: &mut CellBuffer, x: u16, y: u16, on: bool, style: Style) -> u16 {
    put(cells, x, y, tick(on), style);
    // And what that cell *is*, for a front end that can draw the shape
    // rather than the glyph standing in for it. The cell above is the
    // whole of the switch in a terminal and nothing depends on this being
    // heard -- see `shapes`.
    shapes::ticked(
        Rect {
            x,
            y,
            width: 1,
            height: 1,
        },
        on,
    );
    x + TICK_WIDTH
}

/// Draws a key in a cap of its own, and says where the next thing goes.
///
/// A blank inside the cap either side, and the cap on a ground a shade off
/// the page. What binds a key to the word beside it is the block it sits
/// in: the row used to separate a key from its word by one blank and one
/// item from the next by three, which are near enough the same gap that the
/// eye could not tell which side of it a word belonged to.
pub(crate) fn capped(cells: &mut CellBuffer, x: u16, y: u16, keys: &str, theme: &Theme) -> u16 {
    capped_in(cells, x, y, keys, theme.gutter_current, theme)
}

/// The same cap, with the key in this ink: the dim one, for a key a foot
/// keeps where it does nothing.
fn capped_in(cells: &mut CellBuffer, x: u16, y: u16, keys: &str, ink: Color, theme: &Theme) -> u16 {
    let after = write(
        cells,
        x,
        y,
        &format!(" {keys} "),
        Style::new().fg(ink).bg(theme.raised_background),
    );
    // And what those cells *are*, for a front end that can draw the shape
    // rather than only its ground -- see `shapes`. The cells above are the
    // whole of the cap in a terminal and the ground of it in a window;
    // neither depends on this being heard.
    shapes::capped(
        keys,
        Rect {
            x,
            y,
            width: after.saturating_sub(x),
            height: 1,
        },
        theme.raised_background,
        theme.background,
        theme.gutter,
    );
    after
}

/// Says that a key already written here sits in a cap, without touching a
/// cell of it.
///
/// The foot writes its own cap, because a run of cells a shade off the page
/// is the only cap a terminal has and the foot is a row of keys among
/// words. Everywhere else a key is shown it has no ground to give it and
/// needs none: on the card and on the keys page the key is *a column*, and
/// in a conversation it is a word set apart from the words beside it by
/// the gaps either side. So the cells stay exactly as they are, and the
/// shape is said around them -- over the blank either side, which is where
/// a cap's own blanks would have been.
///
/// Which is the whole channel's rule in the one case it is easiest to get
/// wrong: what is said here may not be the only thing saying it.
/// `keys` is how many cells the key itself takes, which the caller
/// measures: a screen that counts a Nerd Font glyph as two cells and one
/// that counts it as one are both here, and a cap measured by the wrong
/// one is a cap that does not fit the key it is round.
pub(crate) fn cap_around(
    x: u16,
    y: u16,
    keys: &str,
    wide: usize,
    cap: Color,
    page: Color,
    edge: Color,
) {
    let Ok(width) = u16::try_from(wide + 2) else {
        return;
    };
    if wide == 0 {
        return;
    }
    shapes::capped(
        keys,
        Rect {
            x: x.saturating_sub(1),
            y,
            width,
            height: 1,
        },
        cap,
        page,
        edge,
    );
}

/// How far apart two items on that row sit.
///
/// Three, against the one blank inside the cap and the one before a word:
/// the gap between items has to beat the gaps inside one, or the row is a
/// line of tokens with nothing saying which belongs to which.
const BETWEEN: u16 = 3;

/// How wide a cap is, with the key in it.
///
/// Asked by the item that is about to be drawn and by the pointer at the
/// far end, which were each carrying their own idea of how much a cap adds.
pub(crate) fn cap_width(keys: &str) -> usize {
    text_width(keys) + 2
}

/// How much of the row one hint takes, the gap after it aside.
///
/// The cap, the word and the blank before it, and the box with its own. One
/// answer, because the drawing asks it twice: once to find out whether the
/// item fits, and once by advancing exactly that far.
fn width_of(hint: &Hint) -> usize {
    cap_width(&hint.keys())
        + hint.does.map_or(0, |does| text_width(does) + 1)
        + hint.switched.map_or(0, |_| usize::from(TICK_WIDTH) + 1)
}

/// Marks a row that stopped before it had said everything.
///
/// The mark Obelus cuts text with everywhere else, in the dim ink, in the
/// blank the next item would have started in. Where the pointer to the card
/// is up this says which of the two is the whole list; where there is no
/// card -- a document's foot has none behind it -- it says on its own that
/// the terminal is too narrow for all of this.
fn cut(cells: &mut CellBuffer, x: u16, y: u16, edge: u16, theme: &Theme) {
    if x >= edge {
        return;
    }
    put(
        cells,
        x,
        y,
        '\u{2026}',
        Style::new().fg(theme.gutter).bg(theme.background),
    );
}

/// How much of a panel's edge is not for what is inside it.
///
/// Two each side: the line, and a blank inside it. Text against a border
/// reads as text that ran into it -- and a hover holds a README, whose own
/// fenced blocks are boxes, so without the blank there were two lines
/// touching with nothing between them.
pub const PANEL_INSET: u16 = 2;

/// The frame round something Obelus floats over the reader's work.
///
/// One shape for all of them: the completion list, the signature line, a
/// hover, and the card of every key. They are the same kind of thing --
/// something put over the page for a moment -- and four of them wearing two
/// shapes was a screen where the shape said nothing.
///
/// Rounded, and on the page's own colour. The rounding is not decoration:
/// what a hover holds is a *document*, and a document's own boxes -- a
/// markdown table, a fenced block -- are square, because that is what
/// every markdown renderer draws. A square frame around a square frame is
/// one thing that looks like two; a round one says which of them is
/// Obelus's furniture and which is the reader's text.
///
/// The frame is what says it, and says it alone. It was on a ground a
/// shade off the page as well, which in a window is the colour of its
/// glass -- a grey box over glass that is the page's colour everywhere
/// else, the lists and the settings under it included.
pub fn panel(cells: &mut CellBuffer, area: Rect, theme: &Theme) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let ground = theme.background;
    // What it is put over, said before it is: a window draws the panel as
    // glass inside its frame, and glass is what is behind it seen through
    // -- see `shapes::Joined::Nowhere`.
    shapes::behind(area, shapes::Joined::Nowhere, ground, cells);
    fill(cells, area, Style::new().fg(theme.foreground).bg(ground));
    let edge = Style::new().fg(theme.gutter).bg(ground);
    let (left, right) = (area.x, area.right() - 1);
    let (top, bottom) = (area.y, area.bottom() - 1);
    for (x, y, glyph) in [
        (left, top, '\u{256d}'),
        (right, top, '\u{256e}'),
        (left, bottom, '\u{2570}'),
        (right, bottom, '\u{256f}'),
    ] {
        put(cells, x, y, glyph, edge);
    }
    for x in left + 1..right {
        put(cells, x, top, '\u{2500}', edge);
        put(cells, x, bottom, '\u{2500}', edge);
    }
    for y in top + 1..bottom {
        put(cells, left, y, '\u{2502}', edge);
        put(cells, right, y, '\u{2502}', edge);
    }
}

/// What is left of a panel for the thing inside it.
///
/// One answer, asked by the drawing of the frame and by whatever is laid
/// out to fit in it -- which in a hover's case happens a frame earlier,
/// because markdown cannot be made into rows until there is a width to
/// make them for.
#[must_use]
pub fn inside(area: Rect) -> Rect {
    Rect {
        x: area.x + PANEL_INSET,
        y: area.y + 1,
        width: area.width.saturating_sub(PANEL_INSET * 2),
        height: area.height.saturating_sub(2),
    }
}

/// How many rows a view gives up to its foot, where it has one.
pub const FOOT_ROWS: u16 = 2;

/// What is left of a region once its foot is taken off the bottom.
///
/// One answer, asked by the drawing and by whatever moves about inside: a
/// page is worth what is on screen, and two answers to how much that is
/// would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn footed(area: Rect, hints: &[Hint]) -> Rect {
    if hints.is_empty() || area.height <= FOOT_ROWS {
        return area;
    }
    Rect {
        height: area.height - FOOT_ROWS,
        ..area
    }
}

/// The keys a view answers to, along the bottom of it under a rule.
///
/// The common ones that can be pressed at the moment, those a view keeps
/// there greyed where they cannot ([`Hint::kept`]), and
/// [`obelus_keymap::keys_card`] at the right-hand end saying there
/// are more. At the foot rather than beside a
/// title, because a key needs a word and words need room.
///
/// What belongs here is what *this* view does. A key that means the same
/// thing wherever the reader is does not: escape backs out of the nearest
/// thing everywhere in Obelus -- `keymap::why_not` refuses to rebind it for
/// that reason -- so a foot that spends a third of itself saying `Leave` is
/// a row of the reader's screen saying what every other view already said.
/// Those go on the card, which is every key here rather than the ones worth
/// telling.
pub fn foot(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    row_of_keys(cells, area, hints, theme, true);
}

/// The same row, for a view with no card behind it.
///
/// A card of every key is a layer's: something opened over the reader's
/// work, which owns the keyboard while it is up and has to be able to say
/// so. A document is where the reader already was, the card's key over one
/// is whatever that key means everywhere, and this row is the whole of what
/// the view says about itself -- so it points at nothing, and gets the width
/// the pointer would have taken.
pub fn foot_without_a_card(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    row_of_keys(cells, area, hints, theme, false);
}

/// Draws that row, with or without the pointer at the end of it.
fn row_of_keys(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme, card: bool) {
    if hints.is_empty() || area.height < FOOT_ROWS {
        return;
    }
    let top = area.y + area.height - FOOT_ROWS;
    rule(
        cells,
        Rect {
            y: top,
            height: 1,
            ..area
        },
        theme,
    );
    let y = top + 1;
    fill(
        cells,
        Rect {
            y,
            height: 1,
            ..area
        },
        Style::new().bg(theme.background),
    );

    // The one at the end first, because it is the one that must not be given
    // up: a foot that ran out of room and dropped the way to the rest of the
    // keys would be a foot that hides the thing it exists to point at.
    let chord = obelus_keymap::keys_card().label();
    let width = u16::try_from(cap_width(&chord) + 1 + text_width("Keys")).unwrap_or(0);
    let edge = match area.width.checked_sub(width + 2).filter(|_| card) {
        Some(offset) => {
            let after = capped(cells, area.x + offset, y, &chord, theme);
            write(
                cells,
                after + 1,
                y,
                "Keys",
                Style::new().fg(theme.gutter).bg(theme.background),
            );
            area.x + offset
        }
        None => area.x + area.width,
    };

    let mut x = area.x + 2;
    for hint in hints
        .iter()
        .filter(|hint| hint.common && (hint.usable || hint.kept))
    {
        let keys = hint.keys();
        // Saturating rather than refused: a hint wider than the screen can
        // hold is one that does not fit, which is the same answer the row
        // gives anything else that does not.
        let wanted = u16::try_from(width_of(hint)).unwrap_or(u16::MAX);
        if x.saturating_add(wanted).saturating_add(BETWEEN) > edge {
            // The row stops here, and says so. A foot that ran out of room
            // used to drop the rest of its keys and look exactly like a
            // foot that had said everything it had -- so a reader on a
            // narrow terminal was told a view answered to two keys when it
            // answered to eight, and nothing on the screen disagreed.
            cut(cells, x, y, edge, theme);
            return;
        }
        // The key in a cap and the word out of it: what a reader is looking
        // for down here is which key, and the word is read once to find out
        // that it is the one.
        x = match hint.usable {
            true => capped(cells, x, y, &keys, theme),
            false => capped_in(cells, x, y, &keys, theme.gutter, theme),
        };
        if let Some(does) = hint.does {
            x = write(
                cells,
                x + 1,
                y,
                does,
                Style::new().fg(theme.gutter).bg(theme.background),
            );
        }
        if let Some(on) = hint.switched {
            // Bright when it is set and dim when it is not, under a glyph
            // that already says which: a box that is empty and loud is the
            // brightest thing on a row about keys, and it is the one thing
            // here that is off.
            let ink = match on {
                true => theme.foreground,
                false => theme.gutter,
            };
            x = ticked(
                cells,
                x + 1,
                y,
                on,
                Style::new().fg(ink).bg(theme.background),
            );
        }
        x += BETWEEN;
    }
}

/// Every key a view answers to, on a card over it.
///
/// All of them, with what cannot be pressed at the moment greyed rather than
/// left out: what a reader should come away with is that this view has these
/// keys, not that its keys come and go. There is room here for a sentence,
/// which is why the words can be words rather than the one the foot fits.
pub fn keys_card(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    if hints.is_empty() {
        return;
    }
    let column = u16::try_from(
        hints
            .iter()
            .map(|hint| text_width(&hint.keys()))
            .max()
            .unwrap_or(0),
    )
    .unwrap_or(0)
    .saturating_add(2);
    let widest = u16::try_from(
        hints
            .iter()
            .map(|hint| hint.said.or(hint.does).map_or(0, text_width))
            .max()
            .unwrap_or(0),
    )
    .unwrap_or(0);
    // The edges, a margin inside them, the keys and what they do.
    let width = column
        .saturating_add(widest)
        .saturating_add(4)
        .min(area.width);
    // The edges, the title, a blank under it, and a row per key.
    let height = u16::try_from(hints.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4)
        .min(area.height);
    if width < 4 || height < 4 {
        return;
    }
    let card = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };

    // The panel's own ground -- see `panel`.
    let paper = theme.background;
    panel(cells, card, theme);
    let room = inside(card);

    write(
        cells,
        room.x,
        room.y,
        "The keys here",
        Style::new().fg(theme.status_foreground).bg(paper),
    );
    let ground = Style::new().fg(theme.foreground).bg(paper);
    let off = Style::new().fg(theme.gutter).bg(paper);
    for (at, hint) in hints.iter().enumerate() {
        let Ok(offset) = u16::try_from(at) else { break };
        let y = room.y + 2 + offset;
        if y >= room.bottom() {
            break;
        }
        let style = match hint.usable {
            true => ground,
            false => off,
        };
        let keys = hint.keys();
        write(cells, room.x, y, &keys, style);
        // The panel's own ground on both counts: what draws the cap here is
        // its outline and the lip under it, the same as a key on a page
        // that is already the colour the key is.
        cap_around(
            room.x,
            y,
            &keys,
            text_width(&keys),
            paper,
            paper,
            theme.gutter,
        );
        let x = room.x + column;
        if let Some(does) = hint.said.or(hint.does) {
            write(cells, x, y, does, style);
        }
        // What the key is set to is not on the card. A card is read to find
        // out what a key *is* -- what to press for a thing -- and which way
        // one of them happens to be switched right now is a different
        // question, asked of the foot, where the key is offered rather than
        // catalogued.
    }
}

/// What a list says when it has nothing in it.
///
/// One place, so that every empty list in Obelus says its own reason in the
/// same voice and the same colour. What the reason *is* belongs to whoever
/// knows it -- the application for a list of files, the component for a
/// filtered one.
pub fn nothing(cells: &mut CellBuffer, area: Rect, reason: &str, theme: &Theme) {
    write(
        cells,
        area.x + 1,
        area.y,
        reason,
        Style::new().fg(theme.gutter).bg(theme.background),
    );
}

/// What a list says when it has nothing in it *yet*: the same line, with
/// the mark that turns in front of it.
///
/// On the line rather than on the row under the list, because the line is
/// where the reader is looking and already says what is being waited for:
/// a mark down there as well would be the waiting said twice. A list that
/// has rows and is still being filled has no such line, and keeps the mark
/// on the row under it -- see `status::still_working`.
pub fn nothing_yet(cells: &mut CellBuffer, area: Rect, reason: &str, phase: u32, theme: &Theme) {
    write(
        cells,
        area.x + 1,
        area.y,
        &spinning(phase).to_string(),
        Style::new().fg(theme.gutter).bg(theme.background),
    );
    shapes::spun(area.x + 1, area.y);
    // The words are `nothing`'s, two columns along: the mark and the one
    // blank after it, as after every glyph.
    let after = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(2),
        ..area
    };
    nothing(cells, after, reason, theme);
}
