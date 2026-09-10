//! Which agents there are, which one is in use, and installing them.
//!
//! The list is somebody else's: [`crate::agent::registry`] fetches it and
//! this keeps it, alongside what obelus knows locally -- what is installed,
//! what is being installed and how far it has got, and what went wrong the
//! last time one was tried.

use super::*;
use crate::agent::{self, Agent, Distribution, Status, install::Progress};

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
        let root = agent::root();
        self.registry
            .iter()
            .map(|agent| {
                let status = if let Some(progress) = self.installing.get(&agent.id) {
                    let _ = progress;
                    Status::Installing
                } else if let Some(failure) = self.install_failures.get(&agent.id) {
                    Status::Failed(failure.clone())
                } else if root
                    .as_deref()
                    .and_then(|root| agent::command_for(agent, root))
                    .is_some()
                {
                    match root
                        .as_deref()
                        .and_then(|root| agent::installed_version(agent, root))
                    {
                        // The registry moves versions hourly, so this is
                        // the ordinary case for an agent installed a week
                        // ago -- and the only place a reader would find out.
                        Some(installed) if installed != agent.version => {
                            Status::Outdated { installed }
                        }
                        _ => Status::Installed,
                    }
                } else if agent.distribution.installable() {
                    Status::Missing
                } else {
                    Status::Unavailable("nothing for this machine")
                };
                Listed {
                    active: self.config().agent.as_deref() == Some(agent.id.as_str()),
                    progress: self.installing.get(&agent.id).copied(),
                    status,
                    agent: agent.clone(),
                }
            })
            .collect()
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
                tracing::warn!(id, why, "an agent did not install");
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
        let Some(root) = agent::root() else {
            self.note = Some("this system has nowhere to install to".to_string());
            return;
        };
        if let Distribution::Archive { .. } = agent.distribution {
            // The archive route reports bytes, so the page can say how far
            // through it is. Nothing to say yet, but the row has to stop
            // offering a button the moment it is pressed.
            self.installing.insert(
                agent.id.clone(),
                Progress {
                    done: 0,
                    total: None,
                    elapsed: std::time::Duration::ZERO,
                },
            );
        } else {
            self.installing.insert(
                agent.id.clone(),
                Progress {
                    done: 0,
                    total: None,
                    elapsed: std::time::Duration::ZERO,
                },
            );
        }
        self.install_failures.remove(id);
        if let Some(sender) = self.events.clone() {
            agent::install::spawn(&agent, &root, sender);
        }
    }

    /// Makes an agent the one obelus talks to.
    ///
    /// One at a time. Two would mean every question having to say which
    /// agent it was for, and a reader having to know.
    pub(super) fn activate_agent(&mut self, id: &str) {
        self.change_setting("agent", &crate::config::Value::Choice(id.to_string()));
    }

    /// Stops talking to whichever agent was active.
    pub(super) fn deactivate_agent(&mut self) {
        self.change_setting("agent", &crate::config::Value::Choice(String::new()));
    }
}
