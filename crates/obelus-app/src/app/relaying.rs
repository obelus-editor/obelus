//! Every window heard in the chat, through the one that talks to it.
//!
//! **One window talks to the chat, and every window is heard in it.** The
//! window holding the chat opens a door ([`obelus_remote::relay`]) and writes
//! where it is beside the lock; every other window on the machine with the
//! same chat set joins it, says which threads its conversations are, and
//! sends through it what it would have said to the platform. What the
//! platform says about a thread goes to the window whose thread it is, and
//! what nobody has open is answered here, as it always was.
//!
//! **Which window a thread begun in the chat goes to is the reader's to
//! say**, because a conversation is about a project and the agent works in
//! its tree. Where only one tree has a window on it there is nothing to ask;
//! where more than one has, the thread is answered with a card listing them
//! -- the project and the branch, one row a tree however many windows are
//! on it, and only trees that have a window: a tree nobody has open would
//! be a window to start, which nothing here does. Pressed, what they wrote
//! goes to that window as the first thing said, and to the window that
//! has the chat where it is one of them, or else the first to join.
//!
//! **A window joined to the chat is quiet about it.** Its status row has
//! the mark, so the reader can see that its conversations are in the chat
//! too, and none of the words: what is wrong with the connection is said
//! once, in the window that holds it.
//!
//! **A window hears where the chat is from the door's file moving**, which
//! the watcher says -- freshness and not a promise: a window that missed it
//! is one whose conversations are not in the chat until the file moves
//! again or it is started again, which is a window not heard and nothing
//! said wrongly.
//!
//! **A chat nobody let go of is taken up.** A relay that is let go says why
//! before it closes the door; one that closes it without a word went with
//! its process, and the windows joined to it wait in the kernel for the lock
//! it held -- the first to have it talks to the chat from then on, and the
//! rest join it. A chat handed to a window that asked, or let go by the
//! reader, is waited for and not taken.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use obelus_remote::{
    model::{Out, Question, Where},
    relay::{Asker, Door, Down, Listening, Numbers, Over, Place, Up, Window},
};

use super::*;

/// What this window keeps about the other windows heard in its chat, or
/// about the window it is heard through.
#[derive(Debug, Default)]
pub(super) struct Relaying {
    /// While this window holds the chat: the door the others join by.
    listening: Option<Listening>,
    /// The windows that have joined it, by its number for each.
    windows: BTreeMap<u64, Joined>,
    /// Its numbers for what each of them has asked of the platform.
    numbers: Numbers,
    /// Threads begun in the chat while the reader says which project they
    /// are for, by thread.
    choosing: BTreeMap<String, Choosing>,
    /// The last number one of those questions was put with.
    routed: u64,
    /// While another window holds the chat: where to send things to it.
    joined: Option<tokio::sync::mpsc::UnboundedSender<Up>>,
    /// Whether it has a connection to the platform for this window to be
    /// heard on, which is what a platform's `Started` says.
    relayed: bool,
    /// The door this window last joined, or tried to: a relay that went or
    /// never answered is not joined again until its file names another.
    door: Option<Door>,
    /// Whether the door's file has been read since it last moved.
    door_read: bool,
    /// What this window last told the relay of where it is.
    told_place: Option<Option<Place>>,
    /// And of which threads are its.
    told_threads: Option<BTreeSet<String>>,
    /// Where this window is, by the tree and the branch it was worked out
    /// for: naming the project opens the repository.
    here: Option<(PathBuf, Option<obelus_git::Head>, Option<Place>)>,
}

impl Drop for Relaying {
    /// A window that goes holding the chat takes its door's file with it,
    /// where the file is still its: nothing is to knock on a port somebody
    /// else may have by the time anybody does.
    fn drop(&mut self) {
        if let Some(listening) = &self.listening {
            close_the_door(listening);
        }
    }
}

/// A window that has joined this one.
#[derive(Debug)]
struct Joined {
    /// Where what is for it goes.
    down: tokio::sync::mpsc::UnboundedSender<Down>,
    /// Where it said it is.
    place: Option<Place>,
    /// The threads it said are its, and the ones handed to it since.
    threads: BTreeSet<String>,
    /// Threads handed to it that it has not yet said are its: a list of its
    /// threads it sent before it heard of one would otherwise take that one
    /// away again, and what was said there in the meantime would go to
    /// nobody.
    handed: BTreeSet<String>,
}

