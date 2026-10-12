//! A theme read from a file.
//!
//! Every colour is optional and whatever is not given comes from a theme
//! that is compiled in. A file that had to name all thirty-seven would be a
//! format nobody writes by hand and a template nobody can generate: the
//! thing generating one -- a desktop that themes every program it has --
//! knows a dozen colours by a semantic name and nothing at all about which
//! of them a comment should be.
//!
//! It is also what keeps old files working. A colour added to [`Theme`]
//! later is a colour every file on disk is silently already answering,
//! because the base answers it.

use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
};

use ratatui::style::Color;

use crate::{
    SyntaxTheme, Theme, Worn, builtin,
    wallpaper::{self, Anchor, Fit, Wallpaper},
};

/// The directory a layer's themes live in, beside its settings file.
#[must_use]
pub fn beside(settings: &Path) -> Option<PathBuf> {
    Some(settings.parent()?.join("themes"))
}

/// What the colours are called inside a theme that is a directory.
pub const FILE: &str = "theme.toml";

/// The themes in a directory, by the name each answers to.
///
/// The file's own stem, so `omarchy.toml` is `omarchy`: a name written
/// inside the file would be a name that can disagree with the one the
/// reader typed into their settings. And a directory holding a
/// `theme.toml` is a theme by the directory's name, which is how a theme
/// keeps a picture beside its colours: the picture is named from inside the
/// file, and a directory is what keeps the two together when it is copied.
#[must_use]
pub fn found_in(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    // With whether each is a file, which decides a name both answer to.
    let mut found: Vec<(String, bool, PathBuf)> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        // Not what is hidden: a theme on its way in is unpacked beside where
        // it goes, under a name starting with a dot, and half of one is not
        // a theme to offer.
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .filter_map(|path| {
            if path.extension().is_some_and(|kind| kind == "toml") {
                let name = path.file_stem()?.to_str()?.to_string();
                return Some((name, true, path));
            }
            let inside = path.join(FILE);
            if !inside.is_file() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            Some((name, false, inside))
        })
        .collect();
    // By name, because a directory hands them over in whatever order it
    // holds them and a list that reorders itself between openings cannot be
    // learned. And one theme to a name: where `rain.toml` and `rain/` are
    // both there, the directory, which is the form that can carry a picture
    // -- the order the directory listed them in was the answer before,
    // which is no answer at all.
    found.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    found.dedup_by(|later, earlier| later.0 == earlier.0);
    found
        .into_iter()
        .map(|(name, _, path)| (name, path))
        .collect()
}

/// Reads one, over whichever built-in theme it says to build on, saying
/// where in the file the trouble is.
///
/// Where, because a theme file is a file the reader wrote and can be shown
/// the line of -- which is worth more than a sentence naming the file they
/// are already looking at.
///
/// # Errors
///
/// When the file cannot be read or is not TOML.
pub fn read_where(path: &Path) -> std::result::Result<Read, Wrong> {
    let said = std::fs::read_to_string(path).map_err(|error| Wrong {
        why: format!("reading {}: {error}", path.display()),
        at: None,
    })?;
    let table: toml::Table = said.parse().map_err(|error: toml::de::Error| Wrong {
        why: error.to_string(),
        at: error
            .span()
            .map(|bytes| obelus_text::span_of_bytes(&said, &bytes)),
    })?;
    let spans = Spans::of(&said);
    let (colours, noted) = over_noting(&table);
    let mut troubles: Vec<Wrong> = noted
        .iter()
        .map(|note| {
            let keys: Vec<&str> = note.keys.iter().map(String::as_str).collect();
            match note.name {
                true => spans.named(&keys, note.why),
                false => spans.wrong(&keys, note.why),
            }
        })
        .collect();
    let wallpaper = hung(path, &spans, &table, &mut troubles);
    Ok(Read {
        worn: Worn { colours, wallpaper },
        troubles,
        wallpaper_at: spans.span(&["wallpaper", "file"], false),
    })
}

/// What a theme file turned out to say.
#[derive(Clone, Debug)]
pub struct Read {
    /// The theme, picture and all.
    pub worn: Worn,
    /// What in it Obelus could not use, each left at what it would have
    /// been without the line.
    ///
    /// The file still read, so these are a reader's to fix and not a reason
    /// to refuse the theme: a picture put in the middle because its anchor
    /// was misspelled is on a screen the reader can still use to fix it.
    pub troubles: Vec<Wrong>,
    /// Where the picture is named, for whatever finds the picture itself
    /// wrong -- that it will not decode is learned by decoding it, which
    /// reading the file does not do.
    pub wallpaper_at: Option<obelus_text::coordinates::Span>,
}

