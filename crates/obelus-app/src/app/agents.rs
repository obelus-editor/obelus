//! Which agents there are, which one is in use, and installing them.
//!
//! The list is somebody else's: [`obelus_agent::registry`] fetches it and
//! this keeps it, alongside what Obelus knows locally -- what is installed,
//! what is being installed and how far it has got, and what went wrong the
//! last time one was tried.

use obelus_agent::{Agent, Listed, Status, install::Progress};
use obelus_component::settings::Offering;
use obelus_ui::image::{Images, Palette};

use super::*;

/// Where the agents page's marks will be drawn, as much of it as two
/// frames have to be compared on.
///
/// A mark sits in two cells, and a terminal is handed a picture only when
/// one of those cells changed. Which cells they are is a function of
/// exactly these: the room the page has, which card the window starts at,
/// how tall each card above is, and the colours the marks were encoded
/// for. Equal here means the same cells, which means nothing is written.
#[derive(Debug, Default, PartialEq)]
pub(super) struct PictureLayout {
    /// How much room the page had.
    room: (u16, u16),
    /// Which card the window starts at.
    first: usize,
    /// Which card has the focus.
    ///
    /// A mark is drawn onto the background it will sit on, because a
    /// terminal is handed pixels rather than a colour scheme -- so the
    /// focused card's mark is a different picture from the same mark on
    /// any other card, and moving the focus rewrites two of them.
    focus: usize,
    /// Every listed agent: its id, how many rows its card takes, and
    /// whether a mark of its own has arrived.
    ///
    /// The last because the marks arrive one at a time, each its own
    /// event -- and the frame a mark arrives on writes a picture into
    /// cells that had a glyph in them, with nothing else on the page
    /// having moved.
    cards: Vec<(String, u16, bool)>,
    /// What the marks were encoded for. `None` before there are settings
    /// to ask, which is not a page anything was drawn on.
    palette: Option<Palette>,
}

/// What Obelus knows about the agents it could run.
///
/// One field on `App` rather than nine. They are one subject -- the list,
/// the mark beside each name, the installs in flight -- and they arrive
/// together: the registry is fetched, the marks are fetched for what it
/// listed, an install is started for one of those. Spread across the
/// application they read as nine unrelated things, and the one that says
/// where they live is the only one a test ever sets.
#[derive(Debug, Default)]
pub(super) struct Agents {
    /// The agents the registry lists, cached-then-fetched.
    pub registry: Vec<obelus_agent::Agent>,
    /// Whether the registry has been asked for and not yet failed.
    pub asked: bool,
    /// Why the registry could not be fetched, until it is tried again.
    pub failure: Option<String>,
    /// Each agent's own mark, as SVG, by the registry's id for it.
    pub icons: HashMap<String, String>,
    /// Whether the marks have been asked for.
    pub asked_icons: bool,
    /// The marks again, as pixels the terminal will take -- or nothing to
    /// take them, on a terminal that cannot show a picture.
    pub images: obelus_ui::image::Images,
    /// The installs running, and how far each has got.
    pub installing: HashMap<String, obelus_agent::install::Progress>,
    /// Why an install did not work, per agent, until it is tried again.
    pub install_failures: HashMap<String, String>,
    /// Where installed agents live, for a test that would rather not use
    /// the reader's own data directory. `None` is that directory.
    pub root: Option<PathBuf>,
    /// What one agent last said it can be set to, as the file beside its
    /// install has it -- and which agent that was.
    ///
    /// Read when there is a reason to and kept until there is another,
    /// rather than read where it is wanted: where it is wanted is
    /// [`App::agent_offering`], which the settings page asks two or three
    /// times a frame -- and a frame is drawn on every keystroke. The
    /// questions a view asks must already be answered; a file read behind
    /// one is work a view has caused.
    ///
    /// The agent's id is kept with it so that a copy left over from
    /// another agent is never handed out as this one's.
    pub offers: Option<(String, obelus_agent::options::Reading)>,
    /// What the conversation on screen will open on, while it has not.
    ///
    /// The protocol carries an agent's options in the answer to
    /// `session/new` and nowhere else, so a conversation nothing has been
    /// said in has none of them -- and the row that draws them went blank,
    /// which is not "there is nothing to set" but "nobody has asked yet".
    /// This is that question answered from the two things Obelus has
    /// without asking: what the agent last said it offers, beside its
    /// install, and what the reader has pinned for it.
    ///
    /// Worked out once a frame rather than kept, because what goes into it
    /// is already in memory and the reader can change any of the three
    /// while looking at it. Empty while a conversation has a session of its
    /// own, which is the ordinary case: then the row is the session's and
    /// this would be a second answer to the same question.
    pub before_a_session: Vec<obelus_agent::acp::Setting>,
}