impl Joined {
    /// Hands it a thread, which is its from now.
    fn hand(&mut self, thread: &str) {
        self.threads.insert(thread.to_string());
        self.handed.insert(thread.to_string());
    }

    /// What it says its threads are, and those handed to it it has not
    /// caught up with -- which it has, once it names them.
    fn says_its_threads_are(&mut self, threads: BTreeSet<String>) {
        self.handed.retain(|thread| !threads.contains(thread));
        self.threads = threads.union(&self.handed).cloned().collect();
    }
}

/// A thread begun in the chat, and the question of where it goes.
#[derive(Debug)]
struct Choosing {
    /// The number the question was put with.
    asked: u64,
    /// Which group.
    room: String,
    /// Who began it.
    from: String,
    /// What they said, which goes to the window they choose.
    text: String,
    /// What the card offered, in its order.
    trees: Vec<Place>,
}

/// The file that says where the relay's door is.
fn the_door() -> Option<PathBuf> {
    Some(super::remote::remote_directory(Path::new(""))?.join("door"))
}

/// The door, as its file says, where it says one.
fn read_the_door() -> Option<Door> {
    Door::read(&std::fs::read_to_string(the_door()?).ok()?)
}

/// Takes the door's file away where it still says this door, and leaves it
/// where another window has written its own since.
fn close_the_door(listening: &Listening) {
    if read_the_door().as_ref() == Some(listening.door())
        && let Some(path) = the_door()
    {
        let _ = std::fs::remove_file(path);
    }
}

/// And written, beside and renamed over: another window may be reading it.
///
/// Readable by this reader alone where the system can say so: the key is
/// what lets a window say things to the chat as the bot and hear what the
/// reader says there, and the default mode lets every account on the
/// machine read a file.
fn write_the_door(door: &Door) {
    let written = (|| {
        use std::io::Write as _;

        let path = the_door()?;
        std::fs::create_dir_all(path.parent()?).ok()?;
        let beside = path.with_extension(format!("{}", std::process::id()));
        let mut options = std::fs::File::options();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&beside).ok()?;
        file.write_all(door.written().as_bytes()).ok()?;
        drop(file);
        std::fs::rename(&beside, &path).ok()
    })();
    if written.is_none() {
        tracing::warn!("the relay's door was not written down");
    }
}

impl App {
    /// Opens the door the other windows join this one by, now that it holds
    /// the chat.
    pub(super) fn start_relaying(&mut self) {
        if self.relaying.listening.is_some() {
            return;
        }
        let Some(events) = self.events.clone() else {
            return;
        };
        match obelus_remote::relay::listen(Arc::new(events)) {
            Ok(listening) => {
                write_the_door(listening.door());
                self.relaying.listening = Some(listening);
            }
            Err(error) => tracing::warn!(%error, "no door for the other windows to join by"),
        }
    }

    /// Closes it, saying why to every window that joined: `over` decides
    /// whether one of them takes the chat up.
    pub(super) fn stop_relaying(&mut self, over: Over) {
        for joined in std::mem::take(&mut self.relaying.windows).into_values() {
            let _ = joined.down.send(Down::Over(over));
        }
        if let Some(listening) = self.relaying.listening.take() {
            close_the_door(&listening);
        }
        self.relaying.numbers = Numbers::default();
        self.relaying.choosing.clear();
    }

    /// Whether a path that changed is the file saying where the relay is.
    pub(super) fn is_the_relays_door(&self, path: &Path) -> bool {
        the_door().is_some_and(|door| door == path)
    }

    /// The relay's door moved: read again on the next frame, by a window
    /// that wants it.
    pub(super) fn the_door_moved(&mut self) {
        self.relaying.door_read = false;
    }

    /// Whether this window is heard in the chat through another.
    pub(super) fn joined_the_relay(&self) -> bool {
        self.relaying.joined.is_some()
    }

    /// Whether the window it is heard through has a connection to speak on.
    pub(super) fn relayed(&self) -> bool {
        self.relaying.relayed
    }

    /// Something to ask of the platform: through the relay, from a window
    /// that is heard through one.
    pub(super) fn to_the_relay(&self, out: Out) {
        if let Some(joined) = &self.relaying.joined
            && joined.send(Up::Out(out)).is_err()
        {
            tracing::warn!("the relay has stopped listening");
        }
    }

