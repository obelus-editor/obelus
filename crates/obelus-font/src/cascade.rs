//! What this machine draws a character in when its monospaced face has not
//! got it.
//!
//! The reader's faces and the machine's monospaced one are Latin, almost
//! always, and the rest of what a reader reads -- Chinese in a comment, a
//! Greek letter in a formula -- is drawn in whatever comes after them. That
//! used to be cosmic-text's own fallback, a table it carries of which face
//! each platform has for each script: `PingFang SC` for Chinese on macOS,
//! whichever language the reader actually reads. Every system already has
//! the answer to this, asked of the face and in the reader's languages, and
//! it is what every other program on the screen draws with, so it is asked.
//! **What comes after the monospaced face is the machine's own answer, asked
//! of the machine.**
//!
//! * macOS: CoreText's cascade list for the monospaced face, in the reader's
//!   preferred languages -- which is what puts `PingFang SC` before `PingFang
//!   TC` for a reader who reads simplified Chinese, a question about the same
//!   characters drawn two ways.
//! * Linux: `fc-match -s`, which is fontconfig's sorted list for `monospace` in
//!   the locale's language, pruned to the faces that add something.
//! * Windows: the faces it links to the monospaced one and to `Segoe UI`, which
//!   is the face Windows draws itself in -- after the face for Han in the
//!   reader's own language. `SystemLink` is in the order of the language
//!   Windows was installed in, so an English Windows puts Japanese and
//!   traditional faces before `Microsoft YaHei UI`, and a reader of simplified
//!   Chinese would get the shapes of somebody else's writing.
//!
//! Only names the font database has are kept, and each once. Where the
//! system says nothing, cosmic-text's table is still under all of it.

use std::collections::HashSet;

use cosmic_text::fontdb;

/// The faces this machine falls back to after `monospace`, in its order.
#[must_use]
/// On macOS that is what CoreText said, asked already for the faces it draws
/// as well: see [`crate::coretext::Fallback`].
pub(crate) fn here(
    db: &fontdb::Database,
    monospace: Option<&str>,
    fallback: &crate::coretext::Fallback,
) -> Vec<String> {
    let said = of_this_platform(db, monospace, fallback);
    let found = kept(db, said);
    tracing::info!(faces = found.len(), first = ?found.first(), "what this machine falls back to");
    found
}

/// Each name once, and only those the font database can draw with.
fn kept(db: &fontdb::Database, said: Vec<String>) -> Vec<String> {
    let families: HashSet<String> = db
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.to_lowercase()))
        .collect();
    let mut seen = HashSet::new();
    said.into_iter()
        .filter(|name| {
            let name = name.to_lowercase();
            families.contains(&name) && seen.insert(name)
        })
        .collect()
}

/// macOS asks CoreText.
#[cfg(target_os = "macos")]
fn of_this_platform(
    _: &fontdb::Database,
    _: Option<&str>,
    fallback: &crate::coretext::Fallback,
) -> Vec<String> {
    fallback.families()
}

/// Linux asks fontconfig.
#[cfg(all(unix, not(target_os = "macos")))]
fn of_this_platform(
    _: &fontdb::Database,
    _: Option<&str>,
    _: &crate::coretext::Fallback,
) -> Vec<String> {
    let said = std::process::Command::new("fc-match")
        // One family a line, the first of the names each goes by -- the
        // same one `monospace` takes out of its answer.
        .args(["-s", "--format=%{family[0]}\\n", "monospace"])
        .output()
        .ok()
        .filter(|ran| ran.status.success())
        .map(|ran| String::from_utf8_lossy(&ran.stdout).to_string());
    said.as_deref().map(lines).unwrap_or_default()
}

/// One family a line, as `fc-match` prints them.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn lines(said: &str) -> Vec<String> {
    said.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Windows reads the faces it links to the one asked for, after the one for
/// Han in the reader's language.
#[cfg(windows)]
fn of_this_platform(
    db: &fontdb::Database,
    monospace: Option<&str>,
    _: &crate::coretext::Fallback,
) -> Vec<String> {
    let files = files(db);
    let locale = sys_locale::get_locale().unwrap_or_default();
    let linked = monospace
        .into_iter()
        .chain(std::iter::once("Segoe UI"))
        .flat_map(linked_to)
        .filter_map(|entry| match link(&entry)? {
            Link::Family(name) => Some(name.to_string()),
            Link::File(file) => files.get(&file.to_lowercase()).cloned(),
        });
    han_for(&locale)
        .iter()
        .map(|name| (*name).to_string())
        .chain(linked)
        .collect()
}

/// The face Windows has for Han in the writing of a reader of this language.
///
/// cosmic-text's own table, which is what was drawn with before the machine
/// was asked, and is still what a reader of anything else gets: simplified
/// Chinese.
#[cfg(any(test, windows))]
fn han_for(locale: &str) -> &'static [&'static str] {
    let locale = locale.replace('_', "-").to_lowercase();
    let region = |regions: &[&str]| regions.iter().any(|region| locale.contains(region));
    match locale.split('-').next().unwrap_or_default() {
        "ja" => &["Yu Gothic UI", "Yu Gothic"],
        "ko" => &["Malgun Gothic"],
        "zh" if region(&["-hk", "-mo"]) => &["MingLiU_HKSCS", "Microsoft JhengHei UI"],
        "zh" if region(&["-tw", "-hant"]) => &["Microsoft JhengHei UI"],
        _ => &["Microsoft YaHei UI"],
    }
}

