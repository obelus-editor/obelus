//! Working in a project: starting, watching it, and letting go of it
//! when it goes.

use super::*;

impl App {
    /// Puts the application on a project.
    ///
    /// Before the settings are read, always: a project has settings of its
    /// own and a theme beside them, and finding those means knowing which
    /// project first. [`App::load_config`] lays the project's answers over the
    /// reader's at the end, so this only has to have happened by then.
    pub fn work_in(&mut self, root: PathBuf) {
        self.working_directory = root;
        // Now, rather than on the first frame: the row draws the branch
        // and a watch says what happens next rather than what already
        // has.
        self.head = obelus_git::head_of_the_tree(&self.working_directory);
        // The one door every way of settling on a project goes through --
        // an argument, the directory Obelus was started in, a row on the
        // page that asks which -- which is why the remembering is here
        // and at none of the three. `remember` declines anything that is
        // not a worktree, so a process that began in the home directory
        // writes nothing.
        projects::remember(&self.working_directory, jiff::Timestamp::now().as_second());
        let root = self.working_directory.clone();
        self.keep_what_is_open_for(&root);
    }

    /// Which branch the tree Obelus was put on has checked out.
    #[must_use]
    pub fn head(&self) -> Option<&obelus_git::Head> {
        self.head.as_ref()
    }

    /// Whether there is a project to do anything in.
    ///
    /// Not while Obelus is still asking which one, and not once
    /// the one it was has gone. Both are known without doing any work,
    /// which is what a requirement has to be.
    #[must_use]
    pub fn has_a_project(&self) -> bool {
        self.which_project.chooser.is_none() && !self.gone
    }

    /// Whether the tree Obelus was put on has gone from disk.
    #[must_use]
    pub const fn tree_has_gone(&self) -> bool {
        self.gone
    }

    /// The tree has gone from under this window.
    ///
    /// Heard from the watcher rather than looked for: the tree going is
    /// its contents going, which are changes like any other, and every
    /// change is asked first whether the tree is still there. Nothing is
    /// polled, which leaves the platforms
    /// whose watcher does not report a watched directory going with the
    /// half that does not depend on it -- what keeps the project's things
    /// from being written into a project that has gone is asked of the disk
    /// at the moment of writing (`obelus_git::project`).
    ///
    /// Said over the whole screen, on top of whatever the reader was in
    /// (`ui::gone`), and answered with one of two keys: enter asks which
    /// project next, and the key that leaves leaves. Everything else that
    /// was over the page goes first, because the page covers the screen
    /// and a page covers what shares its room.
    pub(super) fn the_tree_has_gone(&mut self) {
        tracing::warn!(tree = %self.working_directory.display(), "the tree Obelus is on has gone");
        if self.give_up_unseen(format!(
            "The tree Obelus was on has gone: {}",
            self.working_directory.display()
        )) {
            return;
        }
        self.make_room(layers::Room::Screen);
        self.gone = true;
        self.head = None;
        self.worktrees.left_the_tree();
    }

    /// The page saying the tree has gone, answered -- or not, and then
    /// nothing else hears the key either: what is under the page is about
    /// a project that is not there.
    pub(super) fn the_page_saying_it_has_gone(&mut self, key: &KeyEvent) -> bool {
        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE {
            self.let_go_of_the_project();
            self.ask_which_project();
        } else if self.keymap.lookup(key, Context::Dialog) == Some(Command::Quit) {
            self.request_quit();
        }
        true
    }

