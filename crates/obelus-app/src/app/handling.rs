//! Where an event goes, and where a key goes.

use super::*;

impl App {
    /// Reacts to one event.
    ///
    /// The order keys are offered in is fixed here rather than encoded in the
    /// key table, because it is about which component owns the state a key
    /// moves, not about which key it is. Navigation belongs to whatever holds
    /// the position it moves; commands are the named actions left over.
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            // Redrawing is unconditional after every event, so a resize needs
            // no handling of its own beyond waking the loop.
            Event::Resize => {}
            Event::Closed => self.request_quit(),
            // Nothing asked: whoever sent the signal is not at the screen
            // to answer, and is ending the process either way.
            Event::Stopped => {
                tracing::info!("told to stop");
                self.should_quit = true;
            }
            Event::Terminal(heard) => self.heard_from_a_terminal(heard),
            Event::Summoned(token) => self.summoned(token),
            Event::Remote(event) => self.remote_event(event),
            Event::Reached(number, event) => self.reached_event(number, event),
            Event::Held(number, lock) => self.held_the_remote(number, lock),
            Event::NotLetGo(number) => self.not_let_go(number),
            Event::NotHeld(number) => self.not_held(number),
            Event::Fonts { here, otherwise } => {
                tracing::info!(
                    faces = here.len(),
                    ?otherwise,
                    "the window says what it can draw with"
                );
                self.fonts_here = here;
                self.monospace_here = otherwise;
                // A list already open takes them now: the reader opened it
                // before the window had finished asking, which is the
                // ordinary case on a machine with a thousand fonts.
                let (here, otherwise) = (self.fonts_here.clone(), self.monospace_here.clone());
                if let Some((_, names)) = self.names.as_mut() {
                    names.offered(here, otherwise);
                }
            }
            Event::Watched(obelus_watch::Changed { path }) => {
                // Whether the tree has gone, asked of every change and
                // before anything else is made of it. A tree going is a
                // change to everything in it, in whatever order the kernel
                // and the debouncing leave them -- and the tree's own going
                // is not reliably among them: measured on Linux, an
                // `rm -rf` arrived as the files and directories inside it
                // and nothing for the root. Taken one at a time, the
                // project's settings file going is the reader taking the
                // project's settings away, and git's `HEAD` going is a
                // branch moving. One `stat` per change, which is a change
                // somebody made.
                if self.has_a_project() && obelus_git::is_gone(&self.working_directory) {
                    self.the_tree_has_gone();
                }
                // And from here, nothing that changed is news about the
                // project when there is no project for it to be about.
                let ours = self.has_a_project();
                // The settings, by either of their names: the watcher
                // reports whichever path the change arrived on, and a
                // change that came from a repository arrives on the file
                // the link points at rather than on the link.
                let readers = self.settled.path.as_deref().is_some_and(|config| {
                    path == config || path == obelus_config::resolved(config)
                });
                // Or the project's own, which is a change to the settings just
                // as much -- it is the layer over them. Against the file the
                // project *would* have rather than the one it has, so that the
                // file appearing is a change like any other: the ordinary
                // case is a project with no settings yet, and the moment
                // worth hearing about is the one where it gets some.
                let project = obelus_config::project_path_for(&self.working_directory);
                // Or the directory that file lives in, turning up in a
                // project that had none: the watch on the file could not be
                // taken while there was nowhere to take it, so this is
                // where it is taken. Read as well as watched, and in that
                // order -- whoever made the directory may have written the
                // file into it before the watch was attached, and a watch
                // says what happens next rather than what already has.
                let appeared = ours
                    && path.parent() == Some(self.working_directory.as_path())
                    && project.parent() == Some(path.as_path());
                if appeared {
                    self.watch_the_projects_settings();
                }
                // The directory appearing counts as the settings changing,
                // and not only because the file may be in it already: that
                // is the race this is about. Whoever made the directory
                // writes the file into it, and the two arrive together --
                // so by the time the watch is attached the file is there
                // and its own event has been and gone.
                let project = ours && (path == project || appeared);
                if readers || project {
                    self.reread_config();
                } else if self.is_a_theme(&path) {
                    // The colours the reader is already wearing, read again:
                    // the name in the settings has not moved, and what it
                    // stands for has.
                    self.reread_theme();
                } else if ours && self.is_a_window(&path) {
                    // Another window on the repository opened, closed, or
                    // moved to another tree -- which the list of worktrees
                    // draws while it is up.
                    self.reread_the_windows();
                } else if self.is_the_remote_wanted(&path) {
                    // Another window asking for the chat this one has.
                    self.somebody_wants_the_remote();
                } else if self.is_the_relays_door(&path) {
                    // A window that has the chat saying where it is.
                    self.the_door_moved();
                } else if ours && self.is_a_claim(&path) {
                    // A conversation taken up or let go in another window
                    // -- including one let go by that window dying, which
                    // is a file closed by a writer and nothing else.
                    self.reread_who_holds_what();
                } else if ours && self.is_the_sessions_file(&path) {
                    // Which of the project's notes has a conversation,
                    // written by another Obelus -- or by this one, which
                    // hears its own writes like anybody else's and has
                    // already kept what it wrote. Reading it again costs
                    // one parse and keeps the two windows in step. And the
                    // list of conversations reads it too, for the name each
                    // goes by.
                    self.reread_the_sessions();
                    self.say_the_new_name();
                } else if ours && self.is_the_notes_file(&path) {
                    // What the project means to come back to, written by
                    // another Obelus, the reader's own editor -- or by this
                    // Obelus, which hears its own writes like anybody
                    // else's. Not told apart, because there is nothing to
                    // gain by it: a reread keeps the box the reader is
                    // typing in and puts the caret back by name, so reading
                    // back what Obelus itself just wrote changes nothing on
                    // the page.
                    self.reread_notes();
                    // And the copy the conversation's box is offered
                    // from, which is wanted whether or not that page is
                    // open: a reader talking about a note has usually
                    // walked away from the list of them.
                    self.reread_the_notes_kept();
                } else if ours && obelus_git::state_moved(&path) {
                    self.forget_what_git_said();
                } else {
                    self.reload_path(&path);
                }
                // And the servers, whatever it was: a file changing on
                // disk is news to them as much as to Obelus -- a branch
                // checked out, a build script's output, an editor
                // somewhere else. Some of them watch for themselves and
                // will have heard already; the protocol's own answer is
                // that the client says so, and a server that relies on it
                // is otherwise answering about a file nobody has.
                self.told_servers_about(&path);
            }
            Event::Lsp(obelus_lsp::Message { language, message }) => {
                let Some(client) = self.servers.get_mut(&language) else {
                    return;
                };
                // Whether it had finished its handshake before this
                // message, because finishing one is a moment Obelus has to
                // act on: every standing question about an open file is
                // refused while a server cannot say what it answers, and
                // opening a file is the moment they are all asked.
                let handshaken = client.capabilities().is_some();
                // And whether it was busy, because stopping is the other
                // moment worth acting on: a server that has not finished
                // reading the project answers what it can, which for the
                // questions below is nothing at all -- measured against
                // rust-analyzer, an empty list a second after the
                // handshake, and nothing asking again for as long as the
                // reader sits still.
                let working = client.working_on().is_some();
                // Everything the protocol needs rather than Obelus — the
                // handshake, progress, the server's own log lines — is dealt
                // with in there.
                let reply = client.on_message(&message);
                // Unasked-for news about a file, which arrives on the same
                // pipe as the answers and belongs to nobody's question.
                let published = client.take_published();
                // And what it says went wrong, which is the one thing it
                // says that belongs on the reader's row rather than in the
                // log: the rest of its talk is progress, and the badge
                // says that by turning.
                let complaints = client.take_complaints();
                // And the edits it wants made, which arrive the same way
                // and are answered by making them.
                let asked = client.take_asked_edits();
                for params in published {
                    self.on_published(language, &params);
                }
                // Named, because the row says nothing else about who is
                // complaining -- and left in the words it arrived in,
                // which are the server's and not Obelus's to rewrite.
                if let Some(said) = complaints.last() {
                    let name = obelus_lsp::command_for(language).unwrap_or(language.name());
                    tracing::warn!(language = language.name(), "{said}");
                    self.wrong(format!("{name}: {said}"));
                }
                for edit in &asked {
                    self.on_asked_edit(language, edit);
                }
                if let Some(reply) = reply {
                    self.on_reply(language, reply);
                }
                // And now it can say what it answers. Without this a file
                // opened before its server was ready is a file nothing is
                // ever asked about: the questions were all refused, and the
                // next thing that asks them is the reader saving.
                let now = self.servers.get(&language);
                let ready = now.is_some_and(|client| client.capabilities().is_some());
                let busy = now.is_some_and(|client| client.working_on().is_some());
                if ready && (!handshaken || (working && !busy)) {
                    self.ask_about_open_files(language);
                }
            }
            Event::Counted(counted) => self.on_counted(*counted),
            Event::Shifted(held) => self.shifted = held,
            Event::Scroll(rows) => self.scroll(rows),
            Event::Pointer { kind, x, y } => self.on_pointer(kind, x, y),
            // One change for the whole of it, so undoing a paste is one
            // step rather than however many lines it happened to be.
            Event::Paste(text) => self.paste_text(&text),
            Event::Dropped(path) => self.dropped(&path),
            // A frame of the one thing moving, and nothing else: what is
            // owed at a moment is owed on a machine with nothing animated
            // on it, and each of the three below says when it wants asking.
            Event::Tick => {
                self.phase = self.phase.wrapping_add(1);
                self.drag_on();
            }
            // The notes, once the reader has stopped typing into them.
            Event::NotesSettled => self.settle_notes(),
            Event::SyntaxSettled => {
                // Let go of first: `catch_up_soon` starts another only when
                // there is none, and one held after it has fired is a tree
                // that never gets a second chance.
                self.syntax_pause = None;
                self.settle_syntax();
            }
            // The rename's own clock is inside the wait it belongs to, so
            // there is nothing to let go of here: the wait ending drops it.
            Event::ChangesSettled => self.settle_changes(),
            Event::SignatureSettled => self.ask_signature_again(),
            // The rest asking what is under it. `settle_hover` is still
            // asked every frame, because the rest of what it does is
            // letting go of an answer the pointer has moved off -- that is
            // about where the pointer is now, not about a moment passing.
            Event::PointerRested => self.settle_hover(),
            Event::RenameOverdue => self.rename_without_them(),
            Event::Search(obelus_search::Event::Matches {
                generation,
                hits,
                done,
            }) => self.on_matches(generation, hits, done),
            Event::Agent(obelus_agent::Event::Acp(message)) => self.on_acp(message),
            // Only the running connection's. A word from one that has been
            // stopped -- what it had said before and nobody had read yet,
            // and that it has gone -- is about a process that is not the
            // one running, and read as the running one's it matched an old
            // answer to a new request and had a live agent taken for dead.
            Event::Agent(obelus_agent::Event::Heard { from, incoming }) => {
                match self
                    .talker
                    .as_ref()
                    .map(obelus_agent::acp::Talk::connection)
                {
                    Some(running) if running == from => self.on_acp(incoming),
                    _ => tracing::debug!(
                        from,
                        "a word from a connection that is not the one running"
                    ),
                }
            }
            // Only what was asked of this tree: the server about another
            // went with the project that was on it.
            Event::Tools(obelus_mcp::Asked { root, .. }) if root != self.working_directory => {
                tracing::info!(root = %root.display(), "a tool asked of a tree this window has left");
            }
            Event::Tools(obelus_mcp::Asked { wanted, answer, .. }) => {
                let _ = answer.send(match wanted {
                    obelus_mcp::Wanted::Notes(doing) => self.change_the_notes(doing),
                    obelus_mcp::Wanted::Open { path, line } => self.open_for_an_agent(&path, line),
                    obelus_mcp::Wanted::Workflow => self.workflow_for_an_agent(),
                    obelus_mcp::Wanted::Close { conversation } => {
                        self.close_for_an_agent(conversation)
                    }
                });
            }
            Event::Released(tag) => self.on_released(&tag),
            Event::PullRequests(answer) => self.on_pull_requests(answer),
            Event::Issues(answer) => self.on_issues(answer),
            Event::PullRequestQuerySettled => self.on_the_query_settled(),
            Event::PullRequestDiscussion { number, answer } => {
                self.on_pull_request_discussion(number, answer);
            }
            Event::Agent(obelus_agent::Event::Registry { agents, failure }) => {
                self.on_registry(agents, failure)
            }
            Event::Agent(obelus_agent::Event::Icon { id, svg }) => self.on_icon(id, svg),
            Event::Agent(obelus_agent::Event::Installing { id, progress }) => {
                self.on_installing(id, progress)
            }
            Event::Agent(obelus_agent::Event::Installed { id, failure }) => {
                self.on_installed(id, failure)
            }
            Event::Git(obelus_git::Event::Blamed { path, at, lines }) => {
                // Kept whether or not the reader is still looking at that
                // file: they walked away from it while a walk of its history
                // was running, and they will walk back.
                self.asking_blame.remove(&(path.clone(), at));
                self.blames.insert((path, at), lines);
            }
            Event::Git(obelus_git::Event::Logged {
                generation,
                commits,
                walked,
                done,
            }) => {
                // A batch from a walk whose list is gone, or from one
                // superseded by another tab, another file, another key.
                if self.history_generation.is_current(generation) {
                    self.on_logged(commits, walked, done);
                }
            }
            Event::Scanned(scanned) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.scan_arrived(*scanned);
                }
            }
            Event::Reopened { tree, files } => self.take_up_what_was_open(&tree, files),
            Event::Search(obelus_search::Event::FilesFound {
                generation,
                paths,
                ignored,
            }) => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if !self.walk_generation.is_current(generation) {
                    return;
                }
                // Kept as well as shown. A file list is a tree while
                // nothing is typed and these rows the moment something is,
                // and a reader who types, clears and types again would
                // otherwise wait for a fresh walk each time -- which is a
                // walk per first keystroke rather than one per opening.
                self.found
                    .extend(paths.iter().map(|path| (path.clone(), ignored)));
                // Drawn only where the flat listing is what is showing: on
                // the tab these rows are about, with something typed. A
                // batch arriving while the tree is up would mix a walk of
                // the whole project into the branch the reader has open,
                // and one arriving on the changed tab is the other tab's
                // answer.
                if !self.showing_found() {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    let statuses = &self.statuses;
                    let root = &self.working_directory;
                    picker.extend(paths.into_iter().map(|path| {
                        PickerItem {
                            prose: false,
                            marker: None,
                            icon: Some(obelus_icons::for_path(&path)),
                            label: path.display().to_string(),
                            version: None,
                            detail: None,
                            trailing: None,
                            changed: None,
                            value: PickerValue::File(path.clone()),
                            enabled: true,
                            colours: None,
                            // Git says nothing about a file it was told to
                            // ignore -- `git status` leaves them out -- so the
                            // walk that went looking is what says it.
                            status: match ignored {
                                true => Some(obelus_git::FileStatus::Ignored),
                                false => statuses
                                    .get(&root.join(&path))
                                    .map(|standing| standing.status),
                            },
                            depth: 0,
                            opens: None,
                            kind: None,
                            tab: None,
                            section: None,
                        }
                    }));
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Discards releases once, here, so nothing further down has to
        // remember to.
        if KeyChord::from_event(&key).is_none() {
            return;
        }
        // Whatever Obelus had to say has been read by now, or was not going to
        // be.
        self.quiet();
        // And a drag is over. Mostly it ended with the button coming up,
        // but a pointer that leaves the terminal takes its release with
        // it, and a drag nothing ever ended would go on scrolling under
        // whatever the reader did next.
        self.dragging = None;

        // Whatever is in front, and on down only as far as each thing lets
        // a key through -- which for anything the reader is *in* is not at
        // all (`app/hearing`). What nobody took goes to the key table.
        if self.hand_over(&key) {
            return;
        }

        // A key whose command cannot do its job here does nothing at all.
        // The palette draws such a row dim and refuses to run it; a key is
        // the same row reached another way, and one judgement -- `offers`
        // -- has to answer for both, or a command is off in one place and
        // live in the other. Silence is the answer because the reader has
        // the palette to find out why: `f3` on a tree with nothing changed
        // used to open a list of nothing and say so on the status row,
        // which is a sentence nobody asked for.
        //
        // Except for a key that opens a view or a list over the file, from
        // inside a view or from a list the reader opened: that goes to what
        // it opens, in place of this one, rather than being refused because a
        // dialog is showing (`app/switching`). What a key means in
        // a file is what it means here -- the table is asked as though the
        // file were what is showing -- and only those keys are let through,
        // so nothing opens over anything.
        //
        // Not enter, however it is held. Every list and page takes enter
        // itself, which is why it is never bound (`keymap::why_not`), and
        // with a modifier it is still that list's key: `alt+enter` is "go
        // there" in a history, and in the search, which has no use for it,
        // it was the menu about the name under the caret in the file behind
        // -- the search thrown away for a key nobody meant to leave it by.
        //
        // Unless the view showing has bound that key itself, which is a
        // view saying what the key means *here* -- and that beats what it
        // means one level out, the same way `Keymap::lookup` asks a
        // context's own table before the file's and the file's before
        // everywhere. `f4` was the case while it opened a conversation and
        // meant "which one" inside one, and it swapped the conversation for
        // itself; it is the list everywhere now, and this stays for the
        // next view that takes a key of its own.
        if self.gives_way_to_a_view()
            && key.code != KeyCode::Enter
            && self.keymap.bound_here(&key, self.context()).is_none()
            && let Some(command) = self.keymap.lookup(&key, Context::Normal)
            && command.takes_a_view_s_place()
        {
            self.switch_view(command);
            return;
        }
        if let Some(command) = self.keymap.lookup(&key, self.context())
            && self.offers(command)
        {
            dispatch::dispatch(self, command);
        }
    }
}