/// Which family is in each file the font database read, by the file's name.
#[cfg(windows)]
fn files(db: &fontdb::Database) -> std::collections::HashMap<String, String> {
    db.faces()
        .filter_map(|face| {
            let path = match &face.source {
                fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path,
                fontdb::Source::Binary(_) => return None,
            };
            let file = path.file_name()?.to_string_lossy().to_lowercase();
            Some((file, face.families.first()?.0.clone()))
        })
        .collect()
}

/// The entries `SystemLink` has for one face, which are its links in order.
#[cfg(windows)]
fn linked_to(face: &str) -> Vec<String> {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_MULTI_SZ, RegGetValueW},
    };

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\FontLink\SystemLink");
    let value = wide(face);
    let read = |data: *mut core::ffi::c_void, size: &mut u32| {
        // SAFETY: both names end in a nul, and `data` is null or `size`
        // bytes long.
        unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_MULTI_SZ,
                std::ptr::null_mut(),
                data,
                size,
            )
        }
    };
    let mut size = 0;
    if read(std::ptr::null_mut(), &mut size) != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2)];
    if read(data.as_mut_ptr().cast(), &mut size) != ERROR_SUCCESS {
        return Vec::new();
    }
    data.truncate(size as usize / 2);
    String::from_utf16_lossy(&data)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// What one `SystemLink` entry names.
#[cfg(any(test, windows))]
#[derive(Debug, PartialEq, Eq)]
enum Link<'a> {
    /// A family, which is what most entries say after the file.
    Family(&'a str),
    /// Only the file, which says which family by what is in it.
    File(&'a str),
}

/// Reads one entry: `MSYH.TTC,Microsoft YaHei UI,128,96` is a file, the
/// family in it, and how much to scale it by, of which the family is the
/// name to ask for and the scale is GDI's business.
#[cfg(any(test, windows))]
fn link(entry: &str) -> Option<Link<'_>> {
    let mut fields = entry.split(',').map(str::trim);
    let file = fields.next().filter(|file| !file.is_empty())?;
    Some(match fields.next().filter(|family| !family.is_empty()) {
        Some(family) => Link::Family(family),
        None => Link::File(file),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A family a line, and nothing for a line with nothing on it.
    ///
    /// Deliberate break: not trimming keeps `Noto Sans CJK SC ` with the
    /// space after it, which is a name no face goes by.
    #[test]
    fn fontconfig_says_a_family_a_line() {
        assert_eq!(
            lines("DejaVu Sans Mono\nNoto Sans CJK SC \n\nNoto Color Emoji\n"),
            ["DejaVu Sans Mono", "Noto Sans CJK SC", "Noto Color Emoji"]
        );
    }

    /// Han is drawn in the reader's own writing: Japanese for Japanese,
    /// traditional for Taiwan and Hong Kong however the locale spells it,
    /// and simplified for everybody else.
    ///
    /// Deliberate breaks: matching the whole locale rather than its
    /// language gives `ja-JP` simplified Chinese; and not reading the script
    /// gives `zh-Hant` simplified too.
    #[test]
    fn han_is_drawn_in_the_readers_writing() {
        assert_eq!(han_for("ja-JP")[0], "Yu Gothic UI");
        assert_eq!(han_for("ko_KR"), ["Malgun Gothic"]);
        assert_eq!(han_for("zh-TW")[0], "Microsoft JhengHei UI");
        assert_eq!(han_for("zh-Hant")[0], "Microsoft JhengHei UI");
        assert_eq!(han_for("zh-HK")[0], "MingLiU_HKSCS");
        assert_eq!(han_for("zh-CN"), ["Microsoft YaHei UI"]);
        assert_eq!(han_for("zh-Hans-CN"), ["Microsoft YaHei UI"]);
        assert_eq!(han_for("en-US"), ["Microsoft YaHei UI"]);
        assert_eq!(han_for(""), ["Microsoft YaHei UI"]);
    }

    /// An entry names its family where it has one, and its file where not.
    ///
    /// Deliberate break: taking the first field asks for a family called
    /// `MSYH.TTC`, which no face is, so every Chinese character on a
    /// Windows machine goes past the face Windows links for it.
    #[test]
    fn a_link_is_the_family_after_the_file() {
        assert_eq!(
            link("MSYH.TTC,Microsoft YaHei UI,128,96"),
            Some(Link::Family("Microsoft YaHei UI"))
        );
        assert_eq!(link("SIMSUN.TTC,SimSun"), Some(Link::Family("SimSun")));
        assert_eq!(link("MALGUN.TTF"), Some(Link::File("MALGUN.TTF")));
        assert_eq!(link(""), None);
    }

    /// Each name once, whatever its case, and only the ones there are faces
    /// for -- in the order the system said them.
    ///
    /// Deliberate breaks: not remembering what was kept leaves the second
    /// `Menlo`, which is a second shaping of every character it has not
    /// got; not asking the database keeps a family nobody has.
    #[test]
    fn a_name_is_kept_once_and_only_where_there_is_a_face() {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../fonts/SymbolsNerdFontMono-Regular.ttf").to_vec());
        let name = crate::SYMBOLS_FAMILY;
        assert_eq!(
            kept(
                &db,
                vec![
                    "Nobody's Face".to_string(),
                    name.to_string(),
                    name.to_uppercase(),
                ]
            ),
            [name]
        );
    }
}