/// The picture a table names in its `[wallpaper]`.
///
/// Nothing where there is no such table, and nothing where it names no
/// picture Obelus can find -- which is said, because a theme whose picture
/// has gone looks, without a word, exactly like a theme that never had one.
/// What a `[wallpaper]` table may say: the four this reads, and the two the
/// themes registry reads beside them -- where the picture came from and on
/// what terms -- which Obelus keeps no use for but which are not mistakes.
const WALLPAPER: [&str; 6] = ["file", "opacity", "fit", "anchor", "license", "source"];

fn hung(
    path: &Path,
    spans: &Spans<'_>,
    table: &toml::Table,
    troubles: &mut Vec<Wrong>,
) -> Option<Wallpaper> {
    let hung = table.get("wallpaper")?;
    let Some(hung) = hung.as_table() else {
        troubles.push(spans.wrong(&["wallpaper"], "A wallpaper is a table of its own"));
        return None;
    };
    // A name nothing reads, before anything is read: a misspelt `opacity`
    // is the most likely line in the file to be wrong, and the quietest --
    // the picture is simply covered at the strength it would have been.
    for key in hung.keys().filter(|key| !WALLPAPER.contains(&key.as_str())) {
        troubles.push(spans.named(
            &["wallpaper", key],
            "Nothing about a wallpaper is called this",
        ));
    }
    let Some(named) = hung.get("file") else {
        troubles.push(spans.wrong(&["wallpaper"], "This wallpaper names no file"));
        return None;
    };
    let Some(named) = named.as_str() else {
        troubles.push(spans.wrong(&["wallpaper", "file"], "A file is named in quotes"));
        return None;
    };
    // Beside the theme as it really is: a theme file is often a link into a
    // directory something else owns -- a desktop that themes every program
    // it has -- and its picture is beside the file it was written with, not
    // beside the link.
    let really = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let file = really
        .parent()
        .map_or_else(|| PathBuf::from(named), |beside| beside.join(named));
    let Ok(about) = std::fs::metadata(&file) else {
        troubles.push(spans.wrong(
            &["wallpaper", "file"],
            &format!("There is no picture at {}", file.display()),
        ));
        return None;
    };

    let opacity = match hung.get("opacity") {
        None => wallpaper::OPACITY,
        Some(value) => match value
            .as_integer()
            .and_then(|number| u8::try_from(number).ok())
        {
            Some(opacity) if opacity <= 100 => opacity,
            _ => {
                troubles.push(spans.wrong(
                    &["wallpaper", "opacity"],
                    &format!(
                        "An opacity is a whole number from 0 to 100, so this is {}",
                        wallpaper::OPACITY
                    ),
                ));
                wallpaper::OPACITY
            }
        },
    };
    let fit = match hung.get("fit") {
        None => Fit::default(),
        Some(value) => value.as_str().and_then(Fit::named).unwrap_or_else(|| {
            troubles.push(spans.wrong(
                &["wallpaper", "fit"],
                &format!(
                    "No fit is called this, so it is cover\nOne of {}",
                    words(&Fit::ALL)
                ),
            ));
            Fit::default()
        }),
    };
    let anchor = match hung.get("anchor") {
        None => Anchor::default(),
        Some(value) => value.as_str().and_then(Anchor::named).unwrap_or_else(|| {
            troubles.push(spans.wrong(
                &["wallpaper", "anchor"],
                &format!(
                    "No anchor is called this, so it is center\nOne of {}",
                    words(&Anchor::ALL)
                ),
            ));
            Anchor::default()
        }),
    };
    Some(Wallpaper {
        file,
        opacity,
        fit,
        anchor,
        modified: about.modified().ok(),
    })
}

/// The words a list answers to, for a sentence saying which they are.
fn words<T>(list: &[(&str, T)]) -> String {
    list.iter()
        .map(|(word, _)| *word)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Where things are in a theme file, for saying which line is wrong.
///
/// A second parse, by the crate that keeps spans, for the reason the
/// settings have one: the parse the values come from throws them away.
struct Spans<'a> {
    said: &'a str,
    document: Option<toml_edit::ImDocument<&'a str>>,
}

impl<'a> Spans<'a> {
    fn of(said: &'a str) -> Self {
        Self {
            said,
            document: toml_edit::ImDocument::parse(said).ok(),
        }
    }

