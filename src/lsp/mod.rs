//! Talking to a language server.

pub mod action;
pub mod actions;
pub mod client;
pub mod colour;
pub mod complete;
pub mod edits;
pub mod hierarchy;
pub mod hint;
pub mod hover;
pub mod outline;
pub mod position;
pub mod renaming;
pub mod signature;
pub mod snippet;
pub mod tokens;
pub mod transport;
pub mod trouble;
pub mod uses;

use std::path::{Path, PathBuf};

use crate::syntax::LanguageId;

/// What a language server is doing, as far as the status bar is concerned.
///
/// Three states rather than a boolean, because the two that are not "ready"
/// mean opposite things to a reader whose jump did nothing: one is worth
/// waiting for and the other is worth restarting obelus over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerState {
    /// Spawned, handshake not answered yet.
    Starting,
    /// Handshake answered and the process is alive.
    Ready,
    /// The process is no longer running.
    Gone,
}

impl ServerState {
    /// The Nerd Font glyph for the state.
    ///
    /// A network icon rather than a shape: what a language server is, to a
    /// reader, is something at the other end of a pipe that is either
    /// answering or not.
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Self::Starting => crate::icons::ui::SERVER_STARTING,
            Self::Ready => crate::icons::ui::SERVER_READY,
            Self::Gone => crate::icons::ui::SERVER_GONE,
        }
    }

    /// The mark that stands for the state.
    ///
    /// Ordinary Unicode, not a Nerd Font glyph: this one is on screen the
    /// whole time, so it cannot depend on a font obelus has not been told
    /// about.
    #[must_use]
    pub const fn mark(self) -> char {
        match self {
            Self::Starting => '\u{25cb}',
            Self::Ready => '\u{25cf}',
            Self::Gone => '\u{2715}',
        }
    }
}

/// A program to run, and what to say to it on the command line.
///
/// The arguments are the reason this is a struct: most servers speak the
/// protocol on stdio only when told to (`--stdio`, `start`), and a table that
/// held a bare command name could describe four of the fourteen languages
/// obelus can highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Server {
    /// The program, looked up on `PATH`.
    pub command: &'static str,
    /// What to pass it.
    pub arguments: &'static [&'static str],
}

/// The server to run for a language.
///
/// A table, so a language gains a server by gaining a row. Only the languages
/// obelus can already highlight are here: a server for a language it cannot
/// identify would have nothing to attach to.
///
/// Nothing is installed on obelus's behalf. Reaching into a package manager to
/// fetch a server is a job with no end — the wrong version, the wrong
/// platform, the download that fails behind a proxy — and every editor that
/// has tried it has ended up maintaining a package manager.
#[must_use]
pub const fn server_for(language: LanguageId) -> Option<Server> {
    const fn bare(command: &'static str) -> Option<Server> {
        Some(Server {
            command,
            arguments: &[],
        })
    }
    const fn stdio(command: &'static str) -> Option<Server> {
        Some(Server {
            command,
            arguments: &["--stdio"],
        })
    }

    match language {
        LanguageId::Rust => bare("rust-analyzer"),
        LanguageId::Go => bare("gopls"),
        LanguageId::C | LanguageId::Cpp => bare("clangd"),
        LanguageId::Python => bare("pylsp"),
        // One program for all three, but obelus keys its servers by language,
        // so a project with `.ts` and `.tsx` files in it runs two copies.
        // That is worth fixing by keying on the command instead -- a bigger
        // change than this table -- and not worth leaving the languages out
        // over.
        LanguageId::JavaScript | LanguageId::TypeScript | LanguageId::Tsx => {
            stdio("typescript-language-server")
        }
        // `start`, not `--stdio`. Every one of these spells it differently,
        // which is the whole reason the arguments live in the table.
        LanguageId::Bash => Some(Server {
            command: "bash-language-server",
            arguments: &["start"],
        }),
        LanguageId::Css => stdio("vscode-css-language-server"),
        LanguageId::Html => stdio("vscode-html-language-server"),
        LanguageId::Yaml => stdio("yaml-language-server"),
        // taplo and the JSON server exist, and neither is installed often
        // enough to be worth a row that only ever fails to find them.
        LanguageId::Toml | LanguageId::Json | LanguageId::Markdown => None,
    }
}

