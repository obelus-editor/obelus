//! The prompt's list.
//!
//! Both layouts occupy whole rows, which is why nothing here has to deal with
//! a double-width glyph cut in half by a vertical edge: a partial-width
//! overlay would leave one cell of a pair showing the code underneath and the
//! other showing the list.

use obelus_component::picker::{Marking, Picker, PickerItem, PickerLayout};
use obelus_git::FileStatus;
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    Hint, Marked, Matched, drop_from_left, drop_from_right, editor::SCROLLBAR_WIDTH, fill,
};

/// How many rows of the list a reader gets to walk.
///
/// Fixed rather than sized to the candidates: a boundary that moved as the
/// query narrowed the list would slide the preview up and down under a reader
/// who is looking at it.
///
/// Rows of the *list*, not of the region it is drawn in: a list with tabs
/// spends its first two rows on them, and a reader who asked for ten rows
/// meant ten rows to walk.
const LIST_ROWS: u16 = 10;

/// How wide the column in front of the icon is, where a row that has one
/// puts its mark.
///
/// The glyph and a blank after it, which is what the icon beside it gets.
/// Every mark Obelus draws is one cell; a wider one would push its own row
/// along rather than everybody's.
const MARKER_COLUMNS: u16 = 2;

/// How tall the whole list is: its rows, and the tabs over them.
fn list_region_rows(picker: &Picker, width: u16) -> u16 {
    LIST_ROWS
        .saturating_add(picker.tab_rows())
        .saturating_add(picker.about_rows(width))
}

/// What the keys do in a file list, and which of them do anything now.
///
/// One list read two ways, the way every other footed view here reads its
/// own: the foot draws the common ones that can be pressed, and the card
/// draws all of them with the rest greyed.
///
/// Nothing about enter or escape: a list is a list, and what a reader does
/// to one is not news. Nothing about the keys that walk the tabs either --
/// the tab row draws those itself, right where the tabs are, which says it
/// better than a word at the foot could.
///
/// Every one of these is a switch, and every one draws which way it is set.
/// A switch a reader has to press to find out what it was is the one thing a
/// switch must never ask of them -- and it is drawn with the settings page's
/// own control, so the two places say it the same way.
#[must_use]
pub fn hints(picker: &Picker) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    if !picker.says_keys() {
        return Vec::new();
    }
    // The two enters, where a row both holds something and is somewhere to
    // go. Said at the foot although a list's enter is normally not news,
    // because here it is: this is the one list where enter does not take
    // the row. Neither is a switch -- what they do depends on the row the
    // reader is on -- so neither draws a setting.
    if picker.rows_open() {
        let enter = |modifiers| obelus_editing::keymap::KeyChord::new(KeyCode::Enter, modifiers);
        return vec![
            Hint::common(enter(KeyModifiers::NONE), "Open")
                .saying("Show what this row reaches, or hide it again"),
            Hint::common(enter(KeyModifiers::ALT), "Go there")
                .saying("Leave the list and read the line this row names"),
        ];
    }
    let alt =
        |letter| obelus_editing::keymap::KeyChord::new(KeyCode::Char(letter), KeyModifiers::ALT);
    if picker.is_searching() {
        let how = picker.looks_how();
        let asked = how.unwrap_or_default();
        // Greyed rather than dropped where the question does not arise, so
        // the foot does not change height as the reader walks the tabs.
        let here = how.is_some();
        return vec![
            Hint::common(alt('r'), "Regex")
                .saying("Read the query as a pattern rather than as the text")
                .set(asked.regex)
                .when(here),
            Hint::common(alt('w'), "Word")
                .saying("Only where it stands as a word of its own")
                .set(asked.word)
                .when(here),
            Hint::common(alt('c'), "Case")
                .saying("The capitals as typed, rather than as the query implies")
                .set(asked.sensitive)
                .when(here),
            // The other way round from the three above: this one is the
            // symbols tab's, and they are the two that read a text.
            Hint::common(alt('o'), "Outside")
                .saying("Names from outside the project, where a server knows any")
                .set(picker.reaches_outside().unwrap_or(false))
                .when(picker.reaches_outside().is_some()),
        ];
    }
    let offering = picker.offers_ignored();
    vec![
        Hint::common(alt('i'), "Ignored files")
            .saying("Offer the files the project ignores, or leave them out")
            .set(offering.unwrap_or(false))
            .when(offering.is_some()),
        // Not a switch: what it does depends on the row the reader is on,
        // which is why it draws no setting.
        Hint::common(alt('n'), "Rename")
            .saying("Put this file or directory somewhere else, or call it something else")
            .when(offering.is_some()),
    ]
}