    /// Lets go of everything the project that went was.
    ///
    /// What was open is closed without asking, unsaved work and all -- a
    /// file in a tree that has gone has nowhere to be written, and Obelus
    /// does not make the tree again to write it. Going to another worktree
    /// lets go the same way, and asks about what is unwritten before it
    /// gets here (`App::go_to_worktree`).
    ///
    /// **A window that starts again, without starting again.** What is
    /// kept is what belongs to the process and not to the project -- the
    /// loop's channel, the reader's settings, what the front end can do --
    /// and everything else is a new [`App`]'s. Kept by name rather than
    /// cleared by name, so that a field nobody thought of here is one that
    /// starts empty, and not one still holding the last project's answer.
    pub(super) fn let_go_of_the_project(&mut self) {
        // The sessions nothing was said in, as on the way out: an agent
        // keeps what it is not told to let go of.
        self.let_go_of_what_nothing_was_said_in(None);
        // Moved on rather than made again, because a walk still running
        // holds the old count: a new one would start where the old one's
        // answers are numbered, and they would arrive as current.
        self.files.walk_generation.next();
        self.history_generation.next();
        self.search.search_generation.next();
        self.worktrees.not_showing();

        let was = std::mem::replace(self, Self::new(Vec::new()));
        self.events = was.events;
        self.drawing = was.drawing;
        self.fonts_here = was.fonts_here;
        self.monospace_here = was.monospace_here;
        self.screen_area = was.screen_area;
        self.editor_area = was.editor_area;
        self.settled = was.settled;
        self.keymap = was.keymap;
        self.theme = was.theme;
        self.theme_name = was.theme_name;
        self.files.walk_generation = was.files.walk_generation;
        self.history_generation = was.history_generation;
        self.search.search_generation = was.search.search_generation;
        // The door other windows reach this one by is the process's, and
        // listens for as long as it runs.
        self.worktrees = was.worktrees;
        self.agents = was.agents;
        self.releases = was.releases;
        self.search.looking = was.search.looking;
        self.search.outside = was.search.outside;
        // What Obelus could not make of its own files, which are not the
        // project's: the reader's settings, which nothing reads again here.
        // Obelus's own marks and none of a server's -- the server that said
        // those has gone with the project, and nothing would ever take what
        // it said away.
        let root = was.working_directory;
        self.troubles = was
            .troubles
            .into_iter()
            .filter(|(path, _)| !path.starts_with(&root))
            .filter_map(|(path, troubles)| {
                let ours: Vec<_> = troubles
                    .into_iter()
                    .filter(|trouble| trouble.source.as_deref() == Some(semantics::OBELUS))
                    .collect();
                (!ours.is_empty()).then_some((path, ours))
            })
            .collect();
        self.working_directory = root;
        // A watcher of its own as well, on what is left -- the settings and
        // the theme. Started again rather than kept and given things back
        // one at a time: a watch is a count on the watcher it was taken on,
        // and what this one held for the project and its files is a list
        // nothing here has.
        if was.watcher.is_some()
            && let Some(events) = self.events.clone()
        {
            self.start_watching(events);
        }
        // And the rest of `was` goes at the end of this: the servers, the
        // agent, what it was running and the tools it was offered, all of
        // which stop as they are dropped.
        //
        // The reader's settings without the project's over them, which
        // are in a file that is not there.
        self.apply_project();
    }

    /// Starts everything that needs the loop's channel.
    ///
    /// Best effort throughout: a watcher that will not start, or a language
    /// server that is not installed, is logged and then done without.
    /// Refusing to run because a convenience is missing would trade it for a
    /// missing program.
    ///
    /// Public because it is the whole of what starting means, and a test
    /// about what Obelus does on the way up has nothing else to call.
    pub fn start(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
        // First, before anything is started that compiles: an agent, a
        // server or a terminal is told where the pool is when it starts,
        // and not after -- the servers below are started a few lines down.
        self.settle_the_pool();
        // Before anything else is started: the reader is looking at an empty
        // screen until it arrives.
        self.send_the_reopening();
        self.start_watching(sender);
        // Both of these are about the project, and on a start with
        // nothing to go on there is not one yet: they would be rooted at
        // the directory the process happened to begin in, which from a
        // desktop launcher is the home directory, and nothing would move
        // them when the reader answered. `settle_on` does them then.
        if self.which_project.chooser.is_none() {
            self.offer_the_tools();
            self.watch_the_project();
            self.say_where_this_window_is();
        }
        for index in 0..self.documents.len() {
            self.serve(index);
        }
        // Not about the project: whether a newer Obelus is out is the
        // same question wherever this one was started, so it is asked
        // whether or not the reader has said where they work.
        self.ask_about_releases();
        // Before what went wrong, which a chat that is not there to
        // connect to is one of. And not before there is a project, like
        // the tools above: a conversation begun from the chat would be
        // rooted at wherever the process began. `settle_on` connects.
        if self.remote_at_start && self.which_project.chooser.is_none() {
            self.connect_remote_at_start(self.reading_nothing());
        }
        // What went wrong on the way up, over whatever the first screen is,
        // and last of all so that everything that could go wrong has.
        let told = self.tell_what_went_wrong();
        // Last, and here rather than at the command line: the rows come
        // from a walk that sends on this channel, so a list opened before
        // there was one would be a list nothing ever fills. Not over what
        // went wrong, though: a list opened over a list would put the one
        // the reader is owed under the one they asked for, and the files
        // are one key away once it has been read.
        //
        // Nor where nobody is at the screen to choose from it: the walk
        // that fills it is the whole project.
        if self.list_at_start && !told && !self.headless {
            self.open_file_picker();
        }
    }

