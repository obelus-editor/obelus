//! What the call the cursor is inside takes.
//!
//! A box over the cursor's line rather than under it: what is below the
//! cursor is the code being written, and the argument list belongs with the
//! call above. The same box as the completion panel, because it is the same
//! kind of thing -- a server's answer, put where the reader is looking.
//!
//! Every signature the server sent, the active one first and its argument
//! marked, then what the call says about itself under a rule. Capped by the
//! panel, which is somebody else's text in a box over the code.
//!
//! **The argument being marked is the one thing that may not be clipped
//! away.** A label wider than the box has to lose something, and losing its
//! tail is the obvious answer until the parameter being typed is in the
//! tail -- which is the common case, because a reader types the arguments in
//! order and the last one is furthest right. So the window over the label is
//! chosen to contain the mark, and the label and where the mark is in it are
//! worked out together, in one place: what is drawn has to be what was
//! measured.

use obelus_component::signature::Signature;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{Screen, editor, truncate_from_left, truncate_from_right, write};

/// How wide the box is.
///
/// From the labels, which are known without laying anything out -- the
/// documentation is wrapped to this, so this cannot depend on it.
fn width_of(signature: &Signature, editor: Rect) -> u16 {
    let widest = signature
        .shown()
        .iter()
        .map(|shown| obelus_text::text_width(&shown.label))
        .max()
        .unwrap_or(0);
    let labels = u16::try_from(widest + usize::from(crate::PANEL_INSET * 2)).unwrap_or(u16::MAX);
    // What a box with prose in it wants, asked of the hover -- which is the
    // same question about the same kind of text, and two answers to it would
    // be two widths a paragraph is read in. Only where there is prose: a
    // panel of labels is as wide as its labels and no wider.
    let prose = match signature.documented() {
        true => crate::hover::width(editor),
        false => 0,
    };
    labels.max(prose).min(editor.width)
}

/// The cells inside it, which is what the words are given.
fn room_of(signature: &Signature, editor: Rect) -> u16 {
    width_of(signature, editor).saturating_sub(crate::PANEL_INSET * 2)
}

/// What goes in the box, in rows.
///
/// One answer for both callers: the box is given a height by `layout` and
/// filled by `draw`, and a box measured by one and filled by the other is a
/// box with a blank row in it where the two disagreed -- which is what a
/// clipped one had, because the height counted rows of documentation that
/// the filling then found no room for.
struct Plan {
    /// How many signatures are drawn.
    labels: usize,
    /// How many are left out, where there is a row to say so in.
    ///
    /// One number rather than a count and a flag beside it: nothing left out
    /// and no room to say what was are the same thing to draw, and two
    /// fields that have to agree are two fields that can stop agreeing.
    left: usize,
    /// How many rows of prose, under a rule of its own.
    documentation: usize,
}

impl Plan {
    /// How many rows of the box's inside it is.
    const fn rows(&self) -> usize {
        let documentation = match self.documentation {
            0 => 0,
            rows => rows + 1,
        };
        self.labels + (self.left > 0) as usize + documentation
    }
}

/// What will fit in a given number of rows.
///
/// Asked twice, and that is the point: once with everything there is, to
/// say how tall the box wants to be, and once with the rows it was given.
fn plan(signature: &Signature, room: u16, rows: usize) -> Plan {
    let all = signature.shown().len() + signature.more();
    let labels = match all > rows {
        // One row of room goes on the call the cursor is in, not on a count
        // of the ones there was no room for: a box saying `+2` and nothing
        // else is a box with nothing in it.
        true => rows.saturating_sub(1).max(1).min(rows),
        false => signature.shown().len().min(rows),
    };
    // Said only where there is a row to say it in, and about what was
    // actually left out rather than about what the cap dropped.
    let left = match labels < rows {
        true => all - labels,
        false => 0,
    };
    let room_left = rows.saturating_sub(labels + usize::from(left > 0));
    // The prose wants a rule of its own as well as a row to be in.
    let documentation = match room_left >= 2 {
        true => signature.documentation(room).len().min(room_left - 1),
        false => 0,
    };
    Plan {
        labels,
        left,
        documentation,
    }
}

