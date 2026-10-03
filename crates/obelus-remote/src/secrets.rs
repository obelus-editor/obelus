//! Where a chat's tokens are kept: the system's keyring, and not a file.
//!
//! The settings file is the one a reader is likely to keep in a repository
//! of dotfiles, public or not, and a token in it is a bot anybody can drive.
//! The keyring is the system's own answer to "where does a password go", and
//! it does not follow the reader to another machine -- which is right twice
//! over here, because one app's events are shared out at random among the
//! machines connected to it.
//!
//! **Three places, one question.** A release keeps them in the keyring. A
//! development build keeps them in a file of its own unless told otherwise
//! (`OBELUS_USE_KEYCHAIN=1`): on macOS every build is a new program to the
//! Keychain and asks to be let in again, which zed found was enough to make
//! it do the same. And a test keeps them in a directory it names, because a
//! test that wrote to the reader's keyring would be a test that changed their
//! machine (`secrets_for_test`).
//!
//! **Every one of these waits.** The keyring is a service: on Linux a D-Bus
//! call that may put a prompt up to unlock a collection, on macOS one that may
//! ask whether this program may read the item. So nothing here is called from
//! the loop; the callers run it on the runtime's blocking pool.

use std::path::PathBuf;

/// The service every one of Obelus's entries is filed under.
const SERVICE: &str = "obelus";

/// Why a secret could not be read or kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// There is no keyring on this machine to keep it in -- a Linux with
    /// nothing answering for the Secret Service, which is a server, or a
    /// session that cannot reach the one that is.
    NoKeyring,
    /// There is one and it would not open: locked, or the reader said no.
    Locked,
    /// Something else, in the store's own words, for the log.
    Failed(String),
}

impl std::fmt::Display for Trouble {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoKeyring => formatter.write_str("there is no keyring on this machine"),
            Self::Locked => formatter.write_str("the keyring is locked"),
            Self::Failed(why) => formatter.write_str(why),
        }
    }
}

/// What a platform's field is kept as, by whichever store is keeping it.
fn account(platform: &str, field: &str) -> String {
    format!("remote.{platform}.{field}")
}

/// One of a platform's secrets, or `None` where none has been kept.
///
/// # Errors
///
/// [`Trouble`] where the store could not be asked.
pub fn read(platform: &str, field: &str) -> Result<Option<String>, Trouble> {
    let account = account(platform, field);
    match store() {
        Store::File(path) => Ok(file::read(&path)?.remove(&account)),
        Store::Keyring => keyring::read(&account),
    }
}

/// Keeps one, over whatever was kept before.
///
/// # Errors
///
/// [`Trouble`] where the store could not be asked.
pub fn write(platform: &str, field: &str, value: &str) -> Result<(), Trouble> {
    let account = account(platform, field);
    match store() {
        Store::File(path) => {
            let mut kept = file::read(&path)?;
            kept.insert(account, value.to_string());
            file::write(&path, &kept)
        }
        Store::Keyring => keyring::write(&account, value),
    }
}

/// Forgets one. Forgetting one that was never kept is not a failure.
///
/// # Errors
///
/// [`Trouble`] where the store could not be asked.
pub fn forget(platform: &str, field: &str) -> Result<(), Trouble> {
    let account = account(platform, field);
    match store() {
        Store::File(path) => {
            let mut kept = file::read(&path)?;
            if kept.remove(&account).is_some() {
                file::write(&path, &kept)?;
            }
            Ok(())
        }
        Store::Keyring => keyring::forget(&account),
    }
}

/// Keeps every secret in this directory instead, for the rest of the
/// process.
///
/// For a test: one that read or wrote the reader's keyring would be a test
/// that changed their machine, and on macOS one that stopped at a prompt
/// nobody is there to answer. The first call wins, the way the state
/// directory's does.
pub fn secrets_for_test(directory: PathBuf) {
    let _ = ELSEWHERE.set(directory);
}

/// Where a test has said they go.
static ELSEWHERE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Which of the three places this process keeps them in.
enum Store {
    /// A file of their own: a test's, or a development build's.
    File(PathBuf),
    /// The system's keyring.
    Keyring,
}

fn store() -> Store {
    if let Some(directory) = ELSEWHERE.get() {
        return Store::File(directory.join("secrets.toml"));
    }
    // A development build, unless it has been told to use the real one --
    // which is the one way to try the keyring path short of a release.
    if cfg!(debug_assertions)
        && std::env::var_os("OBELUS_USE_KEYCHAIN").is_none_or(|said| said.is_empty())
        && let Some(path) = development_file()
    {
        return Store::File(path);
    }
    Store::Keyring
}