    /// Offers an agent Obelus's own tools, rooted at this project.
    ///
    /// Started with the loop rather than with the first agent, because
    /// the address is what an agent is told and telling two of them two
    /// addresses would be two servers -- but not before there is a
    /// project, because the root is the whole of what the tools are
    /// about.
    pub(super) fn offer_the_tools(&mut self) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        match obelus_mcp::serve(&self.working_directory, std::sync::Arc::new(sender)) {
            // Said, because the silent half of this is the half nobody can
            // ask about: whether an agent was offered anything, and whether
            // it took it, were both questions Obelus had no answer to.
            Ok((url, listening)) => {
                tracing::info!(url, "Obelus is offering an agent its tools");
                self.tools_url = Some(url);
                self.listening = Some(listening);
            }
            Err(error) => {
                // Not a reason to stop: an Obelus that cannot listen is an
                // Obelus an agent cannot ask anything of, which is what it
                // was until now.
                tracing::warn!(%error, "Obelus is offering an agent nothing");
                self.amiss
                    .push("An agent asking Obelus for its tools reaches nothing".to_string());
            }
        }
    }

    /// Takes the watch on the project's own settings, where there is now
    /// somewhere to take it.
    ///
    /// Asked twice: once while the watches are being set up, and again if
    /// the directory turns up later. Idempotent, because the watcher counts
    /// watches by directory and a second ask for one it already holds is
    /// the count going up -- which is also what makes the project's root
    /// and a file of the reader's that happens to live in it one watch
    /// rather than two.
    pub(super) fn watch_the_projects_settings(&mut self) {
        let project = obelus_config::project_path_for(&self.working_directory);
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        if let Err(error) = watcher.watch(&project) {
            tracing::debug!(%error, path = %project.display(), "still nothing to watch");
        }
    }

    /// Takes the watches that are about the project, and nothing else.
    ///
    /// Its own piece because the project is not always known when Obelus
    /// starts: a start with nothing to go on asks which one, and these
    /// would otherwise all be taken against the directory the process
    /// happened to begin in -- the home directory, from a desktop
    /// launcher. Taken when the reader answers instead, which is
    /// `App::settle_on`.
    ///
    /// The one that cost most by being wrong is git's: with `HEAD` and
    /// `index` unwatched, `forget_what_git_said` never fires, so the
    /// branch on the status row and the marks in the margin are whatever
    /// they were when the project opened for the rest of the session.
    pub(super) fn watch_the_project(&mut self) {
        let root = self.working_directory.clone();
        let project = obelus_config::project_path_for(&root);
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        // What git keeps its state in, because Obelus is not the only
        // thing in the repository: a commit in another window, or in a
        // shell, changes what has changed in every file on screen. The
        // margin would otherwise go on showing a diff against a commit
        // that is no longer the one the file is against.
        for path in obelus_git::state_of(&root) {
            if let Err(error) = watcher.watch(&path) {
                tracing::warn!(%error, path = %path.display(), "not watching the repository");
            }
        }
        // And the project's own settings, for the same reason twice over:
        // another Obelus on this project may be looking at them, and a `git
        // pull` rewrites them under everybody.
        // The file the project *would* have, not the one it has: watching only
        // what was there at startup is the "read once" mistake with a longer
        // fuse, because it looks right until somebody creates the file --
        // the window next door writing the project's first setting, or a
        // pull bringing one.
        // The project itself, for the directory its settings live in
        // coming or going. Always, not only where it is missing now: a
        // reader who deletes `.obelus` and makes it again is the same
        // question as one who never had it, and a watch taken only in the
        // second case left the first unheard for the rest of the session.
        //
        // Not recursive -- what is wanted is one directory appearing
        // directly in the project, and a recursive watch on a repository is
        // `target` and `.git` reported a thousand times over. Where the
        // reader has a file of the project's root open, this is that same
        // watch counted twice rather than a second one.
        if let Err(error) = watcher.watch_directory(&root) {
            tracing::warn!(
                %error,
                path = %root.display(),
                "not watching the project for settings appearing"
            );
        }
        if let Err(error) = watcher.watch(&project) {
            // A project with no settings of its own has no directory to
            // watch, and that is the ordinary case: a quarter of the starts
            // in this machine's own log said so, every one of them about a
            // project that was working perfectly. A warning on every start
            // is how a log stops being read -- the same argument the list of
            // what went wrong on the way up is built on, and the level
            // `settle_a_watch` already uses for the same failure.
            //
            // A directory that *is* there and will not be watched is a real
            // failure and keeps its warning: settings changed in another
            // window will not arrive, and that is worth a word.
            match project.parent().is_some_and(std::path::Path::is_dir) {
                true => tracing::warn!(
                    %error,
                    path = %project.display(),
                    "not watching the project's settings"
                ),
                // The directory is not there either, which is the ordinary
                // case and not a failure worth a word. But the intent above
                // -- hearing the file *appear* -- is the whole reason this
                // watch is on the file the project would have, and it
                // cannot be served by watching a directory that is not
                // there. So the project itself is watched instead, which is
                // where `.obelus` will turn up.
                //
                // Not recursive: what is wanted is one directory appearing
                // directly in the project, and a recursive watch on a
                // repository is `target` and `.git` reported a thousand
                // times over. It is given up by nothing, because the
                // directory can go again as easily as it came -- and where
                // the reader has a file of the project's root open, this is
                // the same watch counted twice rather than a second one.
                false => tracing::debug!(
                    %error,
                    path = %project.display(),
                    "no settings of the project's own yet"
                ),
            }
        }
    }

    /// Starts watching every open file for changes on disk.
    fn start_watching(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        let mut watcher = match Watcher::new(sender) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "auto-reload is off");
                // The one watcher failure worth saying: without it nothing
                // Obelus reads is read again for the rest of the session,
                // so a file changed in another window, a commit, and the
                // settings all go unheard.
                self.amiss.push(
                    "Nothing is being watched, so changes made elsewhere will not arrive"
                        .to_string(),
                );
                return;
            }
        };
        for buffer in self.documents.iter().flatten().filter_map(Document::file) {
            if let Err(error) = watcher.watch(buffer.path()) {
                tracing::warn!(%error, path = %buffer.path().display(), "not watching");
            }
        }
        // And the settings, because Obelus is not the only Obelus. Several
        // of them on one project is the ordinary way to work -- the
        // terminal splits the window, Obelus does not -- so a setting
        // changed in one of them is a setting changed for all of them, and
        // a file read once at startup would leave every other window
        // holding what the reader has already moved on from.
        if let Some(path) = self.settled.path.clone() {
            // And whatever it really names, which for a reader who keeps
            // their settings in a dotfiles repository is a file in there:
            // what a `git pull` rewrites is that one, and a watch on the
            // link's own directory would never hear about it. Both, because
            // the link itself can be replaced too -- by the thing that made
            // it -- and that is a change to these settings as well.
            for path in [obelus_config::resolved(&path), path] {
                if let Err(error) = watcher.watch(&path) {
                    tracing::warn!(%error, path = %path.display(), "not watching the settings");
                }
            }
        }
        self.watcher = Some(watcher);
        // And wherever the colours come from, which is its own question:
        // a theme is a file Obelus never writes and something else may
        // replace under it.
        self.watch_theme();
    }
}