    /// A trouble at a value, or at its key where the value has no span of
    /// its own -- a table does not -- and at nothing where the file has
    /// neither, which is a mark not made rather than one made in the wrong
    /// place.
    fn wrong(&self, keys: &[&str], why: &str) -> Wrong {
        Wrong {
            why: why.to_string(),
            at: self.span(keys, false),
        }
    }

    /// A trouble with a name, at the name: what is misspelt is the word the
    /// reader wrote on the left, not what they set it to.
    fn named(&self, keys: &[&str], why: &str) -> Wrong {
        Wrong {
            why: why.to_string(),
            at: self.span(keys, true),
        }
    }

    fn span(&self, keys: &[&str], name: bool) -> Option<obelus_text::coordinates::Span> {
        let document = self.document.as_ref()?;
        let (last, path) = keys.split_last()?;
        let mut table: &dyn toml_edit::TableLike = document.as_table();
        for key in path {
            table = table.get(key)?.as_table_like()?;
        }
        let key = || table.key(last).and_then(toml_edit::Key::span);
        let value = || {
            table
                .get(last)
                .and_then(toml_edit::Item::as_value)
                .and_then(toml_edit::Value::span)
        };
        let bytes = match name {
            true => key().or_else(value)?,
            false => value().or_else(key)?,
        };
        Some(obelus_text::span_of_bytes(self.said, &bytes))
    }
}

/// Why a theme file would not read, and where.
#[derive(Clone, Debug)]
pub struct Wrong {
    /// What went wrong, in the words it arrived in.
    pub why: String,
    /// Where in the file, where the file was the trouble at all -- a file
    /// whose permissions forbid it has no line that is wrong with it.
    pub at: Option<obelus_text::coordinates::Span>,
}

/// The theme a table describes, over the one it builds on.
#[must_use]
pub fn over(table: &toml::Table) -> Theme {
    over_noting(table).0
}

/// A line of a theme file that did nothing.
struct Note {
    /// Where it is, by the keys that lead to it.
    keys: Vec<String>,
    /// What Obelus could not make of it.
    why: &'static str,
    /// Whether what is wrong is the name rather than what it is set to: a
    /// misspelling is marked on the word that is misspelt.
    name: bool,
}

/// Every line of a theme file that did nothing.
type Noted = Vec<Note>;

