//! A pull request or an issue, opened from the list and read as a page.
//!
//! **Choosing one opens it; talking about it is a key of the page.** The
//! list's enter used to open the conversation that reviews a pull request,
//! claimed before there was a session -- so reading one meant starting a
//! review of it, and a row another Obelus was reviewing could not be read
//! at all. A page is read, not claimed: any window may have one open, and
//! the claim is taken where the conversation is, by `alt+a` on the page.
//!
//! **What `gh` says is kept once, by the list.** The page holds a copy of
//! what the list's preview was shown, and is told whenever an answer about
//! its number lands -- so a pull request open on the page and walked past in
//! the list is asked about once, and both say the same thing.

use obelus_component::discussion::{About, DiscussionView, Outcome};

use crate::app::{pulls::Asked, *};

impl App {
    /// The pull request or issue being read, where that is what is being
    /// read.
    #[must_use]
    pub(crate) fn discussion(&self) -> Option<&DiscussionView> {
        self.document(self.current?)?.discussion()
    }

    /// And to walk it.
    pub(crate) fn discussion_mut(&mut self) -> Option<&mut DiscussionView> {
        let id = self.current?;
        self.document_mut(id)?.discussion_mut()
    }

    /// Goes to the page about one, opening it where it is not open.
    ///
    /// `false` where the list has no row for it, which is the only thing a
    /// page can be opened from.
    pub(in crate::app) fn open_the_discussion(&mut self, asked: Asked) -> bool {
        let number = asked.number();
        let open = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::discussion)
                .is_some_and(|page| page.number() == number)
        });
        let at = match open {
            // What the list says it is now, which choosing its row again is
            // the moment to hear.
            Some(at) => {
                if let Some(about) = self.about(asked)
                    && let Some(page) = self
                        .documents
                        .get_mut(at)
                        .and_then(|document| document.as_mut().and_then(Document::discussion_mut))
                {
                    page.listed_as(about);
                }
                at
            }
            None => {
                let Some(about) = self.about(asked) else {
                    return false;
                };
                let mut page = DiscussionView::new(about);
                // What the preview was shown, at once: the reader chose the
                // row they had just been reading.
                if let Some(said) = self.pulls.discussions.get(&number) {
                    page.told(said.clone());
                } else if let Some(why) = self.pulls.refused.get(&number) {
                    page.refused(why.clone());
                }
                self.documents.push(Some(page.into()));
                self.documents.len() - 1
            }
        };
        let from = self.here();
        self.record(from);
        self.make_room(Room::Region);
        self.go_to_document(DocumentId::new(at));
        true
    }

    /// Tells every page what the list now says it is: a title edited, a
    /// push, a comment that moved when it last changed. The preview is laid
    /// out from the list's rows, and a page that kept the row it opened
    /// with said something else about the same pull request.
    pub(in crate::app) fn list_the_pages_again(&mut self) {
        let abouts: Vec<(usize, About)> = self
            .documents
            .iter()
            .enumerate()
            .filter_map(|(at, document)| {
                let page = document.as_ref()?.discussion()?;
                Some((at, self.about(page.about().asked())?))
            })
            .collect();
        for (at, about) in abouts {
            if let Some(page) = self
                .documents
                .get_mut(at)
                .and_then(|document| document.as_mut().and_then(Document::discussion_mut))
            {
                page.listed_as(about);
            }
        }
    }

    /// Tells every page about `number` what `gh` has just said of it.
    pub(in crate::app) fn tell_the_pages(&mut self, number: u64) {
        let said = self.pulls.discussions.get(&number).cloned();
        let refused = self.pulls.refused.get(&number).cloned();
        for page in self
            .documents
            .iter_mut()
            .flatten()
            .filter_map(Document::discussion_mut)
            .filter(|page| page.number() == number)
        {
            match (&said, &refused) {
                (_, Some(why)) => page.refused(why.clone()),
                (Some(said), None) => page.told(said.clone()),
                (None, None) => {}
            }
        }
    }

    /// Asks `gh` about the page being read, where what was kept about it is
    /// not current -- once a frame, and asking nothing twice: `ask_about`
    /// answers at once where it has asked already or an answer is on its
    /// way.
    pub(in crate::app) fn ask_about_the_page(&mut self) {
        if let Some(asked) = self.discussion().map(|page| page.about().asked()) {
            self.ask_about(asked);
        }
    }

    /// How much room the page being read has, in the terms its keys are
    /// measured in.
    fn discussion_room(&self) -> obelus_component::page::Room {
        let area = self.editor_area;
        obelus_component::page::Room {
            height: obelus_ui::discussion::band(area).height,
            reading: obelus_ui::discussion::reading_width(area),
        }
    }

    /// Keeps the page's window within its rows, once a frame.
    pub(in crate::app) fn settle_the_page(&mut self) {
        let room = self.discussion_room();
        if let Some(page) = self.discussion_mut() {
            let rows = page.rows(room.reading);
            page.settle(&rows, room.height);
        }
    }

    /// Offers a key to the page being read.
    pub(in crate::app) fn discussion_key(&mut self, key: &KeyEvent) -> bool {
        let room = self.discussion_room();
        let Some(page) = self.discussion_mut() else {
            return false;
        };
        match page.handle_key(key, room) {
            Outcome::Ignored => false,
            Outcome::Consumed => true,
            Outcome::Copy(text) => {
                self.copied(&text, "code");
                true
            }
            Outcome::Talk => {
                let topic = match page.about() {
                    About::PullRequest(pull) => {
                        crate::conversation::Topic::PullRequest(pull.number)
                    }
                    About::Issue(issue) => crate::conversation::Topic::Issue(issue.number),
                };
                // Refused where another Obelus has the conversation, which
                // the palette's row for this key has already said by being
                // dim.
                let _ = self.take_up(topic);
                true
            }
        }
    }

    /// Whether another Obelus has the conversation about the page being
    /// read, which is when `alt+a` does nothing here.
    #[must_use]
    pub(crate) fn page_talked_about_elsewhere(&self) -> bool {
        self.discussion().is_some_and(|page| {
            let which = match page.about() {
                About::PullRequest(pull) => obelus_agent::chats::ChatId::PullRequest(pull.number),
                About::Issue(issue) => obelus_agent::chats::ChatId::Issue(issue.number),
            };
            self.taken_up_elsewhere(&which)
        })
    }

    /// What is held on the page being read, as it is drawn.
    pub(in crate::app) fn held_on_the_page(&self) -> Option<String> {
        self.discussion()?
            .held_text(obelus_ui::discussion::reading_width(self.editor_area))
    }

    /// What the pointer did to the page being read.
    ///
    /// A press puts the cursor where it landed and takes hold of the words
    /// there, a drag takes hold of more, and a press on a heading opens or
    /// shuts what is under it -- every heading is a row Obelus drew rather
    /// than anybody's words, so a press there had nothing else to mean. A
    /// link opens on the letting go of a press that never moved.
    pub(in crate::app) fn pointer_in_discussion(
        &mut self,
        kind: crate::event::Pointer,
        x: u16,
        y: u16,
    ) {
        use crate::event::Pointer;

        let area = self.editor_area;
        let width = obelus_ui::discussion::reading_width(area);
        let link = obelus_ui::links::at(&self.pointing.links, x, y).map(str::to_string);
        let Some(page) = self.discussion() else {
            return;
        };
        let rows = page.rows(width);
        let place = obelus_ui::discussion::place_at(area, page, &rows, x, y);
        let spot = place.and_then(|place| obelus_component::page::spot_under(&rows, place));
        let folds = place
            .and_then(|place| rows.get(place.row))
            .and_then(|row| row.folds);
        let cursor = place.filter(|place| place.row < rows.len());
        let Some(page) = self.discussion_mut() else {
            return;
        };
        if matches!(kind, Pointer::Pressed | Pointer::Dragged)
            && let Some(cursor) = cursor
        {
            page.stand(cursor);
        }
        match kind {
            Pointer::Moved => {}
            Pointer::Released => {
                let clicked = spot.is_some_and(|spot| page.clicked(spot));
                page.let_go_of_nothing();
                if clicked && let Some(link) = link {
                    self.open_link(&link);
                }
            }
            Pointer::Pressed if link.is_none() && folds.is_some() => {
                if let Some(fold) = folds {
                    page.fold(fold);
                }
            }
            Pointer::Pressed => match spot {
                Some(spot) => page.hold_from(spot),
                None => page.let_go(),
            },
            Pointer::Dragged => {
                if let Some(spot) = spot {
                    page.hold_to(spot);
                }
            }
        }
    }

    /// The row in the list of what is open for a pull request or an issue.
    pub(in crate::app) fn discussion_row(index: usize, page: &DiscussionView) -> PickerItem {
        let what = match page.about() {
            About::PullRequest(_) => "Pull request",
            About::Issue(_) => "Issue",
        };
        PickerItem {
            prose: false,
            marker: None,
            // Said by `document_rows`, from `Document::icon`.
            icon: None,
            label: page.name(),
            version: None,
            detail: None,
            trailing: Some(what.to_string()),
            changed: None,
            value: PickerValue::Document(DocumentId::new(index)),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }
}