    /// Something to ask of the platform, from the window that holds it,
    /// numbered the relay's way.
    pub(super) fn send_to_the_platform(&mut self, from: Asker, out: Out) {
        let Some(sending) = self.platform_out() else {
            return;
        };
        let out = self.relaying.numbers.out(from, out);
        if sending.send(out).is_err() {
            tracing::warn!("the chat has stopped listening");
        }
    }

    /// Something every window joined to this one hears.
    fn to_every_window(&self, down: &Down) {
        for joined in self.relaying.windows.values() {
            let _ = joined.down.send(down.clone());
        }
    }

    /// The platform was connected to again: what was on its way to the
    /// last connection, from every window, will not be answered.
    pub(super) fn relay_restarted(&mut self) {
        self.relaying.numbers.restarted();
        self.to_every_window(&Down::Started);
    }

    /// Where the connection has got to, for every window heard through it.
    pub(super) fn relay_connection(&self, state: obelus_remote::State, why: Option<String>) {
        self.to_every_window(&Down::Connection { state, why });
    }

    /// The group the threads are in, paired in here.
    pub(super) fn relay_room(&self, room: &str) {
        self.to_every_window(&Down::Room(room.to_string()));
    }

    /// A window joined to this one did something.
    pub(super) fn window_did(&mut self, window: u64, did: Window) {
        match did {
            Window::Came(down) => {
                // Turned away where this window has let the chat go since --
                // with a word, or the window would take a bare end for one
                // that died and take the chat up.
                if self.relaying.listening.is_none() {
                    let _ = down.send(Down::Over(Over::Stopped));
                    return;
                }
                // What it would have heard had it been here all along.
                if self.platform_out().is_some() {
                    let _ = down.send(Down::Started);
                }
                if let Some((state, why)) = self.connection_said() {
                    let _ = down.send(Down::Connection { state, why });
                }
                if let Some(room) = self.the_room() {
                    let _ = down.send(Down::Room(room));
                }
                tracing::info!(window, "a window joined the chat");
                self.relaying.windows.insert(
                    window,
                    Joined {
                        down,
                        place: None,
                        threads: BTreeSet::new(),
                        handed: BTreeSet::new(),
                    },
                );
            }
            Window::Said(up) => {
                let Some(joined) = self.relaying.windows.get_mut(&window) else {
                    return;
                };
                match up {
                    Up::Place(place) => joined.place = place,
                    Up::Threads(threads) => joined.says_its_threads_are(threads),
                    Up::Out(out) => self.send_to_the_platform(Asker::Window(window), out),
                }
            }
            Window::Went => {
                if self.relaying.windows.remove(&window).is_some() {
                    tracing::info!(window, "a window left the chat");
                    self.relaying.numbers.went(window);
                }
            }
        }
    }

    /// What the platform said about something asked of it: sent to the
    /// window that asked, or handed back under this window's own number
    /// where it was this one's. Anything else is this window's to hear.
    pub(super) fn whose_is_it(
        &mut self,
        event: obelus_remote::Event,
    ) -> Option<obelus_remote::Event> {
        use obelus_remote::Event as E;
        // On the same question as the numbering on the way out: a window
        // with no door, where the loopback would not be listened on, still
        // numbers what it asks the relay's way.
        if self.platform_out().is_none() {
            return Some(event);
        }
        match event {
            E::Opened {
                asked,
                thread,
                link,
            } => match self.relaying.numbers.opened(asked)? {
                (Asker::Window(window), theirs) => {
                    if let Some(joined) = self.relaying.windows.get_mut(&window) {
                        // Its from now, rather than from when it next says
                        // which are: a reply in the thread may come first.
                        joined.hand(&thread);
                        let _ = joined.down.send(Down::Opened {
                            asked: theirs,
                            thread,
                            link,
                        });
                    }
                    None
                }
                (_, theirs) => Some(E::Opened {
                    asked: theirs,
                    thread,
                    link,
                }),
            },
            E::Unopened { asked, waited } => match self.relaying.numbers.opened(asked)? {
                (Asker::Window(window), theirs) => {
                    self.to_the_window(
                        window,
                        Down::Unopened {
                            asked: theirs,
                            waited,
                        },
                    );
                    None
                }
                (_, theirs) => Some(E::Unopened {
                    asked: theirs,
                    waited,
                }),
            },
            E::Unasked { asked } => match self.relaying.numbers.asked(asked)? {
                (Asker::Window(window), theirs) => {
                    self.to_the_window(window, Down::Unasked { asked: theirs });
                    None
                }
                (Asker::Routing, theirs) => {
                    self.not_asked_where(theirs);
                    None
                }
                (Asker::Here, theirs) => Some(E::Unasked { asked: theirs }),
            },
            E::Answered {
                from,
                asked,
                chosen,
                words,
            } => {
                // Nobody who is not on the list is answered, wherever the
                // card came from.
                if !self.on_the_list(&from) {
                    return None;
                }
                match self.relaying.numbers.asked(asked)? {
                    (Asker::Window(window), theirs) => {
                        self.to_the_window(
                            window,
                            Down::Answered {
                                from,
                                asked: theirs,
                                chosen,
                                words,
                            },
                        );
                        None
                    }
                    (Asker::Routing, theirs) => {
                        self.chose_where(theirs, &chosen);
                        None
                    }
                    (Asker::Here, theirs) => Some(E::Answered {
                        from,
                        asked: theirs,
                        chosen,
                        words,
                    }),
                }
            }
            event => Some(event),
        }
    }