/// Where the box goes, if there is one to draw.
#[must_use]
pub fn layout(app: &impl Screen, editor: Rect) -> Option<Rect> {
    let signature = app.signature()?;
    let buffer = app.current_buffer()?;
    if editor.width < 4 || editor.height < 3 {
        return None;
    }
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::changed(app.changes()),
        !buffer.folds().is_empty(),
    );
    let (row, cell) = buffer.cursor_screen_cell(app.text_area())?;
    let cursor_y = editor.y + row;
    let cursor_x = editor.x.saturating_add(offset).saturating_add(cell);

    let width = width_of(signature, editor);
    let room = room_of(signature, editor);
    let everything = usize::from(u16::MAX);
    let wanted = u16::try_from(plan(signature, room, everything).rows())
        .unwrap_or(u16::MAX)
        .max(1)
        + 2;
    let over = cursor_y.saturating_sub(editor.y);
    let under = editor.bottom().saturating_sub(cursor_y + 1);
    // Above the cursor where the whole of it fits there, below where it
    // fits there instead -- and on the roomier side with as much as that
    // side holds where it fits neither. The rows are in the order they
    // matter, the call the cursor is in first, so what a clipped box loses
    // is from the bottom. Giving up instead is a server's long doc comment
    // taking the signature off the screen, which is the one thing somebody
    // else's text may not do.
    let (y, height) = if wanted <= over {
        (cursor_y - wanted, wanted)
    } else if wanted <= under {
        (cursor_y + 1, wanted)
    } else if over >= under {
        (cursor_y - over, over)
    } else {
        (cursor_y + 1, under)
    };
    // Two rows of box and nothing between them is a box saying there was
    // something to say.
    if height < 3 {
        return None;
    }
    // And a clipped box is as tall as what will go in *it*, which is less
    // than what it was clipped to whenever the rows it lost were the prose's
    // -- the rule and the paragraph go together, so losing one loses both.
    let inside = u16::try_from(plan(signature, room, usize::from(height - 2)).rows())
        .unwrap_or(height - 2)
        .max(1);
    let height = inside + 2;
    Some(Rect {
        x: cursor_x.min(editor.right().saturating_sub(width)),
        // Where it hangs above the cursor its foot is the cursor's line, so
        // a shorter box starts lower down.
        y: match y < cursor_y {
            true => cursor_y - height,
            false => y,
        },
        width,
        height,
    })
}

/// A label as it will be drawn, and where the marked argument is in it.
///
/// Both together, because the second is an offset into the first: a label
/// clipped after the mark was worked out is a label with the mark somewhere
/// else. The window keeps the mark where the room allows -- the tail goes
/// first, and where the mark is *in* the tail the front goes instead.
fn shown(
    label: &str,
    active: Option<(usize, usize)>,
    room: usize,
) -> (String, Option<(usize, usize)>) {
    let characters: Vec<char> = label.chars().collect();
    let Some((from, to)) = active else {
        return (truncate_from_right(label, room), None);
    };
    let to = to.min(characters.len());
    if from >= to {
        return (truncate_from_right(label, room), None);
    }
    // The mark ends inside the room the label has, so the tail is what
    // there is no room for and the offsets stand. What the mark may reach
    // is a cell less than the room where the tail has to go, because the
    // ellipsis that says so takes that cell: a mark measured against the
    // whole room ends under the ellipsis, drawn bold, with the argument's
    // last character gone.
    let ends: String = characters[..to].iter().collect();
    let reach = match obelus_text::text_width(label) <= room {
        true => room,
        false => room.saturating_sub(1),
    };
    if obelus_text::text_width(&ends) <= reach {
        return (truncate_from_right(label, room), Some((from, to)));
    }
    // It does not, so the front goes: the window ends where the mark ends,
    // with an ellipsis in place of what was dropped. Where the mark itself
    // is wider than the room, what is left is the end of it -- there is
    // nothing better to show than the argument being typed.
    let kept = truncate_from_left(&ends, room);
    let shown: Vec<char> = kept.chars().collect();
    // After the ellipsis at the earliest, for the same reason: it is not
    // part of the argument.
    let first = usize::from(kept.starts_with('\u{2026}'));
    let marked = shown.len().saturating_sub(to - from).max(first);
    (kept, Some((marked, shown.len())))
}

