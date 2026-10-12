//! Taking a theme from the registry, and giving it back.
//!
//! **A theme is installed whole or not at all.** It is unpacked beside where
//! it goes, under a hidden name, read the way the editor reads a theme, and
//! only then put in place by a rename -- the one step a file system does
//! whole. A package that will not download, is not the one the registry
//! described, will not unpack or will not read leaves what was installed as
//! it was. Not the agents' way, which empties the directory and unpacks into
//! it: a theme is something the reader may be wearing, and an update that
//! failed half way would be the screen changing colour to whatever half of
//! a theme was left.
//!
//! **Which package it came from is written inside it**, as the last file of
//! the unpacked theme before it is put in place: its hash is what says
//! whether the theme the registry lists now is the one installed. Not its
//! commit, which is the author's -- or omarchy's -- and stays where it was
//! when the registry makes the same theme into other bytes, as it did when
//! it began measuring how much of the page each of omarchy's pictures
//! wants. A record written before the hash was is not known to be anything,
//! so it is behind.
//!
//! **One Obelus at a time installs a theme**, by the lock an agent's install
//! is held by -- beside the theme's directory, not in it, because the
//! directory is what the rename replaces.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

pub use obelus_download::Progress;
use obelus_sink::Sink;

use crate::{Event, Group, Listed, home};

/// What an installed theme says about where it came from.
pub const RECORD: &str = "installed.json";

/// What an installed theme says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The commit it was packed from.
    pub commit: String,
    /// What the package it was unpacked from hashes to -- none for one
    /// installed before this was written.
    pub sha256: Option<String>,
    /// Which shelf it came off -- kept, because a theme the registry no
    /// longer lists has nowhere else to say which group it is shown in.
    pub group: Group,
}

/// What the theme called `name` under `root` says about itself, where it
/// came from the registry -- and nothing for a directory that did not.
#[must_use]
pub fn installed(name: &str, root: &Path) -> Option<Installed> {
    let text = std::fs::read_to_string(home(name, root)?.join(RECORD)).ok()?;
    let record: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Installed {
        commit: record.get("commit")?.as_str()?.to_string(),
        sha256: record
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        group: record
            .get("group")
            .and_then(serde_json::Value::as_str)
            .and_then(Group::named)
            .unwrap_or(Group::Obelus),
    })
}

/// Installs a theme off the thread that asked, saying how far it has got
/// and, once, how it went.
///
/// Installing one already installed is updating it: the same steps, and the
/// old one stays until the new one is ready to take its place.
pub fn spawn(theme: &Listed, root: &Path, sender: impl Sink<Event>) {
    let (theme, root) = (theme.clone(), root.to_path_buf());
    obelus_runtime::handle().spawn(async move {
        let failure = install(&theme, &root, &sender).await.err();
        if let Some(why) = &failure {
            tracing::warn!(name = theme.name, why, "a theme did not install");
        }
        let _ = sender.send(Event::Installed {
            name: theme.name,
            failure,
        });
    });
}

