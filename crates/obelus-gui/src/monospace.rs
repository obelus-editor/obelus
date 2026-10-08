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
//! macOS has the same question with a different name: CoreText's
//! user fixed-pitch face, which is the one the system itself sets code in.
//! It is `Menlo` almost everywhere, and `Menlo` is what is left when the
//! name it answers with is not one the font database has -- a face of the
//! system's own goes by a name starting with a dot, which is a name nothing
//! else can ask for.
//!
//! Windows has no such question: what it has is a setting inside whichever
//! terminal the reader uses, which is not a thing about the machine. So it
//! is the face its own terminal draws in where the machine has it --
//! `Cascadia Mono`, which comes with Windows 11 and with Windows Terminal --
//! and `Consolas`, which every Windows has, where it has not.

use cosmic_text::fontdb;

/// What this machine draws monospaced text in, where it says.
#[must_use]
pub(crate) fn here(db: &fontdb::Database) -> Option<String> {
    let found = of_this_platform(db);
    tracing::info!(?found, "what this machine calls monospaced");
    found
}

/// Whether the font database has a family by this name.
#[cfg(any(target_os = "macos", windows))]
fn has(db: &fontdb::Database, name: &str) -> bool {
    db.faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(name))
    })
}

/// Linux asks fontconfig, which is what every other program here does.
#[cfg(all(unix, not(target_os = "macos")))]
fn of_this_platform(_: &fontdb::Database) -> Option<String> {
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

/// macOS asks CoreText, and is `Menlo` where the answer is no face here.
#[cfg(target_os = "macos")]
fn of_this_platform(db: &fontdb::Database) -> Option<String> {
    use objc2_core_text::{CTFont, CTFontUIFontType};

    // SAFETY: a size of nothing is the face's own size, and no language is
    // the reader's own.
    let said =
        unsafe { CTFont::new_ui_font_for_language(CTFontUIFontType::UserFixedPitch, 0.0, None) }
            // SAFETY: a font CoreText made always has a family.
            .map(|font| unsafe { font.family_name() }.to_string())
            .filter(|name| has(db, name));
    Some(said.unwrap_or_else(|| "Menlo".to_string()))
}

/// Windows has its terminal's face where it has it, and `Consolas`.
#[cfg(windows)]
fn of_this_platform(db: &fontdb::Database) -> Option<String> {
    let face = match has(db, "Cascadia Mono") {
        true => "Cascadia Mono",
        false => "Consolas",
    };
    Some(face.to_string())
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