/// Draws it.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &impl Screen) {
    let Some(signature) = app.signature() else {
        return;
    };
    let theme = app.theme();
    if area.width < 3 || area.height < 3 {
        return;
    }
    crate::panel(cells, area, theme);

    let inside = crate::inside(area);
    let room = usize::from(inside.width);
    let quiet = Style::new().fg(theme.syntax.comment).bg(theme.background);
    let mut y = area.y + 1;
    let bottom = area.bottom().saturating_sub(1);

    // What goes in it, from the same function that gave it its height.
    let filling = plan(signature, inside.width, usize::from(area.height - 2));

    for line in signature.shown().iter().take(filling.labels) {
        if y >= bottom {
            return;
        }
        let (label, marked) = shown(&line.label, line.active, room);
        write(cells, inside.x, y, &label, quiet);
        // The argument being typed, over the top: the rest of the line is
        // there to be read past, and this is the part the reader is on.
        if let Some((from, to)) = marked {
            let characters: Vec<char> = label.chars().collect();
            let before: String = characters[..from.min(characters.len())].iter().collect();
            let part: String = characters[from.min(characters.len())..to.min(characters.len())]
                .iter()
                .collect();
            if let Ok(offset) = u16::try_from(obelus_text::text_width(&before)) {
                write(
                    cells,
                    inside.x + offset,
                    y,
                    &part,
                    Style::new()
                        .fg(theme.foreground)
                        .bg(theme.background)
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
        y += 1;
    }

    // How many did not fit, in the same words a transcript's row says it
    // in: a count rather than a sentence, which would have to begin with a
    // number.
    if filling.left > 0 {
        write(cells, inside.x, y, &format!("+{}", filling.left), quiet);
        y += 1;
    }

    // And what the call says about itself, under a rule: a paragraph beside
    // a signature with nothing between them reads as more signature.
    if filling.documentation == 0 {
        return;
    }
    crate::divider(cells, area, y, theme);
    y += 1;
    for words in signature
        .documentation(inside.width)
        .iter()
        .take(filling.documentation)
    {
        if y >= bottom {
            return;
        }
        write(cells, inside.x, y, words, quiet);
        y += 1;
    }
}

#[cfg(test)]
mod tests {
    /// The mark stays on screen, whichever side of the label has to go.
    ///
    /// Broken deliberately by clipping the tail whatever the mark is doing,
    /// which is what this did: the row then reads `fn takes(first: &str,
    /// second…` with nothing marked on it at all, on a panel whose only
    /// job is to mark one argument.
    #[test]
    fn the_argument_being_typed_is_what_the_room_is_spent_on() {
        let label = "fn takes(first: &str, second: &str, third: &Something)";
        let last = label.find("third").expect("the last argument");
        let end = label.len() - 1;

        // Room for all of it: nothing moves.
        let (whole, marked) = super::shown(label, Some((last, end)), label.len());
        assert_eq!(whole, label);
        assert_eq!(marked, Some((last, end)));

        // Room for half of it, with the mark in the half that does not fit.
        let (shown, marked) = super::shown(label, Some((last, end)), 24);
        let (from, to) = marked.expect("the mark");
        let characters: Vec<char> = shown.chars().collect();
        let part: String = characters[from..to].iter().collect();
        assert_eq!(
            part, "third: &Something",
            "the argument being typed is not what was kept: {shown:?}"
        );
        assert!(
            obelus_text::text_width(&shown) <= 24,
            "it is wider than the room it was given: {shown:?}"
        );

        // And the ellipsis is not allowed to eat the mark: with room for
        // everything up to the mark's end and nothing after it, the tail
        // still has to be said, and the cell it is said in was the
        // argument's last character.
        let (shown, marked) = super::shown(label, Some((last, end)), end);
        let (from, to) = marked.expect("the mark");
        let characters: Vec<char> = shown.chars().collect();
        let part: String = characters[from..to].iter().collect();
        assert_eq!(
            part, "third: &Something",
            "the ellipsis was drawn as part of the argument: {shown:?}"
        );

        // The mark near the front, where the tail is what goes.
        let first = label.find("first").expect("the first argument");
        let (shown, marked) = super::shown(label, Some((first, first + 11)), 24);
        let (from, to) = marked.expect("the mark");
        let characters: Vec<char> = shown.chars().collect();
        let part: String = characters[from..to].iter().collect();
        assert_eq!(part, "first: &str");
        assert!(
            shown.starts_with("fn takes("),
            "the front was dropped: {shown:?}"
        );
    }

    /// A label with nothing marked is clipped the ordinary way.
    #[test]
    fn a_signature_the_cursor_is_not_in_loses_its_tail() {
        let label = "fn other(a: u32, b: u32)";
        let (shown, marked) = super::shown(label, None, 12);
        assert_eq!(marked, None);
        assert!(shown.starts_with("fn other("), "{shown:?}");
        assert!(obelus_text::text_width(&shown) <= 12, "{shown:?}");
    }
}
