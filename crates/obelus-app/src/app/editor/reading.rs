//! A file shown as its reading: the keys and the pointer of the cursor in
//! it, and what a copy takes from it.
//!
//! The cursor and what it holds are `obelus_component::reading`'s, and the
//! drawing of them `obelus_ui::reading`'s. What is here is the seam with the
//! buffer, which keeps where the reading is scrolled to: the bar and the
//! wheel read the one answer, and the cursor moves it where it has to be
//! seen.

use obelus_component::reading::Outcome;

use crate::app::*;

impl App {
    /// How much room the reading on screen has, in the terms its keys are
    /// measured in: the width it is laid out at (`frame`), and the rows of
    /// the region it is drawn on.
    pub(in crate::app) fn reading_room(&self) -> obelus_component::page::Room {
        obelus_component::page::Room {
            height: self.editor_area.height,
            reading: self
                .editor_area
                .width
                .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH),
        }
    }

    /// Offers a key to the cursor in the reading on screen, if one is.
    pub(in crate::app) fn reading_key(&mut self, key: &KeyEvent) -> bool {
        let room = self.reading_room();
        let Some(was) = self
            .current_buffer()
            .map(|buffer| buffer.viewport().top.get())
        else {
            return false;
        };
        let mut top = was;
        let Some(rendered) = self.rendered.as_mut() else {
            return false;
        };
        let count = rendered.rows.len();
        match rendered
            .cursor
            .handle_key(key, &rendered.rows, room, &mut top)
        {
            Outcome::Ignored => return false,
            Outcome::Consumed => {}
            Outcome::Copy(code) => {
                self.copied(&code, "code");
                return true;
            }
        }
        if top != was
            && let Some(buffer) = self.current_buffer_mut()
        {
            let by = isize::try_from(top).unwrap_or(isize::MAX)
                - isize::try_from(was).unwrap_or(isize::MAX);
            buffer.scroll_rendering(by, count, room.height);
        }
        true
    }

    /// Whether anything is held in the reading on screen.
    pub(in crate::app) fn holding_in_the_reading(&self) -> bool {
        self.rendered
            .as_ref()
            .is_some_and(|rendered| rendered.cursor.holding())
    }

    /// What is held in the reading on screen, as it is drawn.
    pub(in crate::app) fn held_in_the_reading(&self) -> Option<String> {
        let rendered = self.rendered.as_ref()?;
        rendered.cursor.held_text(&rendered.rows)
    }

    /// What the pointer did to the reading on screen: a press puts the
    /// cursor where it landed and takes hold of the words there, and a drag
    /// takes hold of more -- the transcript's and a pull request's gestures,
    /// on the same arithmetic.
    pub(in crate::app) fn pointer_in_reading(
        &mut self,
        kind: crate::event::Pointer,
        x: u16,
        y: u16,
    ) {
        use crate::event::Pointer;

        let area = self.editor_area;
        let Some(top) = self
            .current_buffer()
            .map(|buffer| buffer.viewport().top.get())
        else {
            return;
        };
        let Some(rendered) = self.rendered.as_mut() else {
            return;
        };
        let rows = &rendered.rows;
        let place = obelus_ui::reading::place_at(area, rows, top, x, y);
        let spot = place.and_then(|place| {
            let laid: Vec<_> = rows
                .iter()
                .map(obelus_component::reading::Laid::of)
                .collect();
            obelus_component::page::spot_under(&laid, place)
        });
        let cursor = place.filter(|place| place.row < rows.len());
        if matches!(kind, Pointer::Pressed | Pointer::Dragged)
            && let Some(cursor) = cursor
        {
            rendered.cursor.stand(cursor);
        }
        match kind {
            Pointer::Moved => {}
            Pointer::Released => rendered.cursor.let_go_of_nothing(),
            Pointer::Pressed => match spot {
                Some(spot) => rendered.cursor.hold_from(spot),
                None => rendered.cursor.let_go(),
            },
            Pointer::Dragged => {
                if let Some(spot) = spot {
                    rendered.cursor.hold_to(spot);
                }
            }
        }
    }
}
