//! What this machine calls its monospaced face.
//!
//! A reader who has chosen no faces is not asking for nothing: they are
//! asking for whatever their machine already draws code in. Every other
//! program on a Linux desktop answers that from fontconfig -- the `mono`
//! alias, which is what `fc-match monospace` prints -- and a window that
//! answered it differently would be the one thing on the screen in another
//! face.
//!
//! Which is what happened. cosmic-text resolves `Family::Monospace` from a
//! name it has written into itself:
//!
//! ```text
//! //TODO: configurable default fonts
//! db.set_monospace_family("Noto Sans Mono");
//! ```
//!
//! On the machine this was written on that is a face nobody chose: the
//! desktop's own answer is `JetBrainsMono Nerd Font`, which is what the
//! terminal draws Obelus in a window away. So the platform is asked, and
//! `Family::Monospace` is only what is left when it says nothing.
//!
//! The other two platforms have one answer each and it does not change:
//! macOS ships `Menlo` and Windows ships `Consolas`, and both are what
//! their own terminals use. Neither is asked for at runtime, because
//! neither has a `fc-match` to ask -- what they have is a setting inside
//! whichever terminal the reader uses, which is not a thing about the
//! machine.

/// What this machine draws monospaced text in, where it says.
#[must_use]
pub(crate) fn here() -> Option<String> {
    let found = of_this_platform();
    tracing::info!(?found, "what this machine calls monospaced");
    found
}

/// Linux asks fontconfig, which is what every other program here does.
#[cfg(all(unix, not(target_os = "macos")))]
fn of_this_platform() -> Option<String> {
    let said = std::process::Command::new("fc-match")
        // The family alone, rather than the file and the style `fc-match`
        // prints by default: what is wanted is a name to look up, and the
        // file it happens to live in is not one.
        .args(["--format=%{family}", "monospace"])
        .output()
        .ok()
        .filter(|ran| ran.status.success())
        .map(|ran| String::from_utf8_lossy(&ran.stdout).to_string())?;
    first_family(&said)
}

/// macOS has one, and it is what its own terminal uses.
#[cfg(target_os = "macos")]
fn of_this_platform() -> Option<String> {
    Some("Menlo".to_string())
}

/// So does Windows.
#[cfg(windows)]
fn of_this_platform() -> Option<String> {
    Some("Consolas".to_string())
}

/// The first of the names fontconfig answers with.
///
/// It gives every name the family goes by, comma separated --
/// `JetBrainsMono Nerd Font,JetBrainsMono NF` -- and what a lookup wants is
/// one of them. The first is the one the font calls itself.
#[cfg(not(target_os = "macos"))]
#[cfg(not(windows))]
fn first_family(said: &str) -> Option<String> {
    let first = said.split(',').next()?.trim();
    (!first.is_empty()).then(|| first.to_string())
}

#[cfg(test)]
#[cfg(all(unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    /// One name, out of all the names a family goes by.
    ///
    /// Deliberate break: keeping the whole answer asks for a family called
    /// `JetBrainsMono Nerd Font,JetBrainsMono NF`, which nothing is, so the
    /// face this machine actually uses is never found and every reader who
    /// has chosen nothing gets cosmic-text's own default instead.
    #[test]
    fn one_name_is_taken_out_of_the_names_a_family_goes_by() {
        assert_eq!(
            first_family("JetBrainsMono Nerd Font,JetBrainsMono NF").as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        // A single name, which is what most families answer with.
        assert_eq!(first_family("Iosevka").as_deref(), Some("Iosevka"));
        // And nothing at all, which is fontconfig saying it does not know.
        assert_eq!(first_family(""), None);
        assert_eq!(first_family("  ,x"), None);
    }
}