/// The foot saying what this list's own keys do, and the card over it.
///
/// Over the whole room rather than over the list, because a foot is the
/// bottom of the *view*: a full-area file list has a preview under it, and a
/// foot tucked below the rows would sit across the middle of the screen.
pub fn foot_of(cells: &mut CellBuffer, picker: &Picker, room: Rect, theme: &Theme) {
    let hints = hints(picker);
    if hints.is_empty() {
        return;
    }
    crate::foot(cells, room, &hints, theme);
    if picker.showing_keys() {
        // Above the foot: the foot says how to close this, and a card over
        // it would be a card with no way out on screen.
        crate::keys_card(cells, room_for(picker, room), &hints, theme);
    }
}

/// The room a list and its preview have, once a foot has taken its rows.
///
/// One answer, asked by the list, by the preview and by the drawing. Two
/// would be a foot drawn over rows the preview believed it had, which is a
/// foot written across the file.
#[must_use]
pub fn room_for(picker: &Picker, editor: Rect) -> Rect {
    crate::footed(editor, &hints(picker))
}

/// The fewest rows worth giving a preview.
///
/// A preview showing three lines has stopped being a preview and become a
/// strip of decoration above the prompt.
const LEAST_PREVIEW_ROWS: u16 = 4;

/// How much of the tab row's right-hand end is spoken for.
///
/// The glyphs that say the tabs can be walked, and a column either side of
/// them, so a note about what is still arriving sits beside them rather
/// than on top of them.
const FILLING_INSET: usize = 6;

/// Where the preview goes, if there is room for one.
///
/// Below the list rather than beside it: a terminal is usually wider than one
/// column of paths needs and never taller than it could use, and splitting
/// left and right makes both halves narrow at once.
///
/// The list takes its ten rows and the preview takes the rest, so a taller
/// terminal buys more of the file rather than more file names — which is the
/// way round that matters, since the list is filtered by typing and the
/// preview is not.
///
/// Only for a list that said its rows name something to show. A palette of
/// commands has nothing.
#[must_use]
pub fn preview_region(picker: Option<&Picker>, editor: Rect) -> Option<Rect> {
    let picker = picker?;
    if !picker.shows_previews() {
        return None;
    }
    let editor = room_for(picker, editor);
    // And only where there is anywhere to put it. The arithmetic below is a
    // full-area list's: a compact one sits on the status bar with nothing
    // under it, so there is no room to divide.
    if picker.layout() != PickerLayout::FullArea {
        return None;
    }
    // The list, the rule between them, and enough left to be worth it.
    let rows = list_region_rows(picker, editor.width);
    if editor.height < rows + 1 + LEAST_PREVIEW_ROWS {
        return None;
    }
    Some(Rect {
        y: editor.y + rows + 1,
        height: editor.height - rows - 1,
        ..editor
    })
}

/// The room a compact list leaves the file it is drawn on.
///
/// The rule above it is the list's too: it is there to say the list and the
/// file are two things, and a row of rule is not a row of file.
#[must_use]
pub fn room_above(picker: &Picker, editor: Rect) -> Rect {
    let region = region(picker, editor);
    let height = region
        .y
        .saturating_sub(1)
        .saturating_sub(editor.y)
        .min(editor.height);
    Rect { height, ..editor }
}

/// Whether a compact list shows what its selection names in that room, and
/// how much of it there is to show in.
///
/// The room above the rows is the code's own room. A compact list is drawn
/// on the file precisely so the file stays readable, and for a row in *that*
/// file the file is the preview: the list scrolls it and the reader looks
/// straight at it. A row in another file has nothing to scroll and nowhere
/// to be shown -- and the answer is not to open it, which would be going
/// somewhere they have not chosen to go. It is to draw that file where this
/// one is.
///
/// Nothing comes or goes as the selection moves: the room above the list is
/// full either way, of one file or of another. Which is what makes this safe
/// to settle per row rather than per list, and what [`Picker::previews`] is
/// careful about where it refuses to.
#[must_use]
pub fn preview_over(picker: Option<&Picker>, above: Rect) -> Option<Rect> {
    let picker = picker?;
    if !picker.shows_previews() || picker.layout() == PickerLayout::FullArea {
        return None;
    }
    (above.height >= LEAST_PREVIEW_ROWS).then_some(above)
}

