//! A pull request or an issue, opened from the list and read as a page.
//!
//! **Choosing one opens it; talking about it is a key of the page.** The
//! list's enter used to open the conversation that reviews a pull request,
//! claimed before there was a session -- so reading one meant starting a
//! review of it, and a row another Obelus was reviewing could not be read
//! at all. A page is read, not claimed: any window may have one open, and
//! the claim is taken where the conversation is, by `alt+a` on the page.
//! And `alt+a` in that conversation is the way back, opening the page again
//! where the reader closed it: one key between the two, as a reader who
//! went from one to the other expects of it.
//!
//! **A file it changes is its last commit's, from here or from GitHub.**
//! Not the file on disk, which belongs to whichever branch is checked out:
//! the commit's version, read from the reader's own repository where the
//! commit is there, and asked of GitHub where it is not -- because Obelus
//! does not fetch. Nothing is laid beside it that is a walk of the reader's
//! history ([`obelus_buffer::Content::PullRequest`]).
//!
//! **A page can be opened by its number alone.** The list is where one is
//! chosen, but a page left open when Obelus last stopped comes back, and a
//! conversation outlives its page -- neither with a list asked. So `gh` says
//! what it is called with the rest of what it says about it, and until it
//! has, the page is called by its number.
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
    pub(in crate::app) fn open_the_discussion(&mut self, asked: Asked) {
        let at = self.page_about(asked);
        let from = self.here();
        self.record(from);
        self.make_room(Room::Region);
        self.go_to_document(at);
    }

    /// The page about one, put in the list of what is open where it is not
    /// there yet -- without going to it.
    ///
    /// From the list's row where the list has one, and by its number alone
    /// where it has not: a page opened again after a restart, or from its
    /// conversation, is told what it is called by `gh` with the rest.
    pub(in crate::app) fn page_about(&mut self, asked: Asked) -> DocumentId {
        let number = asked.number();
        let open = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::discussion)
                .is_some_and(|page| page.number() == number)
        });
        let about = self.about(asked);
        if let Some(at) = open {
            // What the list says it is now, which choosing its row again is
            // the moment to hear.
            if let Some(about) = about
                && let Some(page) = self
                    .documents
                    .get_mut(at)
                    .and_then(|document| document.as_mut().and_then(Document::discussion_mut))
            {
                page.listed_as(about);
            }
            return DocumentId::new(at);
        }
        let mut page = match about {
            Some(about) => DiscussionView::new(about),
            None => DiscussionView::asking(asked),
        };
        // What the preview was shown, at once: the reader chose the row
        // they had just been reading.
        if let Some(said) = self.pulls.discussions.get(&number) {
            page.told(said.clone());
        } else if let Some(why) = self.pulls.refused.get(&number) {
            page.refused(why.clone());
        }
        self.documents.push(Some(page.into()));
        DocumentId::new(self.documents.len() - 1)
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
                Some((at, self.about(page.asked())?))
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
        if let Some(asked) = self.discussion().map(DiscussionView::asked) {
            self.ask_about(asked);
        }
    }

    /// How much room the page being read has, in the terms its keys are
    /// measured in.
    pub(in crate::app) fn discussion_room(&self) -> obelus_component::page::Room {
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
            Outcome::Open(to) => {
                self.open_link(&to);
                true
            }
            Outcome::OpenFile { path, line } => {
                self.open_the_pull_requests_file(&path, line);
                true
            }
            Outcome::Talk => {
                let topic = match page.asked() {
                    Asked::PullRequest(number) => crate::conversation::Topic::PullRequest(number),
                    Asked::Issue(number) => crate::conversation::Topic::Issue(number),
                };
                // Refused where another Obelus has the conversation, which
                // the palette's row for this key has already said by being
                // dim.
                let _ = self.take_up(topic);
                true
            }
        }
    }

    /// The pull request or issue the conversation being read is about,
    /// whose page `alt+a` there goes to.
    #[must_use]
    pub(crate) fn page_of_the_conversation(&self) -> Option<Asked> {
        match self.conversation()?.topic {
            crate::conversation::Topic::PullRequest(number) => Some(Asked::PullRequest(number)),
            crate::conversation::Topic::Issue(number) => Some(Asked::Issue(number)),
            crate::conversation::Topic::Loose | crate::conversation::Topic::Note(_) => None,
        }
    }

    /// Whether another Obelus has the conversation about the page being
    /// read, which is when `alt+a` does nothing here.
    #[must_use]
    pub(crate) fn page_talked_about_elsewhere(&self) -> bool {
        self.discussion().is_some_and(|page| {
            let which = match page.asked() {
                Asked::PullRequest(number) => obelus_agent::chats::ChatId::PullRequest(number),
                Asked::Issue(number) => obelus_agent::chats::ChatId::Issue(number),
            };
            self.taken_up_elsewhere(&which)
        })
    }

    /// Opens one of the files the pull request being read changes, as its
    /// last commit has it.
    ///
    /// From the reader's own repository where the commit is there -- their
    /// own branch, or one they fetched -- and from GitHub where it is not,
    /// on a thread, because Obelus does not fetch. Not the file on disk:
    /// that is a different document that happens to share a name, the
    /// branch it belongs to is very often not the one checked out.
    ///
    /// On `line`, counted from one, where the reader asked about one: a
    /// review's thread is about the lines it was said beside.
    fn open_the_pull_requests_file(&mut self, path: &str, line: Option<u64>) {
        let Some((number, sha)) = self.discussion().and_then(|page| match page.about()? {
            About::PullRequest(pull) => Some((pull.number, pull.sha.clone())),
            About::Issue(_) => None,
        }) else {
            return;
        };
        let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes()) else {
            self.wrong("GitHub has not said which commit it is at");
            return;
        };
        let full = self.working_directory.join(path);
        let open = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::file)
                .is_some_and(|buffer| {
                    buffer.path() == full && buffer.content().is_of_pull_request(number, id)
                })
        });
        if let Some(at) = open {
            let at = DocumentId::new(at);
            let from = self.here();
            self.record(from);
            self.stand_on(at, line);
            self.go_to_file(at);
            // Going back to one GitHub refused is asking again.
            let refused = self.file(at).is_some_and(|buffer| {
                matches!(
                    buffer.content().fetched(),
                    Some(obelus_buffer::Fetched::Refused(_))
                )
            });
            if !refused {
                return;
            }
            if let Some(buffer) = self.file_mut(at) {
                buffer.asked_again();
            }
        } else if let Some(text) = obelus_git::history::text_at(&self.working_directory, id, &full)
        {
            self.put_up_the_pull_requests_file(number, id, &full, &text, line);
            return;
        } else {
            // Open at once, empty and saying it is on its way, rather than
            // a second after the key with nothing in between.
            let buffer = obelus_buffer::Buffer::on_its_way(&full, number, id);
            self.documents.push(Some(Document::from(buffer)));
            let at = DocumentId::new(self.documents.len() - 1);
            let from = self.here();
            self.record(from);
            self.go_to_file(at);
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        let root = self.working_directory.clone();
        let instead = self.gh_instead();
        let asked = path.to_string();
        obelus_runtime::handle().spawn_blocking(move || {
            let answer = obelus_github::file_at(&root, instead, &asked, &sha);
            let _ = sender.send(crate::event::Event::PullRequestFile {
                number,
                id,
                path: full,
                line,
                answer,
            });
        });
    }

    /// Takes a file GitHub was asked for: up on the screen where the reader
    /// is still on the page that asked, and in the list of what is open
    /// where they have gone somewhere since.
    pub(in crate::app) fn on_pull_request_file(
        &mut self,
        number: u64,
        id: gix::ObjectId,
        path: &std::path::Path,
        line: Option<u64>,
        answer: Result<Vec<u8>, crate::app::pulls::Unlisted>,
    ) {
        let named = path
            .strip_prefix(&self.working_directory)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        // The document that is waiting for it, which the reader may have
        // closed since: then nothing is.
        let Some(at) = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::file)
                .is_some_and(|buffer| {
                    buffer.path() == path && buffer.content().is_of_pull_request(number, id)
                })
        }) else {
            return;
        };
        let at = DocumentId::new(at);
        let read = match answer {
            Ok(bytes) => obelus_encoding::read(&bytes)
                .map(obelus_encoding::Read::into_owned)
                .ok_or_else(|| format!("Not text: {named}")),
            Err(why) => Err(obelus_component::discussion::unlisted(&why)),
        };
        match read {
            Ok(read) => {
                if let Some(buffer) = self.file_mut(at) {
                    buffer.arrived(read);
                }
                self.stand_on(at, line);
            }
            Err(why) => {
                if let Some(buffer) = self.file_mut(at) {
                    buffer.refused(why);
                }
            }
        }
    }

    /// Puts a pull request's file in the list of what is open, and goes to
    /// it.
    fn put_up_the_pull_requests_file(
        &mut self,
        number: u64,
        id: gix::ObjectId,
        path: &std::path::Path,
        text: &str,
        line: Option<u64>,
    ) {
        let buffer = obelus_buffer::Buffer::at_pull_request(path, number, id, text);
        self.documents.push(Some(Document::from(buffer)));
        let at = DocumentId::new(self.documents.len() - 1);
        self.stand_on(at, line);
        let from = self.here();
        self.record(from);
        self.go_to_file(at);
    }

    /// Puts the caret of a file that is open on `line`, counted from one.
    fn stand_on(&mut self, at: DocumentId, line: Option<u64>) {
        let Some(line) = line.and_then(|line| usize::try_from(line).ok()) else {
            return;
        };
        if let Some(buffer) = self.file_mut(at) {
            buffer.place_cursor(
                obelus_text::coordinates::LineNumber::new(line.saturating_sub(1)),
                obelus_text::coordinates::CharColumn::new(0),
            );
        }
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
        // A file's name is drawn as a link and goes to the file, here, not
        // to the web: the link is the row's own `opens`.
        let file = place
            .and_then(|place| rows.get(place.row))
            .and_then(|row| row.opens.clone())
            .filter(|opens| link.as_deref() == Some(opens.path.as_str()));
        let cursor = place.filter(|place| place.row < rows.len());
        // Whether the row is the page's words: a row Obelus wrote has none,
        // and `spot_under` answers for it with the nearest words there are --
        // the same answer for every such row, so it cannot say where a press
        // on one went down.
        let in_words = cursor
            .and_then(|cursor| rows.get(cursor.row))
            .is_some_and(|row| row.from.is_some());
        let Some(page) = self.discussion_mut() else {
            return;
        };
        if matches!(kind, Pointer::Pressed | Pointer::Dragged)
            && let Some(cursor) = cursor
        {
            page.stand(cursor);
            match kind {
                Pointer::Pressed => page.press_at(cursor),
                _ => page.drag_over(cursor),
            }
        }
        match kind {
            Pointer::Moved => {}
            Pointer::Released => {
                // In the page's words, a click is a hold that never grew;
                // on a row Obelus wrote -- a check's name -- nothing is
                // held, so it is the pointer letting go where it went down.
                let clicked = match (in_words, spot) {
                    (true, Some(spot)) => page.clicked(spot),
                    _ => cursor.is_some_and(|cursor| page.pressed_here(cursor)),
                };
                page.let_go_of_nothing();
                match (clicked, file, link) {
                    (true, Some(file), _) => {
                        self.open_the_pull_requests_file(&file.path, file.line);
                    }
                    (true, None, Some(link)) => self.open_link(&link),
                    _ => {}
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
        let what = match page.asked() {
            Asked::PullRequest(_) => "Pull request",
            Asked::Issue(_) => "Issue",
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