impl App {
    /// The agents page's rows.
    ///
    /// Built on demand rather than kept: the registry, what is installed
    /// and which one is active are three separate facts, and a fourth copy
    /// of them that has to be refreshed is a fourth thing to get wrong.
    #[must_use]
    pub fn listed_agents(&self) -> Vec<Listed> {
        let root = self.agents_root();
        self.agents
            .registry
            .iter()
            .map(|agent| {
                // What the install wrote down when it finished, which is the
                // only thing that says an agent is installed. Never npm's
                // own tree: it is built in an order of its own, so a run
                // that was killed leaves a directory that looks finished.
                let installed = root
                    .as_deref()
                    .and_then(|root| obelus_agent::installation(&agent.id, root));
                let status = if self.agents.installing.contains_key(&agent.id) {
                    Status::Installing
                } else if let Some(failure) = self.agents.install_failures.get(&agent.id) {
                    Status::Failed(failure.clone())
                } else if let Some(installed) = installed {
                    // The registry moves versions hourly, so this is the
                    // ordinary case for an agent installed a week ago --
                    // and the only place a reader would find out.
                    if installed.version == agent.version {
                        Status::Installed
                    } else {
                        Status::Outdated {
                            installed: installed.version,
                        }
                    }
                } else if agent.distribution.installable() {
                    Status::Missing
                } else {
                    Status::Unavailable("Nothing for this machine")
                };
                Listed {
                    // In use *and* here. An agent the settings name and the
                    // machine does not have is not something Obelus can
                    // talk to, and a card saying "active" over a button
                    // that offers to install it says two things at once.
                    active: self.config().agent.as_deref() == Some(agent.id.as_str())
                        && matches!(status, Status::Installed | Status::Outdated { .. }),
                    progress: self.agents.installing.get(&agent.id).copied(),
                    status,
                    agent: agent.clone(),
                }
            })
            .collect()
    }

    /// What the active agent offers to be set, and what the reader has
    /// said each should start on.
    ///
    /// Built on demand from three places, for the reason the cards are:
    /// which agent is active is a setting, what it offers is a file beside
    /// its install, and what the reader has chosen is another setting. A
    /// fourth copy kept level with all three is a fourth thing to get
    /// wrong.
    ///
    /// `None` where no agent is active, which is the one case where the
    /// settings page has no group for one: a heading over nothing, on a
    /// machine where the reader has not chosen an agent, would be Obelus
    /// asking them to set something up for nobody.
    #[must_use]
    pub fn agent_offering(&self) -> Option<Offering> {
        let id = match self.config().agent.as_deref() {
            None | Some("") => return None,
            Some(id) => id,
        };
        // What it is called, which is the heading. The registry's name for
        // it where the registry has arrived, and its id until then: a
        // heading that appears as a name and turns into another name when
        // a fetch lands is a page rearranging itself under the reader.
        let name = self
            .agents
            .registry
            .iter()
            .find(|agent| agent.id == id)
            .map_or(id, |agent| agent.name.as_str())
            .to_string();
        // Three answers, and each is a different thing to say. Nothing
        // written down is an agent Obelus has not talked to yet; a file
        // that will not read is not an agent with nothing to be set, and
        // saying so is how the reader finds out there is a file to look
        // at.
        //
        // From what was read, not from the file: this is asked several
        // times a frame.
        let read = self
            .agents
            .offers
            .as_ref()
            .filter(|(whose, _)| whose == id)
            .map(|(_, read)| read);
        let (offers, silence) = match read {
            Some(obelus_agent::options::Reading::Offers(offers)) if !offers.is_empty() => {
                (offers.clone(), None)
            }
            Some(obelus_agent::options::Reading::Unreadable(why)) => (
                Vec::new(),
                Some(format!("What it offers will not read: {why}")),
            ),
            _ => (
                Vec::new(),
                Some(
                    "What this one can be set to appears here after the first conversation \
                     with it."
                        .to_string(),
                ),
            ),
        };
        Some(Offering {
            name,
            offers,
            chosen: self.config().agent_defaults(id).clone(),
            silence,
        })
    }