/// The theme a table describes, and every line of it that did nothing: a
/// value that is not a colour, a name no colour has, a base that is no
/// built-in theme.
///
/// What a name is, is asked of the reading itself: every key looked for is
/// written down as it is looked for, and a key in the file nobody looked for
/// is one nothing reads -- a misspelling, as silent as leaving the colour
/// out. A list of the names kept beside this would be a second answer to
/// which colours there are, and the first colour added to the theme and
/// not to the list would be marked as a misspelling in every file that set
/// it.
fn over_noting(table: &toml::Table) -> (Theme, Noted) {
    let asked: RefCell<HashSet<(&str, &str)>> = RefCell::new(HashSet::new());
    let noted: RefCell<Noted> = RefCell::new(Vec::new());
    // Named `base` rather than `mode`, because it names a theme rather than
    // a kind of one: what a file inherits is thirty-seven colours, and which
    // thirty-seven is the whole of what this says.
    let base = match table.get("base") {
        None => &builtin::DARK,
        Some(said) => match said.as_str().and_then(builtin::by_name) {
            Some(base) => base,
            None => {
                noted.borrow_mut().push(Note {
                    keys: vec!["base".to_string()],
                    why: "No built-in theme is called this, so it builds on dark",
                    name: false,
                });
                &builtin::DARK
            }
        },
    };
    let syntax = table.get("syntax").and_then(toml::Value::as_table);
    let terminal = table.get("terminal").and_then(toml::Value::as_table);
    let read = |section: &'static str, within: Option<&toml::Table>, key: &'static str| {
        asked.borrow_mut().insert((section, key));
        let said = within?.get(key)?;
        let read = said.as_str().and_then(hex);
        if read.is_none() {
            let mut at: Vec<String> = Vec::new();
            if !section.is_empty() {
                at.push(section.to_string());
            }
            at.push(key.to_string());
            noted.borrow_mut().push(Note {
                keys: at,
                why: "Not a colour, so it is the one this builds on\nA colour is written #rrggbb or #rgb",
                name: false,
            });
        }
        read
    };
    let of = |key: &'static str, fallback: Color| read("", Some(table), key).unwrap_or(fallback);
    let syn = |key: &'static str, fallback: Color| read("syntax", syntax, key).unwrap_or(fallback);

    // A field at a time rather than a loop over names, so a colour added to
    // the theme and not read here does not compile. The same rule the
    // settings follow, and the reason both are this long.
    let theme = Theme {
        background: of("background", base.background),
        foreground: of("foreground", base.foreground),
        gutter: of("gutter", base.gutter),
        gutter_current: of("gutter_current", base.gutter_current),
        scrollbar_track: of("scrollbar_track", base.scrollbar_track),
        status_foreground: of("status_foreground", base.status_foreground),
        status_stale: of("status_stale", base.status_stale),
        selected_row_background: of("selected_row_background", base.selected_row_background),
        raised_background: of("raised_background", base.raised_background),
        picker_match_background: of("picker_match_background", base.picker_match_background),
        marked_background: of("marked_background", base.marked_background),
        selection_background: of("selection_background", base.selection_background),
        change_added: of("change_added", base.change_added),
        change_modified: of("change_modified", base.change_modified),
        change_removed: of("change_removed", base.change_removed),
        change_added_background: of("change_added_background", base.change_added_background),
        change_modified_background: of(
            "change_modified_background",
            base.change_modified_background,
        ),
        change_removed_background: of("change_removed_background", base.change_removed_background),
        bracket_background: of("bracket_background", base.bracket_background),
        syntax: SyntaxTheme {
            attribute: syn("attribute", base.syntax.attribute),
            boolean: syn("boolean", base.syntax.boolean),
            comment: syn("comment", base.syntax.comment),
            constant: syn("constant", base.syntax.constant),
            constructor: syn("constructor", base.syntax.constructor),
            escape: syn("escape", base.syntax.escape),
            function: syn("function", base.syntax.function),
            keyword: syn("keyword", base.syntax.keyword),
            label: syn("label", base.syntax.label),
            number: syn("number", base.syntax.number),
            operator: syn("operator", base.syntax.operator),
            property: syn("property", base.syntax.property),
            punctuation: syn("punctuation", base.syntax.punctuation),
            string: syn("string", base.syntax.string),
            type_name: syn("type_name", base.syntax.type_name),
            variable: syn("variable", base.syntax.variable),
            error: syn("error", base.syntax.error),
            warning: syn("warning", base.syntax.warning),
        },
        // What the file names, and what the base names where it does not:
        // a colour left to Obelus stays left, so it is worked out from
        // *this* theme's colours rather than from the base's.
        terminal: std::array::from_fn(|index| {
            read("terminal", terminal, crate::TERMINAL_NAMES[index]).or(base.terminal[index])
        }),
    };

    // And what nobody asked for. A line that will not do is a line that did
    // nothing, and is said, rather than the theme thrown away: the rest of
    // the file is still colours, and a theme refused over one typo is a
    // screen the reader cannot use to find the typo.
    let asked = asked.into_inner();
    let mut noted = noted.into_inner();
    for (key, said) in table {
        match key.as_str() {
            // Read elsewhere: the base above, the picture by `hung`, and
            // what a theme says about itself by whatever lists it.
            "base" | "wallpaper" | "about" => {}
            section @ ("syntax" | "terminal") => match said.as_table() {
                Some(within) => noted.extend(
                    within
                        .keys()
                        .filter(|key| !asked.contains(&(section, key.as_str())))
                        .map(|key| Note {
                            keys: vec![section.to_string(), key.clone()],
                            why: "No colour here is called this",
                            name: true,
                        }),
                ),
                None => noted.push(Note {
                    keys: vec![section.to_string()],
                    why: "These colours are a table of their own",
                    name: false,
                }),
            },
            key if !asked.contains(&("", key)) => noted.push(Note {
                keys: vec![key.to_string()],
                why: "No colour in a theme is called this",
                name: true,
            }),
            _ => {}
        }
    }
    (theme, noted)
}

