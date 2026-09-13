//! Which agents there are, which one is in use, and installing them.
//!
//! The list is somebody else's: [`crate::agent::registry`] fetches it and
//! this keeps it, alongside what obelus knows locally -- what is installed,
//! what is being installed and how far it has got, and what went wrong the
//! last time one was tried.

use super::*;
use crate::{
    agent::{self, Agent, Status, install::Progress},
    ui::image::{Images, Palette},
};

/// One row of the agents page: what the registry says, and what obelus
/// knows about it here.
#[derive(Clone, Debug)]
pub struct Listed {
    /// The registry's entry.
    pub agent: Agent,
    /// What obelus knows about it locally.
    pub status: Status,
    /// How far an install has got, while one is running.
    pub progress: Option<Progress>,
    /// Whether this is the one obelus would talk to.
    pub active: bool,
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
        self.registry
            .iter()
            .map(|agent| {
                // What the install wrote down when it finished, which is the
                // only thing that says an agent is installed. Never npm's
                // own tree: it is built in an order of its own, so a run
                // that was killed leaves a directory that looks finished.
                let installed = root
                    .as_deref()
                    .and_then(|root| agent::installation(&agent.id, root));
                let status = if self.installing.contains_key(&agent.id) {
                    Status::Installing
                } else if let Some(failure) = self.install_failures.get(&agent.id) {
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
                    Status::Unavailable("nothing for this machine")
                };
                Listed {
                    // In use *and* here. An agent the settings name and the
                    // machine does not have is not something obelus can
                    // talk to, and a card saying "active" over a button
                    // that offers to install it says two things at once.
                    active: self.config().agent.as_deref() == Some(agent.id.as_str())
                        && matches!(status, Status::Installed | Status::Outdated { .. }),
                    progress: self.installing.get(&agent.id).copied(),
                    status,
                    agent: agent.clone(),
                }
            })
            .collect()
    }

    /// Where obelus keeps the agents it installs.
    ///
    /// The reader's data directory, or wherever a test has pointed it: what
    /// is under here is written by installing things and read to find out
    /// whether they are installed, so a test of either would otherwise have
    /// to use the reader's own.
    #[must_use]
    pub(super) fn agents_root(&self) -> Option<PathBuf> {
        self.agents_root.clone().or_else(agent::root)
    }

    /// Keeps installed agents somewhere else, for a test.
    pub fn agents_root_for_test(&mut self, root: PathBuf) {
        self.agents_root = Some(root);
    }

    /// Fetches the registry, showing whatever was cached while it runs.
    ///
    /// Once per session: the registry's versions move hourly and a reader's
    /// session does not last that long, so asking again on every visit to
    /// the page would be a network round trip for the same answer.
    pub(super) fn refresh_registry(&mut self) {
        if !self.registry.is_empty() || self.asked_registry {
            return;
        }
        self.asked_registry = true;
        self.registry_failure = None;
        // Nothing read here, not even the cache: the thread does both, and
        // the frame that opens the settings does no I/O at all.
        if let Some(sender) = self.events.clone() {
            agent::registry::spawn_fetch(sender);
        }
    }

    /// Takes a registry, from wherever it was read.
    ///
    /// A failure leaves whatever list there already is -- the cached one,
    /// usually -- and lets the next visit to the page try again: a session
    /// that started with no network is a session that may have one later.
    pub(super) fn on_registry(&mut self, agents: Vec<Agent>, failure: Option<String>) {
        if let Some(why) = failure {
            self.registry_failure = Some(why);
            self.asked_registry = false;
            return;
        }
        if agents.is_empty() {
            return;
        }
        self.registry_failure = None;
        self.registry = agents;
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
        self.images = images;
    }

    /// The marks, for the view to draw.
    #[must_use]
    pub fn images(&self) -> &Images {
        &self.images
    }

    /// Fetches every mark obelus does not have, once.
    ///
    /// Only on a terminal that can show one: on every other terminal the
    /// cards wear glyphs, and forty downloads for something nothing will
    /// draw is forty requests a reader did not ask for.
    fn fetch_icons(&mut self) {
        if !self.images.available() || self.asked_icons || self.registry.is_empty() {
            return;
        }
        let wanted: Vec<(String, String)> = self
            .registry
            .iter()
            .filter(|agent| !self.icons.contains_key(&agent.id))
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
        self.asked_icons = true;
        if let Some(sender) = self.events.clone() {
            agent::icon::spawn_fetch(wanted, sender);
        }
    }

    /// Takes one agent's mark.
    pub(super) fn on_icon(&mut self, id: String, svg: String) {
        self.icons.insert(id, svg);
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
        let room = (editor_area.width, editor_area.height);
        if let Some(settings) = self.settings.as_mut() {
            match cards {
                true => settings.settle_cards(&listed, room),
                // A group of settings is a dozen rows and they all fit
                // today. It has a window anyway: a group that grows past
                // the screen should scroll rather than lose its last rows
                // silently.
                false => settings.settle_rows(room.1.saturating_sub(2)),
            }
        }
    }

    /// Whether the next frame will hand the terminal a picture.
    ///
    /// Which is worth knowing outside the agents page, because a terminal
    /// handed a sixel draws it then and there -- in the middle of obelus
    /// writing the rest of the frame. That is the one case where the frame
    /// has to be written with the caret put out, and everywhere else the
    /// caret is left alone: see [`crate::app::render`].
    ///
    /// One place today. If a second thing starts drawing pictures, this is
    /// the function that has to know about it.
    #[must_use]
    pub fn shows_pictures(&self) -> bool {
        self.images.available()
            && !self.icons.is_empty()
            && self.settings.as_ref().is_some_and(Settings::on_agents)
    }

    /// Encodes the marks the agents page is about to draw.
    ///
    /// Per frame, and free after the first: encoding is cached, and what
    /// this walks is the handful of cards on screen. It happens here rather
    /// than in the view because handing pixels to a terminal changes what
    /// obelus is holding, and a view holds nothing.
    pub(super) fn prepare_icons(&mut self) {
        if !self.shows_pictures() {
            return;
        }
        let palette = Palette {
            ink: self.theme().gutter_current,
            paper: self.theme().background,
            selected: self.theme().selected_row_background,
        };
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
        let Self { images, icons, .. } = self;
        for (id, focused) in wanted {
            if let Some(svg) = icons.get(&id) {
                images.prepare(&id, svg, focused, palette);
            }
        }
    }

    /// Why the list could not be fetched, if it could not.
    #[must_use]
    pub fn registry_failure(&self) -> Option<&str> {
        self.registry_failure.as_deref()
    }

    /// Notes how far an install has got.
    pub(super) fn on_installing(&mut self, id: String, progress: Progress) {
        self.installing.insert(id, progress);
    }

    /// Takes an install's outcome.
    pub(super) fn on_installed(&mut self, id: String, failure: Option<String>) {
        self.installing.remove(&id);
        match failure {
            Some(why) => {
                self.install_failures.insert(id, why);
            }
            None => {
                self.install_failures.remove(&id);
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
        let Some(agent) = self.registry.iter().find(|agent| agent.id == id).cloned() else {
            return;
        };
        if self.installing.contains_key(id) {
            return;
        }
        let Some(root) = self.agents_root() else {
            self.note = Some("this system has nowhere to install to".to_string());
            return;
        };
        // Nothing to report yet -- a download says how far through it is
        // once bytes arrive, and a package manager never does -- but the
        // row has to stop offering a button the moment it is pressed.
        self.installing.insert(
            agent.id.clone(),
            Progress {
                done: 0,
                total: None,
                elapsed: std::time::Duration::ZERO,
            },
        );
        self.install_failures.remove(id);
        if let Some(sender) = self.events.clone() {
            tracing::info!(id, root = %root.display(), "installing an agent");
            agent::install::spawn(&agent, &root, sender);
        }
    }

    /// Makes an agent the one obelus talks to.
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
            .and_then(|root| agent::installation(id, &root))
            .is_none()
        {
            tracing::warn!(id, "not using an agent that is not installed");
            self.note = Some(format!("{id} is not installed"));
            return;
        }
        self.change_setting("agent", &crate::config::Value::Choice(id.to_string()));
    }

    /// Stops talking to whichever agent was active.
    ///
    /// And stops the process, if one is running: an agent nobody has chosen
    /// is an agent nobody is talking to, and leaving it alive would leave a
    /// node process holding a session obelus can no longer reach.
    pub(super) fn deactivate_agent(&mut self) {
        self.stop_agent();
        self.change_setting("agent", &crate::config::Value::Choice(String::new()));
    }
}