    /// Reads what the active agent offers, for [`App::agent_offering`] to
    /// hand out until there is a reason to read again.
    ///
    /// Those reasons are all of them: the settings page opening, Obelus
    /// writing the file itself, and the active agent changing. A file
    /// another Obelus writes while this page is open is not among them --
    /// it is the agent's own statement about itself rather than anything
    /// the two are editing, so the worst a stale copy can do is list what
    /// that agent offered an hour ago.
    /// The same, for a test that has not been through the settings page.
    pub fn reread_what_the_agent_offers_for_test(&mut self) {
        self.reread_what_the_agent_offers();
    }

    pub(super) fn reread_what_the_agent_offers(&mut self) {
        let id = match self.config().agent.as_deref() {
            None | Some("") => {
                self.agents.offers = None;
                return;
            }
            Some(id) => id.to_string(),
        };
        let Some(root) = self.agents_root() else {
            self.agents.offers = None;
            return;
        };
        let read = obelus_agent::options::read(&id, &root);
        self.agents.offers = Some((id, read));
    }

    /// Where Obelus keeps the agents it installs.
    ///
    /// The reader's data directory, or wherever a test has pointed it: what
    /// is under here is written by installing things and read to find out
    /// whether they are installed, so a test of either would otherwise have
    /// to use the reader's own.
    #[must_use]
    pub(super) fn agents_root(&self) -> Option<PathBuf> {
        self.agents.root.clone().or_else(obelus_agent::root)
    }

    /// Keeps installed agents somewhere else, for a test.
    pub fn agents_root_for_test(&mut self, root: PathBuf) {
        self.agents.root = Some(root);
    }

    /// Fetches the registry, showing whatever was cached while it runs.
    ///
    /// Once per session: the registry's versions move hourly and a reader's
    /// session does not last that long, so asking again on every visit to
    /// the page would be a network round trip for the same answer.
    pub(super) fn refresh_registry(&mut self) {
        if !self.agents.registry.is_empty() || self.agents.asked {
            return;
        }
        self.agents.asked = true;
        self.agents.failure = None;
        // Nothing read here, not even the cache: the thread does both, and
        // the frame that opens the settings does no I/O at all.
        if let Some(sender) = self.events.clone() {
            obelus_agent::registry::spawn_fetch(sender);
        }
    }

    /// Takes a registry, from wherever it was read.
    ///
    /// A failure leaves whatever list there already is -- the cached one,
    /// usually -- and lets the next visit to the page try again: a session
    /// that started with no network is a session that may have one later.
    pub(super) fn on_registry(&mut self, agents: Vec<Agent>, failure: Option<String>) {
        if let Some(why) = failure {
            self.agents.failure = Some(why);
            self.agents.asked = false;
            return;
        }
        if agents.is_empty() {
            return;
        }
        self.agents.failure = None;
        self.agents.registry = agents;
        // Marks are fetched from the list, so this is the first moment
        // there is anything to fetch. The cached list arrives first and the
        // fetched one replaces it; the once-only flag inside means the
        // second arrival costs nothing.
        self.fetch_icons();
    }

    /// Takes the marks the terminal will draw, or the fact that it will
    /// not.
    ///
    /// Set from `main`, because asking the terminal what it can do means
    /// writing to it and reading its answer -- which has to happen before
    /// the alternate screen, and cannot happen inside a frame. A test gets
    /// no pictures, which is also what most terminals get.
    pub fn use_images(&mut self, images: Images) {
        self.agents.images = images;
    }