/// The list, above the prompt.
pub struct PickerView<'a> {
    picker: &'a Picker,
    theme: &'a Theme,
    /// How far the ticker has got, for a row whose mark turns.
    ///
    /// The drawing reads nothing else that changes with time, and it is
    /// here for one thing: a conversation an agent is working in says so
    /// from this list, because that is where a reader who is not looking at
    /// it would see it.
    phase: u32,
}

impl<'a> PickerView<'a> {
    /// Borrows what a view needs, which is the list and the colours to draw
    /// it in.
    ///
    /// One constructor, including for a list that is not the one taking keys
    /// -- the commands an agent takes, against the box a message is written
    /// in. It is drawn by the same code as every other list, because it is
    /// the same thing to a reader: rows, one of them chosen, what matched
    /// marked.
    #[must_use]
    pub const fn new(picker: &'a Picker, theme: &'a Theme, phase: u32) -> Self {
        Self {
            picker,
            theme,
            phase,
        }
    }
}

/// Where the list goes within the editor region.
///
/// A compact list sits on the bottom edge and grows upwards only as far as it
/// has to, so the code above stays readable. A full-area list gives up its
/// bottom rows to the preview, and one row between them to the rule that says
/// they are different things.
///
/// A free function rather than a method on the view, because measuring is not
/// drawing: the key handler has to know how tall the list will be in order to
/// size a page, and it was reaching that fact by building a widget it never
/// rendered. Nothing here reads the theme, which is the other half of saying
/// the same thing.
#[must_use]
pub fn region(picker: &Picker, editor: Rect) -> Rect {
    let editor = room_for(picker, editor);
    match picker.layout() {
        PickerLayout::FullArea => match preview_region(Some(picker), editor) {
            Some(_) => Rect {
                height: list_region_rows(picker, editor.width),
                ..editor
            },
            None => editor,
        },
        PickerLayout::Compact { .. } => {
            let wanted = picker.visible_rows(editor.height, editor.width);
            Rect {
                y: editor.y + editor.height - wanted,
                height: wanted,
                ..editor
            }
        }
    }
}

/// The rows of the list, within the region it is drawn in.
///
/// A list with tabs keeps its first two rows for them -- the tabs and the rule
/// under them -- and one that says what it is about keeps the rows that takes,
/// so what a reader walks is what is left. One function, shared with the
/// drawing and with everything that has to know how many rows are on screen: a
/// window settled on a height the rows do not have scrolls before the last row
/// it drew, and a page steps further than the reader can see.
#[must_use]
pub fn rows_region(picker: &Picker, region: Rect) -> Rect {
    let above = picker
        .tab_rows()
        .saturating_add(picker.about_rows(region.width));
    Rect {
        y: region.y + above,
        height: region.height.saturating_sub(above),
        ..region
    }
}

/// The row the tabs are drawn on, where the list has any.
///
/// Below whatever the list says about itself, which is the same offset the
/// drawing takes.
#[must_use]
pub fn tab_row(picker: &Picker, region: Rect) -> Option<Rect> {
    (picker.tab_rows() > 0).then(|| Rect {
        y: region.y + picker.about_rows(region.width),
        height: 1,
        ..region
    })
}

/// Which row of the list a point on screen is on, and whether it is on
/// that row's arrow.
///
/// The way back out of the arithmetic the drawing goes in by: the rows
/// start below the tabs and whatever the list says about itself, the
/// window decides which of them is first, and a row's own columns begin
/// one in and two more for every level of depth. Each of those numbers is
/// read here from the same place the drawing takes it, because a pointer
/// that landed on a different row than the one it looks like it landed on
/// is worse than a pointer that does nothing.
///
/// `None` for a point outside the rows, or past the last of them.
#[must_use]
pub fn row_at(picker: &Picker, region: Rect, x: u16, y: u16) -> Option<(usize, bool)> {
    let list = rows_region(picker, region);
    if y < list.y || y >= list.bottom() || x < list.x || x >= list.right() {
        return None;
    }
    // The bar takes the last column, and a row never draws under it.
    let rows = list.width.saturating_sub(SCROLLBAR_WIDTH);
    if x >= list.x.saturating_add(rows) {
        return None;
    }
    let first = picker.first_visible(list.height);
    let at = first + usize::from(y - list.y);
    let item = picker.matches().nth(at)?;
    // The row's own columns, in the order the drawing spends them: one to
    // stand clear of the edge, two for every level in, and then the arrow.
    let indent = 1u16.saturating_add(item.depth.saturating_mul(2).min(rows / 3));
    let on_the_arrow = item.opens.is_some()
        && x >= list.x.saturating_add(indent)
        && x < list.x.saturating_add(indent).saturating_add(MARKER_COLUMNS);
    Some((at, on_the_arrow))
}