/// Just the program, for the places that name it to the reader.
#[must_use]
pub const fn command_for(language: LanguageId) -> Option<&'static str> {
    match server_for(language) {
        Some(server) => Some(server.command),
        None => None,
    }
}

/// The path a `file:` uri names.
///
/// `None` for anything else -- `untitled:`, a scheme obelus has never
/// heard of -- which is a document obelus cannot open and so cannot edit.
///
/// [`client::path_of`] does the work, because it is written next to
/// [`client::uri_for`]: an escaping and an unescaping that disagree name a
/// different file. The one this replaced disagreed -- it turned each
/// escaped *byte* into a character, so every path with a non-ASCII letter
/// in it came back mojibake and named nothing.
#[must_use]
pub fn path_of_uri(uri: &str) -> Option<PathBuf> {
    client::path_of(uri)
}

/// What to tell a server about a file that changed on disk.
///
/// `None` for a path that cannot be a uri, which is the one case there is
/// nothing to say. The kind is worked out from what is there *now*: the
/// watcher says a path moved and not how, and a file that is gone is the
/// one case a server must not go on reading -- told that it merely
/// changed, it would try.
#[must_use]
pub fn watched_change(path: &Path) -> Option<serde_json::Value> {
    let uri = client::uri_for(path).ok()?;
    // 1 created, 2 changed, 3 deleted, as the protocol numbers them.
    let kind = match path.exists() {
        true => 2,
        false => 3,
    };
    Some(serde_json::json!({ "changes": [{ "uri": uri, "type": kind }] }))
}

/// Whether a command can be found.
#[must_use]
pub fn on_path(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    path.metadata()
        .is_ok_and(|data| data.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every server obelus knows how to start, and the arguments it needs.
    ///
    /// A server told to speak the protocol on stdio and not given the flag
    /// that makes it do so sits there saying nothing, which looks exactly
    /// like a server that is still indexing.
    #[test]
    fn the_servers_that_need_arguments_have_them() {
        for language in LanguageId::ALL.iter().copied() {
            let Some(server) = server_for(language) else {
                continue;
            };
            assert!(!server.command.is_empty(), "{}", language.name());
            assert_eq!(
                command_for(language),
                Some(server.command),
                "the two readers of the table disagree for {}",
                language.name()
            );
        }

        // The three that are known to need persuading, by name, so a table
        // edit that dropped the flag fails here rather than in a session.
        let typescript = server_for(LanguageId::TypeScript).expect("a server");
        assert_eq!(typescript.arguments, ["--stdio"]);
        assert_eq!(
            server_for(LanguageId::Tsx),
            server_for(LanguageId::JavaScript),
            "one program serves all three flavours"
        );
        assert_eq!(
            server_for(LanguageId::Bash).expect("a server").arguments,
            ["start"],
            "bash-language-server spells it differently, which is the point"
        );
        assert!(
            server_for(LanguageId::Rust)
                .expect("a server")
                .arguments
                .is_empty(),
            "rust-analyzer needs no arguments"
        );
    }

    #[test]
    fn a_language_with_no_server_has_no_row() {
        assert_eq!(command_for(LanguageId::Rust), Some("rust-analyzer"));
        assert_eq!(command_for(LanguageId::Toml), None);
    }

    /// The probe has to reject a directory and a non-executable file, or
    /// obelus tries to spawn something that cannot run and reports it as the
    /// server failing rather than as never having been there.
    #[test]
    fn the_probe_finds_a_real_command_and_nothing_else() {
        assert!(on_path("sh"), "sh should be on PATH");
        assert!(!on_path("obelus-not-a-real-command"));
        // A directory that exists on PATH-like paths must not count.
        assert!(!on_path("."));
    }
}
