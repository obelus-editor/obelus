//! Where a chat's tokens are kept: a file of their own in this machine's
//! state, and not the settings.
//!
//! The settings file is the one a reader is likely to keep in a repository
//! of dotfiles, public or not, and a token in it is a bot anybody can drive.
//! The state is what Obelus keeps about this machine, and it does not follow
//! the reader to another one -- which is right twice over here, because one
//! app's events are shared out at random among the machines connected to it.
//! The file is the reader's alone to read (`obelus_private`), the way the
//! keys `servers.toml` keeps beside it are.
//!
//! The system's keyring was tried and given up: on macOS every build is a
//! new program to the Keychain and asks to be let in again, on Linux there
//! may be no Secret Service to answer at all, and a prompt to unlock a
//! collection stood between the reader and a chat that only wanted a token
//! it already had. What it guarded against is somebody else on the machine
//! reading the file, which the file's permissions already say no to.
//!
//! **Two places, one question.** A test keeps them in a directory it names,
//! because a test that wrote to the reader's own would be a test that changed
//! their machine (`secrets_for_test`). Reading is still done on the runtime's
//! blocking pool, as everything that touches the disk on a key's behalf is.

use std::path::PathBuf;

/// Why a secret could not be read or kept, in the file system's own words,
/// for the log and the row it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trouble(String);

impl std::fmt::Display for Trouble {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a platform's field is kept as.
///
/// Under `remote`, the word the chat had before it went to working on
/// another machine, and kept so: a secret is something a reader typed in
/// once, and a new name would ask them to pair every platform again.
fn account(platform: &str, field: &str) -> String {
    format!("remote.{platform}.{field}")
}

/// One of a platform's secrets, or `None` where none has been kept.
///
/// # Errors
///
/// [`Trouble`] where the file could not be read.
pub fn read(platform: &str, field: &str) -> Result<Option<String>, Trouble> {
    Ok(file::read(&path()?)?.remove(&account(platform, field)))
}

/// Keeps one, over whatever was kept before.
///
/// # Errors
///
/// [`Trouble`] where the file could not be read or written.
pub fn write(platform: &str, field: &str, value: &str) -> Result<(), Trouble> {
    let path = path()?;
    let mut kept = file::read(&path)?;
    kept.insert(account(platform, field), value.to_string());
    file::write(&path, &kept)
}

/// Forgets one. Forgetting one that was never kept is not a failure.
///
/// # Errors
///
/// [`Trouble`] where the file could not be read or written.
pub fn forget(platform: &str, field: &str) -> Result<(), Trouble> {
    let path = path()?;
    let mut kept = file::read(&path)?;
    if kept.remove(&account(platform, field)).is_some() {
        file::write(&path, &kept)?;
    }
    Ok(())
}

/// Keeps every secret in this directory instead, for the rest of the
/// process.
///
/// For a test: one that read or wrote the reader's own file would be a test
/// that changed their machine. The first call wins, the way the state
/// directory's does.
pub fn secrets_for_test(directory: PathBuf) {
    let _ = ELSEWHERE.set(directory);
}

/// Where a test has said they go.
static ELSEWHERE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn path() -> Result<PathBuf, Trouble> {
    ELSEWHERE
        .get()
        .cloned()
        .or_else(obelus_logging::state_directory)
        .map(|directory| directory.join("secrets.toml"))
        .ok_or_else(|| Trouble("there is nowhere on this machine to keep a token".to_string()))
}

/// A table of strings, only the owner's to read.
mod file {
    use std::{collections::BTreeMap, path::Path};

    use super::Trouble;

    pub(super) fn read(path: &Path) -> Result<BTreeMap<String, String>, Trouble> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(Trouble(error.to_string())),
        };
        let table = text
            .parse::<toml::Table>()
            .map_err(|error| Trouble(error.to_string()))?;
        Ok(table
            .into_iter()
            .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
            .collect())
    }

    /// Private from the moment it has a name, and renamed over the old one,
    /// so that another Obelus reading at the same moment reads the old one
    /// or the new one and never half of either (`obelus_private`).
    pub(super) fn write(path: &Path, kept: &BTreeMap<String, String>) -> Result<(), Trouble> {
        let failed = |error: std::io::Error| Trouble(error.to_string());
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(failed)?;
        }
        let table: toml::Table = kept
            .iter()
            .map(|(key, value)| (key.clone(), toml::Value::String(value.clone())))
            .collect();
        obelus_private::write(path, table.to_string().as_bytes()).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::file;

    /// What is kept is what comes back, and nobody else can read the file.
    ///
    /// Broken deliberately by writing it with `std::fs::write` rather than
    /// through `obelus_private`: the file came back readable by its group
    /// and everybody else.
    #[test]
    fn a_kept_secret_comes_back_and_is_the_owners_alone() {
        let directory = std::env::temp_dir().join(format!("obelus-secrets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("secrets.toml");
        let mut kept = std::collections::BTreeMap::new();
        kept.insert("remote.slack.app_token".to_string(), "xapp-1-x".to_string());
        file::write(&path, &kept).expect("writing");
        assert_eq!(file::read(&path).expect("reading"), kept);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("there")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the secrets are readable by others: {mode:o}"
            );
        }
        let _ = std::fs::remove_dir_all(&directory);
    }
}