/// How many rows of the list a reader will actually see, given the room.
///
/// The two above composed, which is the only question anybody outside this
/// module asks: the window follows the selection when it knows how many rows
/// are on screen, the matched characters are worked out for those rows, and a
/// page moves by that many. Named once so that three callers cannot each get
/// the composition slightly wrong.
#[must_use]
pub fn rows_drawn(picker: &Picker, room: Rect) -> u16 {
    rows_region(picker, region(picker, room)).height
}

impl Widget for PickerView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // The whole region first. A list with fewer rows than the region
        // would otherwise leave the code showing underneath, which reads as a
        // half-drawn screen rather than as a short list.
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );

        // What the list is about, above everything: a question the reader
        // did not start needs its words before its answers, and a rule
        // under them is what makes the answers read as answers rather than
        // as more of the sentence.
        let about_rows = self.picker.about_rows(area.width);
        if let Some(about) = self.picker.what_about()
            && about_rows > 0
        {
            let words = obelus_text::wrapped(about, area.width.saturating_sub(2));
            for (row, words) in words.iter().enumerate().take(usize::from(about_rows) - 1) {
                let Ok(row) = u16::try_from(row) else { break };
                crate::write(
                    cells,
                    area.x + 1,
                    area.y + row,
                    words,
                    Style::new()
                        .fg(self.theme.foreground)
                        .bg(self.theme.background),
                );
            }
            crate::rule(
                cells,
                Rect {
                    y: area.y + about_rows - 1,
                    height: 1,
                    ..area
                },
                self.theme,
            );
        }

        // The tabs next, with their own rule under them, and the list below
        // whatever they took. The rule is why a tab row reads as a heading
        // over the list rather than as its first row.
        let list = rows_region(self.picker, area);
        let tabs = self.picker.tab_rows();
        if tabs > 0 {
            let under = Rect {
                y: area.y + about_rows,
                height: area.height.saturating_sub(about_rows),
                ..area
            };
            let used = crate::tabs(
                cells,
                under,
                self.picker.tabs(),
                self.picker.tab(),
                self.theme,
            );
            // Beside the tabs, inside the key glyphs that already sit there: a
            // list still filling has to say so somewhere that does not move
            // its rows out from under the reader when it stops.
            if let Some(note) = self.picker.is_filling() {
                let room = usize::from(under.width)
                    .saturating_sub(obelus_text::text_width(note) + FILLING_INSET);
                if let Ok(offset) = u16::try_from(room)
                    && under.x + offset > used
                {
                    crate::write(
                        cells,
                        under.x + offset,
                        under.y,
                        note,
                        Style::new().fg(self.theme.gutter).bg(self.theme.background),
                    );
                }
            }
            crate::rule(
                cells,
                Rect {
                    y: under.y + 1,
                    height: 1,
                    ..area
                },
                self.theme,
            );
        }

        // Nothing to list. A region of background says only that a key did
        // something and nothing came of it; the reason is the whole content
        // of that region. Below the tabs rather than instead of them: an
        // empty tab is the one place a reader most needs to see the others.
        if let Some(reason) = self.picker.nothing_to_show() {
            crate::nothing(cells, list, reason, self.theme);
            return;
        }

        // The same bar the editor has, for the same reason: a window over
        // something longer than itself should say how much longer. The rows
        // give up its column, so a row never draws under it.
        let first = self.picker.first_visible(list.height);
        let rows = Rect {
            width: list.width.saturating_sub(SCROLLBAR_WIDTH),
            ..list
        };
        // Only when the matches do not fit, which is the window's own
        // answer -- the same one every other list asks it for.
        let matched = self.picker.match_count();
        if self.picker.window().scrollable(list.height) {
            crate::scrollbar(cells, list, first, matched, self.theme);
        }

        // Where this list has got to, for a front end that can draw it
        // arriving rather than simply being there.
        if let Ok(top) = i64::try_from(first) {
            crate::shapes::scrolled(rows, top);
        }

        let selected = self.picker.selected();
        for (row, (index, item)) in self
            .picker
            .matches()
            .enumerate()
            .skip(first)
            .take(usize::from(list.height))
            .enumerate()
        {
            let Ok(row) = u16::try_from(row) else { break };
            self.row(
                cells,
                rows,
                rows.y + row,
                item,
                index == selected,
                self.picker.indices_at(index),
            );
        }
    }
}