    /// Says what is drawing Obelus, so that it can be told the settings it
    /// is the only one that can act on.
    ///
    /// A terminal says nothing and is told nothing: its font, its size and
    /// its colours are the terminal's own, and a reader who wants larger
    /// text there asks the terminal for it. A window has no such owner --
    /// it draws its own pixels -- so the size of the text is a setting
    /// like any other, and this is how the change reaches the thing that
    /// would have to redraw.
    pub fn drawn_by(&mut self, drawing: std::sync::Arc<dyn crate::app::Drawing>) {
        drawing.text_size(self.settled.config.font_size);
        drawing.use_fonts(&self.settled.config.fonts);
        self.drawing = Some(drawing);
    }

    /// The marks, for the view to draw.
    #[must_use]
    pub fn images(&self) -> &Images {
        &self.agents.images
    }

    /// Fetches every mark Obelus does not have, once.
    ///
    /// Only on a terminal that can show one: on every other terminal the
    /// cards wear glyphs, and forty downloads for something nothing will
    /// draw is forty requests a reader did not ask for.
    fn fetch_icons(&mut self) {
        if !self.agents.images.available()
            || self.agents.asked_icons
            || self.agents.registry.is_empty()
        {
            return;
        }
        let wanted: Vec<(String, String)> = self
            .agents
            .registry
            .iter()
            .filter(|agent| !self.agents.icons.contains_key(&agent.id))
            .filter_map(|agent| {
                agent
                    .icon
                    .clone()
                    .map(|address| (agent.id.clone(), address))
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        self.agents.asked_icons = true;
        if let Some(sender) = self.events.clone() {
            obelus_agent::icon::spawn_fetch(wanted, sender);
        }
    }

    /// Takes one agent's mark.
    pub(super) fn on_icon(&mut self, id: String, svg: String) {
        self.agents.icons.insert(id, svg);
    }

    /// Moves the agents page's window of cards, if the focused one has
    /// left it.
    ///
    /// Here rather than in the view because it is state, and the view holds
    /// none; per frame rather than per keystroke because how many cards fit
    /// is a fact about the screen, which the reader can resize without
    /// pressing anything.
    pub(super) fn settle_agents(&mut self, editor_area: Rect) {
        if self.settings.is_none() {
            return;
        }
        let cards = self.settings.as_ref().is_some_and(Settings::on_agents);
        let listed = self.listed_agents();
        // Built before the page is borrowed: it comes out of the
        // application, and the page is about to be held mutably.
        let offering = self.agent_offering();
        let room = (editor_area.width, editor_area.height);
        if let Some(settings) = self.settings.as_mut() {
            match cards {
                true => settings.settle_cards(&listed, room),
                // A group of settings is a dozen entries and they all fit
                // today. It has a window anyway: a group that grows past
                // the screen should scroll rather than lose its last rows
                // silently -- and an entry is as tall as what it has to
                // say, so the window is settled by height like the cards.
                false => settings.settle_rows(room, offering.as_ref()),
            }
        }
    }

    /// Whether the next frame will hand the terminal a picture.
    ///
    /// Which is worth knowing outside the agents page, because a terminal
    /// handed a sixel draws it then and there -- in the middle of Obelus
    /// writing the rest of the frame. That is the one case where the frame
    /// has to be written with the caret put out, and everywhere else the
    /// caret is left alone: see [`crate::app::render`].
    ///
    /// One place today. If a second thing starts drawing pictures, this is
    /// the function that has to know about it.
    #[must_use]
    pub fn shows_pictures(&self) -> bool {
        self.agents.images.available()
            && !self.agents.icons.is_empty()
            && self.settings.as_ref().is_some_and(Settings::on_agents)
    }

    /// Whether this frame has to be written with the caret put out.
    ///
    /// Only a frame that writes a picture does, and a picture is written
    /// only where a cell changed -- so the question is whether the marks
    /// will land anywhere other than where they last landed, and
    /// [`Self::picture_moved`] answers it by comparing.
    ///
    /// Asked from what is true rather than from the event that woke the
    /// frame. Deciding it by event went wrong the obvious way: walking the
    /// cards scrolls the window, so the arrows move every mark on the page,
    /// and an arrow key left off the list of events that matter wrote its
    /// sixels under a caret the reader could see. The other direction cost
    /// as much -- a frame that hides and shows the caret is a frame the
    /// caret blinks in, and install progress arrives several times a
    /// second without moving a mark at all.
    pub(super) fn writes_a_picture(&mut self, editor: Rect) -> bool {
        if !self.shows_pictures() || !self.agents.images.requires_hidden_cursor() {
            // Nothing of this page is on screen to compare against, so
            // whatever comes back to it is a page that has to be written.
            self.picture_layout = None;
            return false;
        }
        self.picture_moved(editor)
    }

    /// Whether the marks would land anywhere other than where they did.
    ///
    /// The window is settled first, because which card is at the top is
    /// exactly what the arrows move: read before the page has been
    /// settled, this compares the frame's own focus against the previous
    /// frame's window and answers about a page that never existed.
    fn picture_moved(&mut self, editor: Rect) -> bool {
        self.settle_agents(editor);
        let now = self.picture_layout(editor);
        let moved = self.picture_layout.as_ref() != Some(&now);
        self.picture_layout = Some(now);
        moved
    }

    /// The same question, for a test.
    ///
    /// Without the two conditions in front of it, which are about the
    /// terminal: whether one can draw a picture at all is found by writing
    /// an escape sequence and reading the answer, and a test has nobody to
    /// ask. What a test is about here is the other half -- what counts as
    /// the marks having moved.
    #[must_use]
    pub fn picture_moved_for_test(&mut self, editor: Rect) -> bool {
        self.picture_moved(editor)
    }

    /// Where every mark on the agents page will be drawn, near enough to
    /// compare two frames of it.
    ///
    /// Not the coordinates themselves: a mark's position is a function of
    /// these, and every one of them is a value the page already keeps.
    /// The heights are in it because a card is as tall as its description
    /// needs and an install that failed adds a row -- so a card above the
    /// window's first can change height and push every mark below it down
    /// without anything else here moving.
    fn picture_layout(&self, editor: Rect) -> PictureLayout {
        let listed = self.listed_agents();
        let Some(settings) = self.settings.as_ref() else {
            return PictureLayout::default();
        };
        let rows = settings.agents(&listed);
        let width = obelus_component::settings::Settings::card_width(editor.width);
        let focus = settings.focus().min(rows.len().saturating_sub(1));
        PictureLayout {
            room: (editor.width, editor.height),
            first: settings.top().min(focus),
            focus,
            cards: rows
                .iter()
                .map(|agent| {
                    let id = agent.agent.id.clone();
                    let marked = self.agents.icons.contains_key(&id);
                    (id, settings.card_rows(agent, width), marked)
                })
                .collect(),
            palette: Some(self.icon_palette()),
        }
    }

    /// The colours the marks are inked and backed in.
    ///
    /// One answer, because a mark encoded for one palette and compared
    /// against another is a picture that never looks like it moved.
    fn icon_palette(&self) -> Palette {
        Palette {
            ink: self.theme().gutter_current,
            paper: self.theme().background,
            selected: self.theme().selected_row_background,
        }
    }

    /// Encodes the marks the agents page is about to draw.
    ///
    /// Per frame, and free after the first: encoding is cached, and what
    /// this walks is the handful of cards on screen. It happens here rather
    /// than in the view because handing pixels to a terminal changes what
    /// Obelus is holding, and a view holds nothing.
    pub(super) fn prepare_icons(&mut self) {
        if !self.shows_pictures() {
            return;
        }
        let palette = self.icon_palette();
        let listed = self.listed_agents();
        let wanted: Vec<(String, bool)> = {
            let Some(settings) = self.settings.as_ref() else {
                return;
            };
            let rows = settings.agents(&listed);
            let focus = settings.focus().min(rows.len().saturating_sub(1));
            rows.iter()
                .enumerate()
                .map(|(index, agent)| (agent.agent.id.clone(), index == focus))
                .collect()
        };
        let Agents { images, icons, .. } = &mut self.agents;
        for (id, focused) in wanted {
            if let Some(svg) = icons.get(&id) {
                images.prepare(&id, svg, focused, palette);
            }
        }
    }

    /// Why the list could not be fetched, if it could not.
    #[must_use]
    pub fn registry_failure(&self) -> Option<&str> {
        self.agents.failure.as_deref()
    }

    /// Notes how far an install has got.
    pub(super) fn on_installing(&mut self, id: String, progress: Progress) {
        self.agents.installing.insert(id, progress);
    }

    /// Takes an install's outcome.
    pub(super) fn on_installed(&mut self, id: String, failure: Option<String>) {
        self.agents.installing.remove(&id);
        match failure {
            Some(why) => {
                self.agents.install_failures.insert(id, why);
            }
            None => {
                self.agents.install_failures.remove(&id);
                // Installed and nothing else in use: the reader pressed the
                // button, so this is the one they want.
                if self.config().agent.is_none() {
                    self.activate_agent(&id);
                }
            }
        }
    }

    /// Starts installing an agent, or says why it will not.
    pub(super) fn install_agent(&mut self, id: &str) {
        let Some(agent) = self
            .agents
            .registry
            .iter()
            .find(|agent| agent.id == id)
            .cloned()
        else {
            return;
        };
        if self.agents.installing.contains_key(id) {
            return;
        }
        let Some(root) = self.agents_root() else {
            self.note = Some("This system has nowhere to install to".to_string());
            return;
        };
        // Nothing to report yet -- a download says how far through it is
        // once bytes arrive, and a package manager never does -- but the
        // row has to stop offering a button the moment it is pressed.
        self.agents.installing.insert(
            agent.id.clone(),
            Progress {
                done: 0,
                total: None,
                elapsed: std::time::Duration::ZERO,
            },
        );
        self.agents.install_failures.remove(id);
        if let Some(sender) = self.events.clone() {
            tracing::info!(id, root = %root.display(), "installing an agent");
            obelus_agent::install::spawn(&agent, &root, sender);
        }
    }

    /// Makes an agent the one Obelus talks to.
    ///
    /// One at a time. Two would mean every question having to say which
    /// agent it was for, and a reader having to know.
    ///
    /// Only one that is installed. The setting is written to a file and read
    /// back on every start, so writing it for an agent that is not here
    /// leaves a card reading "active" that nothing can talk to -- which the
    /// reader can only get out of by noticing that turning it off and on
    /// again is what fixes it.
    pub(super) fn activate_agent(&mut self, id: &str) {
        if self
            .agents_root()
            .and_then(|root| obelus_agent::installation(id, &root))
            .is_none()
        {
            tracing::warn!(id, "not using an agent that is not installed");
            self.note = Some(format!("{id} is not installed"));
            return;
        }
        // Whatever was running is not this one. Stopped rather than left
        // alive: `start_agent` only starts one when there is none, so an
        // agent left running is an agent every conversation goes on
        // talking to -- the reader having chosen another one on this very
        // page, and nothing on screen saying otherwise.
        if self.talking_to_someone_else(id) {
            self.stop_agent();
            self.let_the_conversations_go();
        }
        self.change_setting("agent", &obelus_config::Value::Choice(id.to_string()));
        self.reread_what_the_agent_offers();
        // And if Obelus has never been told what this one can be set to,
        // it asks -- by opening a conversation with it, because the
        // protocol has no other way: what an agent offers arrives with a
        // session and `initialize` says nothing about it.
        //
        // In the background, not on screen: the reader is on the settings
        // page and asked to use this agent, not to start talking to it.
        // But it is an ordinary conversation, in the list of what is open
        // like any other -- so if the agent wants them to sign in, the
        // question is somewhere they can answer it, rather than asked into
        // a session nothing can show.
        self.learn_what_the_agent_offers();
    }

    /// Whether an agent is running and it is not this one.
    fn talking_to_someone_else(&self, id: &str) -> bool {
        self.talker.as_ref().is_some_and(|talker| talker.id() != id)
    }

    /// Stops talking to whichever agent was active.
    ///
    /// And stops the process, if one is running: an agent nobody has chosen
    /// is an agent nobody is talking to, and leaving it alive would leave a
    /// node process holding a session Obelus can no longer reach.
    ///
    /// And lets the conversations go with it, for the other half of the
    /// same fact: a session is a name that agent gave to something, and a
    /// conversation still holding one asks nobody for a new one -- so
    /// choosing the agent again left the reader with conversations that
    /// could never be talked in.
    pub(super) fn deactivate_agent(&mut self) {
        self.stop_agent();
        self.let_the_conversations_go();
        self.change_setting("agent", &obelus_config::Value::Choice(String::new()));
        self.reread_what_the_agent_offers();
    }
}