/// The whole of an install, for `spawn` to say the end of.
async fn install(theme: &Listed, root: &Path, sender: &impl Sink<Event>) -> Result<(), String> {
    let started = Instant::now();
    let claim = claim(&theme.name, root)?;
    let bytes = obelus_download::bytes(&theme.package, started, |progress| {
        let _ = sender.send(Event::Installing {
            name: theme.name.clone(),
            progress,
        });
    })
    .await?;
    // Hashing, unpacking and renaming are work rather than waiting, so they
    // go where work goes -- with the claim, which is held to the end.
    let (theme, root) = (theme.clone(), root.to_path_buf());
    obelus_runtime::handle()
        .spawn_blocking(move || {
            let settled = settle(&bytes, &theme, &root);
            drop(claim);
            settled
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
}

/// Checks a package, unpacks it beside where it goes, and puts it there.
fn settle(bytes: &[u8], theme: &Listed, root: &Path) -> Result<(), String> {
    if !obelus_download::hashes_to(bytes, &theme.sha256) {
        return Err("It is not the package the registry describes".to_string());
    }
    let into = home(&theme.name, root).ok_or_else(|| {
        format!(
            "{} is not a name Obelus can keep a directory of",
            theme.name
        )
    })?;
    let staging = root.join(format!(".{}.arriving-{}", theme.name, std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| format!("{staging:?}: {error}"))?;
    let settled = unpack_and_read(bytes, theme, &staging)
        .and_then(|arrived| put_in_place(&arrived, &into, root, &theme.name));
    let _ = std::fs::remove_dir_all(&staging);
    settled
}

/// The package unpacked into `staging`, the theme in it found and read, and
/// the record written beside it -- the directory that is the theme.
fn unpack_and_read(bytes: &[u8], theme: &Listed, staging: &Path) -> Result<PathBuf, String> {
    obelus_download::unpack(bytes, &theme.package, staging)?;
    let arrived = theme_in(staging)
        .ok_or_else(|| format!("There is no {} in the package", obelus_theme::written::FILE))?;
    // Read the way the editor will read it, so that what is put in place is
    // a theme the reader can wear rather than a file Obelus refuses the
    // first time it is chosen.
    obelus_theme::written::read_where(&arrived.join(obelus_theme::written::FILE))
        .map_err(|wrong| format!("Obelus cannot read the theme it was sent: {}", wrong.why))?;
    let record = serde_json::json!({
        "commit": theme.commit,
        "sha256": theme.sha256,
        "group": theme.group.word(),
    })
    .to_string();
    std::fs::write(arrived.join(RECORD), record).map_err(|error| format!("{error}"))?;
    Ok(arrived)
}

/// Where in an unpacked package the theme is: at the top, or in the one
/// directory the package holds -- which is how a package made by packing a
/// theme's directory comes out.
fn theme_in(unpacked: &Path) -> Option<PathBuf> {
    if unpacked.join(obelus_theme::written::FILE).is_file() {
        return Some(unpacked.to_path_buf());
    }
    let mut entries = std::fs::read_dir(unpacked).ok()?.filter_map(Result::ok);
    let only = entries.next()?.path();
    (entries.next().is_none() && only.join(obelus_theme::written::FILE).is_file()).then_some(only)
}

/// Puts an arrived theme where it goes, the old one out of the way first
/// and put back if the new one will not go.
fn put_in_place(arrived: &Path, into: &Path, root: &Path, name: &str) -> Result<(), String> {
    let old = root.join(format!(".{name}.leaving-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&old);
    let had = into.exists();
    if had {
        std::fs::rename(into, &old).map_err(|error| format!("{into:?}: {error}"))?;
    }
    if let Err(error) = std::fs::rename(arrived, into) {
        if had {
            let _ = std::fs::rename(&old, into);
        }
        return Err(format!("{into:?}: {error}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Takes away a theme taken from the registry.
///
/// Only one that came from it: a directory with no record is not one the
/// registry put there, and not this to take away.
///
/// # Errors
///
/// When another Obelus is installing it, or the directory will not go.
pub fn remove(name: &str, root: &Path) -> Result<(), String> {
    let _claim = claim(name, root)?;
    let into = home(name, root)
        .ok_or_else(|| format!("{name} is not a name Obelus can keep a directory of"))?;
    if installed(name, root).is_none() {
        return Err(format!("{name} did not come from the registry"));
    }
    std::fs::remove_dir_all(&into).map_err(|error| format!("{into:?}: {error}"))
}

/// A theme this process is installing or taking away, given up by being
/// dropped.
struct Claim {
    path: PathBuf,
    /// The lock itself: held for as long as the file is open, and given up
    /// with the process however it ends.
    _file: std::fs::File,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Holds `name` against the other Obelus processes on the machine.
fn claim(name: &str, root: &Path) -> Result<Claim, String> {
    if home(name, root).is_none() {
        return Err(format!(
            "{name} is not a name Obelus can keep a directory of"
        ));
    }
    std::fs::create_dir_all(root).map_err(|error| format!("{root:?}: {error}"))?;
    let path = root.join(format!(".{name}.installing"));
    let file = std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("{path:?}: {error}"))?;
    if obelus_claim::held_by_somebody_else(&file) {
        return Err(format!("Another Obelus is installing {name}"));
    }
    Ok(Claim { path, _file: file })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::{Group, Listed};

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join(format!("obelus-themes-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory");
        directory
    }

    /// A package, packed the way the registry packs one: the theme's
    /// directory, whole.
    fn package(name: &str, theme: &str) -> Vec<u8> {
        let mut packed = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(theme.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        packed
            .append_data(&mut header, format!("{name}/theme.toml"), theme.as_bytes())
            .expect("the entry");
        packed
            .into_inner()
            .expect("the tar")
            .finish()
            .expect("the gzip")
    }

    fn listed(name: &str, bytes: &[u8], commit: &str) -> Listed {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(bytes);
        Listed {
            name: name.to_string(),
            group: Group::Obelus,
            dark: true,
            author: String::new(),
            description: String::new(),
            wallpaper: None,
            package: format!("https://example.invalid/{name}.tar.gz"),
            sha256: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
            commit: commit.to_string(),
            updated: String::new(),
        }
    }

    fn page(root: &Path, name: &str) -> String {
        std::fs::read_to_string(root.join(name).join("theme.toml")).unwrap_or_default()
    }

    /// A package is put in place whole, with where it came from written
    /// inside it; another commit of it replaces it; and one that is not the
    /// package described, or not a theme Obelus can read, leaves what was
    /// there as it was -- and leaves nothing half unpacked beside it.
    ///
    /// Deliberate breaks: the hash not asked (the wrong package is put in
    /// place), the theme not read before it goes in (the unreadable one is
    /// put in place), and the old one emptied before the new one is ready,
    /// the agents' way (a failed update leaves nothing).
    #[test]
    fn a_theme_goes_in_whole_or_not_at_all() {
        let root = scratch("whole");
        let first = package("rain", "base = \"dark\"\nbackground = \"#101820\"\n");
        // Off omarchy's shelf, which is not what a record that says nothing
        // is taken for -- so the group is what was written, not the default.
        let mut omarchys = listed("rain", &first, "aaa");
        omarchys.group = Group::Omarchy;
        super::settle(&first, &omarchys, &root).expect("installed");
        assert!(page(&root, "rain").contains("#101820"));
        assert_eq!(
            super::installed("rain", &root).map(|record| (record.commit, record.group)),
            Some(("aaa".to_string(), Group::Omarchy))
        );
        assert_eq!(
            super::installed("rain", &root).and_then(|record| record.sha256),
            Some(omarchys.sha256.clone()),
            "the record does not say which package it was"
        );

        let second = package("rain", "base = \"dark\"\nbackground = \"#202830\"\n");
        super::settle(&second, &listed("rain", &second, "bbb"), &root).expect("updated");
        assert!(page(&root, "rain").contains("#202830"));
        assert_eq!(
            super::installed("rain", &root).map(|record| record.commit),
            Some("bbb".to_string())
        );

        let mut wrong = listed("rain", &first, "ccc");
        wrong.sha256 = "0".repeat(64);
        assert!(super::settle(&first, &wrong, &root).is_err());
        let unreadable = package("rain", "this(");
        assert!(super::settle(&unreadable, &listed("rain", &unreadable, "ddd"), &root).is_err());
        assert!(
            page(&root, "rain").contains("#202830"),
            "a failed update took the theme"
        );

        let left: Vec<String> = std::fs::read_dir(&root)
            .expect("the root")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["rain"], "something was left beside it");
    }

    /// Only a theme the registry put there is taken away.
    ///
    /// Deliberate break: the record not asked, which takes away a directory
    /// the reader made.
    #[test]
    fn only_what_the_registry_put_there_is_taken_away() {
        let root = scratch("remove");
        let bytes = package("rain", "base = \"dark\"\n");
        super::settle(&bytes, &listed("rain", &bytes, "aaa"), &root).expect("installed");
        std::fs::create_dir_all(root.join("mine")).expect("the reader's");
        std::fs::write(root.join("mine/theme.toml"), "base = \"dark\"\n").expect("the reader's");

        super::remove("rain", &root).expect("taken away");
        assert!(!root.join("rain").exists());
        assert!(super::remove("mine", &root).is_err());
        assert!(
            root.join("mine").exists(),
            "a theme the registry did not put there went"
        );
    }
}
