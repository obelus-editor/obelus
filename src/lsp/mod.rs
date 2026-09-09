//! Talking to a language server.

pub mod action;
pub mod client;
pub mod position;
pub mod transport;

use std::path::Path;

use crate::syntax::LanguageId;

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
pub const fn command_for(language: LanguageId) -> Option<&'static str> {
    match language {
        LanguageId::Rust => Some("rust-analyzer"),
        // taplo and the JSON server exist, and neither is installed often
        // enough to be worth a row that only ever fails to find them.
        LanguageId::Toml | LanguageId::Json => None,
    }
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
