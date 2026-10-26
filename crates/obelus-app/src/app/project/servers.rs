//! The machines this one has drawn an Obelus on, and the list
//! `connect-to-server` offers them in.
//!
//! **Kept by the machine the reader sits at.** Which servers the reader has
//! been on, with the key each printed, is a fact about the reader's own
//! machine -- the one whose windows connect -- so it is kept in this
//! machine's state and read by both the front end that connects (the key)
//! and the Obelus that offers the list (the rows). The key is kept beside
//! the machine's name so that a server is a row and not a question: the
//! first connection asks for it, and none after does.
//!
//! **A new one first, then the ones there have been, by the day.** The list
//! opens on the row that asks where, and the machines after it are in the
//! runs of days the conversations and the projects are in
//! (`conversations::when_said`).

use obelus_command::Command;
use obelus_component::{
    picker::{Picker, PickerItem, PickerLayout, PickerValue},
    prompt::{Prompt, PromptKind},
};

/// How tall the list may be.
const SERVER_ROWS: u16 = 16;

/// Where the list lives.
fn path() -> Option<std::path::PathBuf> {
    Some(obelus_logging::state_directory()?.join("servers.toml"))
}

/// A machine an Obelus was drawn on, the key it printed, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// The machine, and its port where the reader gave one.
    pub at: String,
    /// The key it printed, as it printed it.
    pub key: String,
    /// When a window last got in, in seconds since the epoch.
    pub last: Option<i64>,
}

/// Every machine kept, newest first -- or nothing where the file will not
/// read, which is said in the log and is not a reason to write over it.
fn read() -> Option<Vec<Server>> {
    let path = path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "the list of servers will not read");
            return None;
        }
    };
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "the list of servers will not read");
            return None;
        }
    };
    Some(servers_in(&table))
}

/// The machines a table holds, newest first.
fn servers_in(table: &toml::Table) -> Vec<Server> {
    let mut servers: Vec<Server> = table
        .get("server")
        .and_then(toml::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_table()?;
                    Some(Server {
                        at: row.get("at")?.as_str()?.to_string(),
                        key: row.get("key")?.as_str()?.to_string(),
                        last: row.get("last").and_then(toml::Value::as_integer),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    servers.sort_by_key(|server| std::cmp::Reverse(server.last));
    servers
}

/// What the file holds, as text a reader can read.
fn to_toml(servers: &[Server]) -> String {
    let mut out = String::new();
    for server in servers {
        out.push_str("[[server]]\n");
        out.push_str(&format!(
            "at = {}\n",
            toml::Value::String(server.at.clone())
        ));
        out.push_str(&format!(
            "key = {}\n",
            toml::Value::String(server.key.clone())
        ));
        if let Some(last) = server.last {
            out.push_str(&format!("last = {last}\n"));
        }
        out.push('\n');
    }
    out
}

/// The key kept for a machine, where one is.
#[must_use]
pub fn key_for(at: &str) -> Option<String> {
    read()?
        .into_iter()
        .find(|server| server.at == at)
        .map(|server| server.key)
}

/// Puts a machine at the top of the list with the key it let a window in
/// with, read-modify-write for the reason the projects' list is.
///
/// Answers whether it was written.
pub fn remember(at: &str, key: &str, now: i64) -> bool {
    let Some(path) = path() else {
        return false;
    };
    let Some(mut servers) = read() else {
        return false;
    };
    servers.retain(|server| server.at != at);
    servers.insert(
        0,
        Server {
            at: at.to_string(),
            key: key.to_string(),
            last: Some(now),
        },
    );
    super::projects::written_beside(&path, &to_toml(&servers))
}

impl crate::app::App {
    /// The list `connect-to-server` opens: a new machine first, and every
    /// one there has been after it.
    pub(in crate::app) fn open_servers(&mut self) {
        let now = std::time::SystemTime::now();
        let zone = jiff::tz::TimeZone::system();
        let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
        let new = PickerItem {
            prose: false,
            marker: Some((obelus_component::picker::Marking::Aside, "+".to_string())),
            icon: None,
            label: "New connection".to_string(),
            version: None,
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::NewServer,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        };
        let items = std::iter::once(new)
            .chain(
                read()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|server| PickerItem {
                        prose: false,
                        marker: None,
                        icon: None,
                        label: server.at.clone(),
                        version: None,
                        detail: None,
                        trailing: server.last.map(|last| obelus_git::how_long_ago(last, now)),
                        changed: None,
                        value: PickerValue::Server(server.at),
                        enabled: true,
                        colours: None,
                        status: None,
                        depth: 0,
                        opens: None,
                        kind: None,
                        tab: None,
                        section: Some(
                            crate::app::conversations::when_said(server.last, today, &zone)
                                .to_string(),
                        ),
                    }),
            )
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: SERVER_ROWS });
        // Wrapped, which is what draws a heading over each run of days.
        picker.wraps(None);
        picker.keeps_order(true);
        picker.before_typing("Filter machines");
        picker.opened_by(Command::ServerConnect);
        self.show_list(picker);
    }

    /// The new connection's row: where to, asked on the row the list was
    /// filtered on.
    pub(in crate::app) fn ask_where_to_connect(&mut self) {
        self.ask_on_the_status_row(Prompt::new(PromptKind::Server));
    }

    /// A machine named, from the list or the row that asked: gone to with
    /// the key kept for it, or the key asked for where there is none.
    pub(in crate::app) fn connect_to(&mut self, at: String) {
        let at = at.trim().to_string();
        if at.is_empty() {
            return;
        }
        match key_for(&at) {
            Some(key) => self.leave_for(crate::app::Going::To {
                at,
                key,
                window: None,
            }),
            None => self.ask_on_the_status_row(Prompt::new(PromptKind::ServerKey { at })),
        }
    }

    /// Back to the reader's own machine, from the Obelus on another one:
    /// this one ends, asking first what leaving asks.
    pub(in crate::app) fn go_home(&mut self) {
        self.leave_for(crate::app::Going::Home);
    }

    /// The key for a machine, typed: gone to with it.
    pub(in crate::app) fn connect_with(&mut self, at: String, key: &str) {
        let key = key.trim().to_string();
        if key.is_empty() {
            return;
        }
        self.leave_for(crate::app::Going::To {
            at,
            key,
            window: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is written is read back as it was, newest first.
    ///
    /// Broken deliberately by writing the key under another name (`secret`
    /// for `key`): the machine came back with no key, so not at all.
    #[test]
    fn the_list_of_servers_reads_back_as_written() {
        let servers = vec![
            Server {
                at: "devbox:9000".to_string(),
                key: "00ff".to_string(),
                last: Some(20),
            },
            Server {
                at: "10.0.0.2".to_string(),
                key: "ab\"cd".to_string(),
                last: Some(30),
            },
        ];
        let table: toml::Table = to_toml(&servers).parse().expect("the file reads");
        let back = servers_in(&table);
        assert_eq!(back, vec![servers[1].clone(), servers[0].clone()]);
    }
}