/// Beside the settings rather than in the state: a token is something the
/// reader gave Obelus, not something it worked out and could again.
fn development_file() -> Option<PathBuf> {
    Some(
        obelus_config::path()?
            .parent()?
            .join("development-secrets.toml"),
    )
}

/// The file the other two places use: a table of strings, only the owner's
/// to read.
mod file {
    use std::{collections::BTreeMap, path::Path};

    use super::Trouble;

    pub(super) fn read(path: &Path) -> Result<BTreeMap<String, String>, Trouble> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(Trouble::Failed(error.to_string())),
        };
        let table = text
            .parse::<toml::Table>()
            .map_err(|error| Trouble::Failed(error.to_string()))?;
        Ok(table
            .into_iter()
            .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
            .collect())
    }

    /// Beside the file and renamed over it, so that another Obelus reading
    /// at the same moment reads the old one or the new one and never half
    /// of either; and made unreadable to anybody else before anything is
    /// written into it.
    pub(super) fn write(path: &Path, kept: &BTreeMap<String, String>) -> Result<(), Trouble> {
        let failed = |error: std::io::Error| Trouble::Failed(error.to_string());
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(failed)?;
        }
        let table: toml::Table = kept
            .iter()
            .map(|(key, value)| (key.clone(), toml::Value::String(value.clone())))
            .collect();
        let beside = path.with_extension(format!("toml.{}", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&beside).map_err(failed)?;
        std::io::Write::write_all(&mut file, table.to_string().as_bytes()).map_err(failed)?;
        drop(file);
        std::fs::rename(&beside, path).map_err(failed)
    }
}

/// The system's keyring, through whichever store this system has.
mod keyring {
    use super::{SERVICE, Trouble};

    /// The store, made once: it is a connection to a service, and on Linux
    /// whether there is one to connect to is the whole of the answer to
    /// "is there a keyring here".
    static STORE: std::sync::LazyLock<Result<(), Trouble>> = std::sync::LazyLock::new(|| {
        #[cfg(target_os = "macos")]
        let store = apple_native_keyring_store::keychain::Store::new();
        #[cfg(windows)]
        let store = windows_native_keyring_store::Store::new();
        #[cfg(all(unix, not(target_os = "macos")))]
        let store = zbus_secret_service_keyring_store::Store::new();
        match store {
            Ok(store) => {
                keyring_core::set_default_store(store);
                Ok(())
            }
            Err(error) => {
                tracing::warn!(%error, "there is no keyring to keep a chat's tokens in");
                Err(Trouble::NoKeyring)
            }
        }
    });

    fn entry(account: &str) -> Result<keyring_core::Entry, Trouble> {
        STORE.clone()?;
        keyring_core::Entry::new(SERVICE, account).map_err(trouble)
    }

    pub(super) fn read(account: &str) -> Result<Option<String>, Trouble> {
        match entry(account)?.get_password() {
            Ok(said) => Ok(Some(said)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(trouble(error)),
        }
    }

    pub(super) fn write(account: &str, value: &str) -> Result<(), Trouble> {
        entry(account)?.set_password(value).map_err(trouble)
    }

    pub(super) fn forget(account: &str) -> Result<(), Trouble> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(trouble(error)),
        }
    }

    fn trouble(error: keyring_core::Error) -> Trouble {
        match error {
            keyring_core::Error::NoStorageAccess(_) => Trouble::Locked,
            keyring_core::Error::NoDefaultStore => Trouble::NoKeyring,
            error => Trouble::Failed(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::file;

    /// What is kept is what comes back, and nobody else can read the file.
    ///
    /// Broken deliberately by taking the mode off the open: the file came
    /// back readable by its group and everybody else.
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

    /// The real keyring keeps a secret and gives it back, from the runtime's
    /// blocking pool -- which is where every caller asks it from, and where
    /// the Secret Service's D-Bus connection needs a runtime to be found.
    ///
    /// Ignored, because it writes to the keyring of whoever runs it: one
    /// entry, under an account no setting uses, taken out again before it
    /// returns. Run with `cargo test -p obelus-remote -- --ignored`.
    #[test]
    #[ignore = "writes to the keyring of whoever runs it"]
    fn the_keyring_keeps_a_secret() {
        let probe = obelus_runtime::handle().block_on(async {
            obelus_runtime::handle()
                .spawn_blocking(|| {
                    let account = "remote.test.probe";
                    super::keyring::write(account, "a probe")?;
                    let read = super::keyring::read(account);
                    super::keyring::forget(account)?;
                    Ok::<_, super::Trouble>((read?, super::keyring::read(account)?))
                })
                .await
                .expect("the blocking pool")
        });
        assert_eq!(
            probe,
            Ok((Some("a probe".to_string()), None)),
            "the keyring did not keep a secret, or kept it after it was forgotten"
        );
    }
}