/// `#rrggbb`, which is how every palette anybody has is written down.
///
/// The three-digit form too, because it is what a person types. Nothing
/// else: a named colour would be the terminal's sixteen, and a theme that
/// reached for those could not promise the same file looks the same twice.
#[must_use]
pub fn hex(said: &str) -> Option<Color> {
    let digits = said.strip_prefix('#')?;
    // Asked before anything is cut out of it: the cutting below is by byte,
    // and a character of more than one byte among six of them was a cut
    // through the middle of it -- a panic, in the editor wearing the theme
    // and in the check a theme is put through before anybody wears it.
    if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let (red, green, blue) = match digits.len() {
        3 => {
            let mut characters = digits.chars();
            let mut doubled = || {
                let digit = characters.next()?.to_digit(16)?;
                u8::try_from(digit * 16 + digit).ok()
            };
            (doubled()?, doubled()?, doubled()?)
        }
        6 => (
            u8::from_str_radix(&digits[0..2], 16).ok()?,
            u8::from_str_radix(&digits[2..4], 16).ok()?,
            u8::from_str_radix(&digits[4..6], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Color::Rgb(red, green, blue))
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    /// A theme on its way in, unpacked under a hidden name beside where it
    /// goes, is not offered as one.
    ///
    /// Deliberate break: what is hidden not left out, which offers
    /// `.rain.arriving-1` beside `rain` for as long as an install takes.
    #[test]
    fn what_is_hidden_is_not_a_theme() {
        let root = std::env::temp_dir().join(format!("obelus-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["rain", ".rain.arriving-1"] {
            std::fs::create_dir_all(root.join(name)).expect("the directory");
            std::fs::write(root.join(name).join("theme.toml"), "base = \"dark\"\n")
                .expect("the theme");
        }
        let names: Vec<String> = super::found_in(&root)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["rain"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A colour is six or three hexadecimal digits and nothing else: a
    /// character of several bytes among them is not one, and is said not
    /// to be rather than cut through -- and nor is the sign a number may
    /// carry.
    ///
    /// Deliberate break: the question about the digits taken out, which
    /// panics on the first and reads the second as a colour.
    #[test]
    fn a_colour_is_hexadecimal_digits_and_nothing_else() {
        assert_eq!(super::hex("#a€bc"), None);
        assert_eq!(super::hex("#+1+1+1"), None);
        assert_eq!(super::hex("#€"), None);
        assert_eq!(super::hex("#1a2B3c"), Some(Color::Rgb(0x1a, 0x2b, 0x3c)));
    }

    /// Every line that does nothing is noted where it is -- a value that is
    /// not a colour, a name no colour has at the top and in each table, a
    /// base that is no built-in theme -- and nothing else is: the colours
    /// that are colours, and the tables something else reads.
    ///
    /// Deliberate breaks: no walk for what nobody asked for (only the bad
    /// value and the base are noted), and the names asked for not written
    /// down as they are asked (every colour in the file is then a
    /// misspelling).
    #[test]
    fn a_line_that_does_nothing_is_noted_where_it_is() {
        let table: toml::Table =
            "base = \"dusk\"\nbackground = \"blue\"\nbackgound = \"#111111\"\n\
             foreground = \"#abcdef\"\n\
             [syntax]\nkeywrd = \"#123456\"\ncomment = \"#654321\"\n\
             [terminal]\nred = \"#123456\"\npurple = \"#123456\"\n\
             [wallpaper]\nfile = \"rain.jpg\"\n[about]\nauthor = \"someone\"\n"
                .parse()
                .expect("a table");
        let (theme, noted) = super::over_noting(&table);
        let mut at: Vec<String> = noted.iter().map(|note| note.keys.join(".")).collect();
        at.sort();
        assert_eq!(
            at,
            [
                "backgound",
                "background",
                "base",
                "syntax.keywrd",
                "terminal.purple"
            ]
        );
        // And what could be used, was.
        assert_eq!(theme.foreground, Color::Rgb(0xab, 0xcd, 0xef));
        assert_eq!(theme.syntax.comment, Color::Rgb(0x65, 0x43, 0x21));
    }

    /// A terminal colour a file names is the file's, and one it does not is
    /// worked out from the file's own colours -- not from the base it built
    /// on, which would put the base's blue beside the file's.
    ///
    /// Broken deliberately twice: `over` ignoring `[terminal]` (red is the
    /// worked-out one instead), and taking the unnamed ones from the base
    /// (blue is the base's function colour).
    #[test]
    fn a_terminal_colour_is_the_files_or_worked_out_from_it() {
        let table: toml::Table = "base = \"dark\"\n[syntax]\nfunction = \"#abcdef\"\n\
                                  [terminal]\nred = \"#123456\"\n"
            .parse()
            .expect("a table");
        let theme = super::over(&table);
        assert_eq!(theme.terminal_colour(1), Some(Color::Rgb(0x12, 0x34, 0x56)));
        assert_eq!(theme.terminal_colour(4), Some(Color::Rgb(0xab, 0xcd, 0xef)));
        assert_eq!(theme.terminal_colour(7), Some(theme.foreground));
        // Past the sixteen, a colour is exactly what it says.
        assert_eq!(theme.terminal_colour(16), None);
    }
}