impl PickerView<'_> {
    /// One row: its background, then its icon, label, detail and key.
    fn row(
        &self,
        cells: &mut CellBuffer,
        area: Rect,
        y: u16,
        item: &PickerItem,
        chosen: bool,
        matched: &[u32],
    ) {
        let background = if chosen {
            self.theme.selected_row_background
        } else {
            self.theme.background
        };
        // A row that cannot be chosen is drawn in the colour the gutter uses
        // -- present, and plainly not for now. Its own colours are dropped
        // with it: a dim row with a bright name in it reads as available.
        let style = if item.enabled {
            Style::new().fg(self.theme.foreground).bg(background)
        } else {
            Style::new().fg(self.theme.gutter).bg(background)
        };
        // A row that names a thing is coloured by what it names, in the same
        // colours the code itself uses: an outline of a file is a list of
        // its own words, and reading it should feel like reading the file.
        // The matched characters still win over this -- why a row is in the
        // list beats what the row is.
        let label_style = match (item.status, item.kind) {
            // Nothing wins over a row that cannot be chosen: a dim row with
            // a bright name in it reads as available, which is the one
            // thing the dim is there to deny. Nothing in Obelus currently
            // makes a row that is both disabled and carries a status, so
            // there is no test under this -- it is here because the rule
            // written above it was not true.
            _ if !item.enabled => style,
            // What git says wins over what the syntax layer says: a list of
            // a project's files is mostly files nobody has touched, and the
            // few that have been are what a reader is looking for.
            (Some(FileStatus::Changed), _) => style.fg(self.theme.change_modified),
            (Some(FileStatus::New), _) => style.fg(self.theme.change_added),
            // The colour a deleted line wears in the margin, and struck
            // through: the name is of something that is not there, and a
            // list where that is the only difference is a list a reader
            // has to look twice at. A terminal that has not got the
            // attribute ignores it and the colour still says it.
            (Some(FileStatus::Gone), _) => style
                .fg(self.theme.change_removed)
                .add_modifier(ratatui::style::Modifier::CROSSED_OUT),
            // Dim, because the project said it does not keep this one: it is
            // in the list only because the reader asked for the ignored
            // ones too, and a build artefact in the same ink as the source
            // beside it is a list that has stopped saying which is which.
            (Some(FileStatus::Ignored), _) => style.fg(self.theme.gutter),
            (None, Some(kind)) => style.fg(self.theme.syntax.colour(kind)),
            (None, None) => style,
        };
        fill(
            cells,
            Rect {
                y,
                height: 1,
                ..area
            },
            style,
        );

        // Two columns a level, which is enough to see and cheap enough to
        // spend: an outline of deeply nested code otherwise pushes the names
        // off the row it is meant to be showing.
        let mut column = 1u16.saturating_add(item.depth.saturating_mul(2).min(area.width / 3));
        // Before the icon, because it is not part of the name: it says
        // something about the row rather than about the thing it names.
        //
        // In whichever colour that is. A mark for work that is not on disk
        // gets the one the status row marks the same fact in, and is not
        // dimmed: it is the one thing in a list of files a reader must not
        // miss, and the gutter's grey -- which is the colour of a line
        // number, chosen to recede -- made it something to notice only once
        // it was pointed out. The rest do want to recede, and get it: a
        // fold arrow is the arrow the gutter and the transcript draw, and a
        // reader who learnt the mark there has to find it here.
        // A row that opens wears the arrow, in the colour a mark beside a
        // name wears. Drawn from what the row *says* about itself rather
        // than from a marker that happens to hold an arrow: the same field
        // the keys and the pointer ask, so the mark and what pressing it
        // does cannot come apart.
        let arrow = item
            .opens
            .map(|open| (self.theme.gutter, format!("{} ", crate::opens(open))));
        if let Some((marking, marker)) = item.marker.as_ref() {
            let colour = match marking {
                Marking::Unwritten => self.theme.status_stale,
                Marking::Aside => self.theme.gutter,
                // An agent at work in a conversation nobody is looking at,
                // and one waiting on an answer: the second is the reader's
                // to do something about, so it is the one that stands out.
                Marking::Working => self.theme.gutter,
                Marking::Waiting => self.theme.status_stale,
            };
            // A mark with nothing in it is one that turns: the frame comes
            // from the ticker rather than from the row, because what it is
            // saying is that time is passing somewhere else.
            let marker = match marker.is_empty() {
                true => crate::spinning(self.phase).to_string(),
                false => marker.clone(),
            };
            // With a blank column after it, the way the icon has one: two
            // glyphs touching read as one glyph nobody has seen before.
            let marker = format!("{marker} ");
            column = at(
                cells,
                area,
                column,
                y,
                &marker,
                style.fg(colour),
                &Marked::plain(),
            );
        } else if let Some((colour, arrow)) = arrow {
            column = at(
                cells,
                area,
                column,
                y,
                &arrow,
                style.fg(colour),
                &Marked::plain(),
            );
        } else if self.picker.marked() {
            // The column is kept on the rows that have nothing to put in
            // it, so that the icons and the names of a list line up. A name
            // that sat two columns right of its neighbours because that
            // file is unwritten says the same thing twice, and says it in a
            // way that makes the list harder to read down.
            column = column.saturating_add(MARKER_COLUMNS);
        }
        if let Some(icon) = item.icon {
            let mut glyph = String::new();
            glyph.push(icon);
            // In the label's own colour, because the glyph is part of the
            // name: a file git says has changed is a changed file picture
            // and all, and an icon left in the plain foreground reads as a
            // second thing on the row with a colour of its own.
            column = at(
                cells,
                area,
                column,
                y,
                &glyph,
                label_style,
                &Marked::plain(),
            );
            // One blank column after it, always. The terminal allocates one
            // cell for a private-use codepoint, and the icons in a Nerd
            // Font's non-`Mono` variant are drawn two cells wide, so the
            // glyph bleeds to the right. This is what it bleeds into.
            column = column.saturating_add(1);
        }

        // What the right-aligned text needs, plus a gap, comes out of
        // everything else's room first -- but never more than half the row.
        // It used to be the one part that never got cut, which was right
        // while it held a key hint or a line number; a search row's trailing
        // is a path, and a path longer than the row left the label with no
        // columns at all: a list of icons with nothing beside them.
        let trailing = item.trailing.as_deref().unwrap_or_default();
        // Right of everything, including the trailing: a number is read down
        // a column, and a column with a ragged right edge is a column a
        // reader has to find the end of on every row.
        let counted = item
            .changed
            .map(|(added, removed)| (format!("+{added}"), format!("\u{2212}{removed}")));
        let counted_columns = counted.as_ref().map_or(0, |(up, down)| {
            u16::try_from(text_width(up) + text_width(down) + 2).unwrap_or(u16::MAX)
        });
        let wanted = if trailing.is_empty() {
            counted_columns
        } else {
            u16::try_from(text_width(trailing) + 2)
                .unwrap_or(u16::MAX)
                .saturating_add(counted_columns)
        };
        let reserved = wanted.min(area.width / 2);
        let limit = area.width.saturating_sub(1).saturating_sub(reserved);
        let inner = Rect {
            width: limit,
            ..area
        };

        // A path too long for the row loses its head, not its tail: the file
        // name is the part being looked for, and the directories above it
        // are the part already known. A sentence is the other way round and
        // loses its end, where the words it can spare are.
        //
        // Either way the cut is marked. It used to be that a sentence simply
        // ran out of row, which a reader cannot tell from a sentence that
        // ends there: a commit subject stopping mid-word reads as a subject
        // whose author stopped mid-word.
        let room = usize::from(limit.saturating_sub(column));
        let total = item.label.chars().count();
        let (dropped, elided) = match item.prose {
            true => (0, drop_from_right(&item.label, room)),
            false => (drop_from_left(&item.label, room), 0),
        };
        if dropped >= total || elided >= total {
            // Not even room for the ellipsis.
            return;
        }
        if dropped > 0 {
            column = at(cells, inner, column, y, "\u{2026}", style, &Marked::plain());
        }
        // Cut before it is written rather than left to run off the edge, so
        // there is a column for the mark. The characters kept are the ones
        // the label started with, so the matched indices and the syntax runs
        // -- which count from the front -- still land where they belong.
        let shown = match elided {
            0 => item.label.as_str(),
            _ => {
                let end = item
                    .label
                    .char_indices()
                    .nth(total - elided)
                    .map_or(item.label.len(), |(index, _)| index);
                &item.label[..end]
            }
        };
        column = at(
            cells,
            inner,
            column,
            y,
            shown,
            label_style,
            &Marked {
                matched: Matched::Indices(matched),
                mark: self.theme.picker_match_background,
                syntax: item
                    .colours
                    .as_deref()
                    .filter(|runs| !runs.is_empty())
                    .map(|runs| (runs, self.theme)),
                skip: dropped,
            },
        );
        // In the row's own style and not the label's: the mark says the row
        // ran out of room, which is a fact about the row and not part of
        // what it says. The same reason the one in front of a path is.
        if elided > 0 {
            column = at(cells, inner, column, y, "\u{2026}", style, &Marked::plain());
        }

        let dim = style.fg(self.theme.gutter);
        if let Some(detail) = &item.detail {
            column = at(
                cells,
                inner,
                column.saturating_add(2),
                y,
                detail,
                dim,
                &Marked::plain(),
            );
        }

        if !trailing.is_empty() {
            // Cut from the left, like a label: the end of a path is the file
            // name and the line, which is the part that says where to go.
            let room = usize::from(reserved.saturating_sub(2));
            let dropped = drop_from_left(trailing, room);
            let shown = trailing.chars().count().saturating_sub(dropped);
            if shown > 0
                && let Ok(offset) = u16::try_from(
                    usize::from(area.width.saturating_sub(counted_columns))
                        .saturating_sub(text_width(trailing) + 1),
                )
                .map(|offset| offset.max(area.width.saturating_sub(reserved).saturating_add(1)))
                && offset >= column
            {
                if dropped > 0 {
                    at(cells, area, offset, y, "\u{2026}", dim, &Marked::plain());
                    at(
                        cells,
                        area,
                        offset + 1,
                        y,
                        trailing,
                        dim,
                        &Marked {
                            skip: dropped + 1,
                            ..Marked::plain()
                        },
                    );
                } else {
                    at(cells, area, offset, y, trailing, dim, &Marked::plain());
                }
            }
        }

        // Last, at the row's right-hand edge, in the colours the margin marks
        // the same two facts in. Never cut: its width came out of the label's
        // before the label was, and a number missing a digit is worse than no
        // number -- so a row with no room for it keeps its name instead.
        if let Some((up, down)) = counted {
            let width = u16::try_from(text_width(&up) + text_width(&down) + 1).unwrap_or(u16::MAX);
            if let Some(offset) = area.width.checked_sub(width + 1)
                && offset >= column
            {
                let after = at(
                    cells,
                    area,
                    offset,
                    y,
                    &up,
                    style.fg(self.theme.change_added),
                    &Marked::plain(),
                );
                at(
                    cells,
                    area,
                    after.saturating_add(1),
                    y,
                    &down,
                    style.fg(self.theme.change_removed),
                    &Marked::plain(),
                );
            }
        }
    }
}

/// The shared row writer, for a column counted from the row's own left
/// edge rather than from the screen's.
///
/// Every list in Obelus draws its characters through
/// [`crate::write_marked`] -- what marks a match, what colours a line of
/// code, what a truncated head skips. A picker's rows are laid out relative
/// to the row, so this is the one line of arithmetic between the two.
fn at(
    cells: &mut CellBuffer,
    area: Rect,
    column: u16,
    y: u16,
    contents: &str,
    style: Style,
    marked: &Marked<'_>,
) -> u16 {
    crate::write_marked(cells, area, area.x + column, y, contents, style, marked)
        .saturating_sub(area.x)
}