    /// Something for one window, where it is still joined.
    fn to_the_window(&self, window: u64, down: Down) {
        if let Some(joined) = self.relaying.windows.get(&window) {
            let _ = joined.down.send(down);
        }
    }

    /// Somebody on the list said something in the room, heard by the window
    /// that holds the chat: to the window whose thread it is, or whose
    /// project a thread they began is for.
    pub(super) fn route_heard(&mut self, from: &str, room: &str, at: &Where, text: &str) {
        let heard = |at: Where| Down::Heard {
            from: from.to_string(),
            room: room.to_string(),
            at,
            text: text.to_string(),
        };
        match at {
            Where::Thread(thread) => {
                if self.relaying.choosing.contains_key(thread) {
                    self.say_to(Out::Say {
                        room: room.to_string(),
                        thread: thread.clone(),
                        to: from.to_string(),
                        text: "Answer on the card above.".to_string(),
                        notify: false,
                    });
                    return;
                }
                match self.window_with(thread) {
                    Some(window) => self.to_the_window(window, heard(at.clone())),
                    None => self.heard_in_thread(thread, text),
                }
            }
            Where::Fresh(thread) => {
                // Heard twice is one thread: a platform sends an event again
                // when it thinks it was not taken.
                if self.relaying.choosing.contains_key(thread) {
                    return;
                }
                if let Some(window) = self.window_with(thread) {
                    self.to_the_window(window, heard(at.clone()));
                    return;
                }
                if self.keeps_the_thread(thread) {
                    self.heard_fresh(thread, text);
                    return;
                }
                self.route_fresh(from, room, thread, text);
            }
        }
    }

    /// The window joined to this one whose thread this is.
    fn window_with(&self, thread: &str) -> Option<u64> {
        self.relaying
            .windows
            .iter()
            .find(|(_, joined)| joined.threads.contains(thread))
            .map(|(window, _)| *window)
    }

    /// A thread begun in the chat: to the one tree a window is on, or a
    /// card asking which.
    fn route_fresh(&mut self, from: &str, room: &str, thread: &str, text: &str) {
        let trees = self.trees();
        if trees.len() < 2 {
            // Nowhere with a project is here, as it was before there was
            // anywhere else.
            let there = trees.first().map(|place| place.tree.clone());
            if there.is_none_or(|tree| !self.go_fresh(&tree, from, room, thread, text)) {
                self.heard_fresh(thread, text);
            }
            return;
        }
        self.relaying.routed += 1;
        let asked = self.relaying.routed;
        let question = Question {
            about: "Which project is this for?".to_string(),
            choices: trees
                .iter()
                .enumerate()
                .map(|(at, place)| (at.to_string(), place.name.clone()))
                .collect(),
            several: false,
            needed: true,
            words: None,
        };
        self.relaying.choosing.insert(
            thread.to_string(),
            Choosing {
                asked,
                room: room.to_string(),
                from: from.to_string(),
                text: text.to_string(),
                trees,
            },
        );
        self.send_to_the_platform(
            Asker::Routing,
            Out::Ask {
                room: room.to_string(),
                thread: thread.to_string(),
                to: from.to_string(),
                asked,
                question,
            },
        );
    }

    /// Every tree a window heard in the chat is on, this one's first and
    /// then in the order the others joined: one each, however many windows
    /// are on it.
    fn trees(&mut self) -> Vec<Place> {
        let mut trees: Vec<Place> = self.place_here().into_iter().collect();
        for joined in self.relaying.windows.values() {
            if let Some(place) = &joined.place
                && !trees.iter().any(|tree| tree.tree == place.tree)
            {
                trees.push(place.clone());
            }
        }
        trees
    }

    /// Hands a thread begun in the chat to a window on `tree`: this one where
    /// it is on it, or else the first to join that is. Whether there was
    /// one.
    fn go_fresh(&mut self, tree: &Path, from: &str, room: &str, thread: &str, text: &str) -> bool {
        if self
            .place_here()
            .is_some_and(|here| here.tree.as_path() == tree)
        {
            self.heard_fresh(thread, text);
            return true;
        }
        let Some(joined) = self.relaying.windows.values_mut().find(|joined| {
            joined
                .place
                .as_ref()
                .is_some_and(|place| place.tree == tree)
        }) else {
            return false;
        };
        joined.hand(thread);
        let _ = joined.down.send(Down::Heard {
            from: from.to_string(),
            room: room.to_string(),
            at: Where::Fresh(thread.to_string()),
            text: text.to_string(),
        });
        true
    }

    /// The reader said which project a thread they began is for.
    fn chose_where(&mut self, asked: u64, chosen: &[String]) {
        let Some(thread) = self
            .relaying
            .choosing
            .iter()
            .find(|(_, choosing)| choosing.asked == asked)
            .map(|(thread, _)| thread.clone())
        else {
            return;
        };
        let Some(choosing) = self.relaying.choosing.remove(&thread) else {
            return;
        };
        let Some(place) = chosen
            .first()
            .and_then(|id| id.parse::<usize>().ok())
            .and_then(|at| choosing.trees.get(at))
            .cloned()
        else {
            // Nothing chosen, which the card does not allow: the question
            // stays up.
            self.relaying.choosing.insert(thread, choosing);
            return;
        };
        let Choosing {
            room, from, text, ..
        } = choosing;
        self.send_to_the_platform(
            Asker::Routing,
            Out::Settle {
                room: room.clone(),
                thread: thread.clone(),
                to: from.clone(),
                asked,
                said: format!("\u{2714} {}", place.name),
            },
        );
        if !self.go_fresh(&place.tree, &from, &room, &thread, &text) {
            // Every window on it has gone since the card went up: asked
            // again, among the trees there are now.
            self.say_to(Out::Say {
                room: room.clone(),
                thread: thread.clone(),
                to: from.clone(),
                text: format!("No window is on {} any more.", place.name),
                notify: false,
            });
            self.route_fresh(&from, &room, &thread, &text);
        }
    }

    /// The card asking which project was not taken by the platform, which
    /// said it in words that nothing can answer: the thread is this
    /// window's, as it was before there was anywhere else.
    fn not_asked_where(&mut self, asked: u64) {
        let Some(thread) = self
            .relaying
            .choosing
            .iter()
            .find(|(_, choosing)| choosing.asked == asked)
            .map(|(thread, _)| thread.clone())
        else {
            return;
        };
        if let Some(choosing) = self.relaying.choosing.remove(&thread) {
            self.heard_fresh(&thread, &choosing.text);
        }
    }

    /// How many windows joined to this one have said where they are, for a
    /// test that has to know they have before a thread is begun.
    #[must_use]
    pub fn windows_placed_for_test(&self) -> usize {
        self.relaying
            .windows
            .values()
            .filter(|joined| joined.place.is_some())
            .count()
    }

    /// Where this window's door is, while it holds the chat.
    #[must_use]
    pub fn door_for_test(&self) -> Option<std::net::SocketAddr> {
        self.relaying
            .listening
            .as_ref()
            .map(|listening| listening.door().address)
    }

    /// The door this window is heard in the chat through, while it is.
    #[must_use]
    pub fn joined_for_test(&self) -> Option<std::net::SocketAddr> {
        self.relaying
            .joined
            .as_ref()
            .and(self.relaying.door.as_ref())
            .map(|door| door.address)
    }

    /// Where this window is, for the card: the project and the branch, and
    /// nowhere while it is on no project.
    fn place_here(&mut self) -> Option<Place> {
        if !self.has_a_project() {
            return None;
        }
        if let Some((tree, head, place)) = &self.relaying.here
            && *tree == self.working_directory
            && head.as_ref() == self.head.as_ref()
        {
            return place.clone();
        }
        let tree = self.working_directory.clone();
        let named = |path: &Path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        // The project by its main checkout, which is what every worktree of
        // it has in common -- and the tree by its branch, which is what the
        // reader calls it.
        let project = named(&obelus_git::main_checkout(&tree).unwrap_or_else(|| tree.clone()));
        let name = match &self.head {
            Some(obelus_git::Head::Branch(branch)) => format!("{project} \u{b7} {branch}"),
            Some(obelus_git::Head::Detached) => format!("{project} \u{b7} {}", named(&tree)),
            None => project,
        };
        // Resolved, so that two windows on one tree spelt two ways are one
        // row of the card.
        let place = Some(Place {
            tree: dunce::canonicalize(&tree).unwrap_or_else(|_| tree.clone()),
            name,
        });
        self.relaying.here = Some((tree, self.head.clone(), place.clone()));
        place
    }

    /// Joins the window that holds the chat, or leaves it, and tells it
    /// where this one is and which threads are its, where either has moved.
    ///
    /// Once a frame, from what is true: whether a window wants to be heard
    /// through another moves with the settings, with its own asking for the
    /// chat and with the project, and a join switched on and off at each of
    /// those would outlive its reason the first time one forgot. The door's
    /// file is read only when it has moved, or when this window starts
    /// wanting it -- a watch says what happens next, not what was there.
    pub(super) fn settle_the_relay(&mut self) {
        let wanted = self.platform().is_some() && !self.has_the_remote();
        if !wanted {
            if self.relaying.joined.is_some() {
                self.leave_the_relay();
            }
            self.relaying.door_read = false;
            self.relaying.door = None;
            return;
        }
        if !self.relaying.door_read {
            self.relaying.door_read = true;
            if let Some(door) = read_the_door()
                && self.relaying.door.as_ref() != Some(&door)
            {
                self.join_the_relay(door);
            }
        }
        let Some(joined) = self.relaying.joined.clone() else {
            return;
        };
        let place = self.place_here();
        if self.relaying.told_place.as_ref() != Some(&place) {
            let _ = joined.send(Up::Place(place.clone()));
            self.relaying.told_place = Some(place);
        }
        let threads = self.threads_open_here();
        if self.relaying.told_threads.as_ref() != Some(&threads) {
            let _ = joined.send(Up::Threads(threads.clone()));
            self.relaying.told_threads = Some(threads);
        }
    }

    /// Joins the relay behind `door`, leaving whatever this window was
    /// joined to before.
    fn join_the_relay(&mut self, door: Door) {
        if self.relaying.joined.is_some() {
            self.leave_the_relay();
        }
        let Some(sink) = self.a_numbered_connection() else {
            return;
        };
        tracing::info!(address = %door.address, "joining the window that has the chat");
        self.relaying.joined = Some(obelus_remote::relay::join(door.clone(), sink));
        self.relaying.door = Some(door);
    }

    /// Stops being heard through the relay, because this window wants the
    /// chat itself or wants none.
    fn leave_the_relay(&mut self) {
        tracing::info!("leaving the window that has the chat");
        self.relaying.joined = None;
        self.relaying.door = None;
        self.forget_the_relay();
    }

    /// What this window kept about a relay it is no longer heard through.
    fn forget_the_relay(&mut self) {
        self.relaying.relayed = false;
        self.relaying.told_place = None;
        self.relaying.told_threads = None;
        self.forget_what_the_relay_said();
        self.forget_what_was_on_its_way();
    }

    /// The relay this window was heard through has a new connection to the
    /// platform, which is somewhere to be heard: what was on its way to
    /// the last one is asked for again.
    pub(super) fn relay_has_started(&mut self) {
        self.relaying.relayed = true;
        self.no_longer_taking_up_the_chat();
        self.forget_what_was_on_its_way();
    }

    /// The relay this window was heard through went, and why.
    ///
    /// The door is kept, so that the same door is not knocked on again; its
    /// file is read again where something else may have been written there
    /// -- a window the chat was handed to, or one that took it up.
    pub(super) fn left_the_relay(&mut self, over: Over) {
        tracing::info!(?over, "the window that had the chat went");
        self.relaying.joined = None;
        self.forget_the_relay();
        if over != Over::Unreached {
            self.relaying.door_read = false;
        }
        if over == Over::Went {
            self.take_up_the_chat();
        }
    }
}
