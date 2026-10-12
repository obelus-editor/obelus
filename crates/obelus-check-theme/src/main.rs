//! `obelus-check-theme`: whether Obelus can read a theme, and where it
//! cannot.
//!
//! What a theme is put through before the themes registry lists it. Given a
//! theme -- a directory holding a `theme.toml`, or a `.toml` file -- it says
//! every line Obelus could not use, as `file:line:column: why`, the way a
//! compiler does, and fails; and says so and succeeds where there are none.
//!
//! **The same reading the editor does, and nothing of its own.** The words
//! and the places come from [`obelus_theme::written::read_where`], which is
//! what marks a theme file in the editor, and the picture is decoded by
//! [`obelus_theme::wallpaper::decode`], which is what the window draws it
//! from. A check of its own would be a second answer to what Obelus can
//! read, and the first theme it passed that the editor could not read would
//! be a theme a reader downloaded and could not wear.
//!
//! **Only Obelus's own rules.** A colour that is not a colour, a name no
//! colour has, a fit or anchor nobody answers to, a picture that is not
//! there or will not decode, a name a built-in theme already has. How large
//! a picture may be, which licences are taken, what a theme must say about
//! itself: those are the registry's, and kept beside the registry -- an
//! Obelus has to be released for one of its rules to change, and the
//! registry's change when it decides.
//!
//! **A binary of its own, and not in a release.** Nobody but the registry
//! runs it: a reader making a theme sees the same marks on the file in the
//! editor as they make it. So it is built where it is used, from the tag of
//! the Obelus readers have, and every reader's download is not a megabyte
//! larger for a tool one machine runs.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use obelus_theme::{builtin, wallpaper, written};

fn main() -> ExitCode {
    let mut given = std::env::args_os().skip(1);
    let (Some(theme), None) = (given.next(), given.next()) else {
        eprintln!(
            "Usage: obelus-check-theme <a theme: a directory holding {}, or a .toml file>",
            written::FILE
        );
        return ExitCode::from(2);
    };
    let theme = PathBuf::from(theme);
    let said = check(&theme);
    // On the error stream, the one a CI log and a terminal both show and a
    // pipe of the answer does not take.
    for line in &said {
        eprintln!("{line}");
    }
    if said.is_empty() {
        eprintln!("{}: Obelus can read this theme", theme.display());
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every line Obelus could not use in the theme at `given`, one to a line
/// of output, or nothing where there are none.
fn check(given: &Path) -> Vec<String> {
    let Some((name, file)) = theme_at(given) else {
        return vec![format!(
            "{}: No theme is here: a theme is a .toml file, or a directory holding {}",
            given.display(),
            written::FILE
        )];
    };
    let mut wrong: Vec<written::Wrong> = Vec::new();
    // Before what is in it, because a theme by such a name would be a theme
    // standing in front of one every reader already has.
    if builtin::by_name(&name).is_some() || name == builtin::OMARCHY {
        wrong.push(written::Wrong {
            why: format!("A theme built into Obelus is already called `{name}`"),
            at: None,
        });
    }
    match written::read_where(&file) {
        Err(unread) => wrong.push(unread),
        Ok(read) => {
            wrong.extend(read.troubles);
            if let Some(hung) = read.worn.wallpaper {
                // Beside the theme, which a reader's own theme need not be
                // -- it may name a picture anywhere on their machine -- and a
                // theme checked here must: it is about to be packed up and
                // copied somewhere else, and what is outside its directory
                // stays behind, leaving a theme that names a picture nobody
                // who downloads it has.
                if !beside(&file, &hung.file) {
                    wrong.push(written::Wrong {
                        why: "The picture is outside the theme's directory, and is not carried with it"
                            .to_string(),
                        at: read.wallpaper_at,
                    });
                }
                if let Err(error) = wallpaper::decode(&hung.file) {
                    wrong.push(written::Wrong {
                        why: format!(
                            "The picture {} will not decode: {error}",
                            hung.file.display()
                        ),
                        at: read.wallpaper_at,
                    });
                }
            }
        }
    }
    // In the order of the file, the way a reader fixes it: what is about the
    // whole of it first, then down the lines. They arrive in the order they
    // were found, which is the order of the code that found them.
    wrong.sort_by_key(|one| one.at.map(|at| (at.line.get(), at.column.get())));
    wrong.iter().map(|one| line(&file, one)).collect()
}

/// The theme `given` names, by the name it answers to, and the file its
/// colours are in -- the name read the way Obelus reads it, so the check is
/// of the theme a reader would choose.
///
/// The names from the whole path, and the file as it was given: `.` and
/// `theme.toml` said from inside a theme have no name of their own to read,
/// and were taken for no theme at all -- while what is said about the file
/// stays the way the caller wrote it, which in a CI log is relative.
fn theme_at(given: &Path) -> Option<(String, PathBuf)> {
    let whole = std::path::absolute(given).ok()?;
    let whole = whole.components().collect::<PathBuf>();
    if given.is_dir() {
        let file = given.join(written::FILE);
        let name = whole.file_name()?.to_str()?.to_string();
        return file.is_file().then_some((name, file));
    }
    if !given.is_file() || given.extension().is_none_or(|kind| kind != "toml") {
        return None;
    }
    // A directory's own file names the theme by the directory.
    let name = match whole.file_name()?.to_str()? {
        written::FILE => whole.parent()?.file_name()?.to_str()?,
        _ => whole.file_stem()?.to_str()?,
    };
    Some((name.to_string(), given.to_path_buf()))
}

/// Whether `picture` is inside the directory `theme`'s file is in, as the
/// disk has them -- `..` and links followed -- so a path that climbs out
/// and back is inside, and one that reaches out through a link is not.
fn beside(theme: &Path, picture: &Path) -> bool {
    let (Ok(theme), Ok(picture)) = (theme.canonicalize(), picture.canonicalize()) else {
        // A picture that is not there is said by the reading, at its line.
        return true;
    };
    theme
        .parent()
        .is_some_and(|directory| picture.starts_with(directory))
}

/// One thing wrong, where it is: the line and column counted from one, the
/// way a compiler and every editor count them, and the rest of a reason
/// that runs to a second line under the first.
fn line(file: &Path, wrong: &written::Wrong) -> String {
    let why = wrong.why.replace('\n', "\n    ");
    match wrong.at {
        Some(at) => format!(
            "{}:{}:{}: {why}",
            file.display(),
            at.line.get() + 1,
            at.column.get() + 1
        ),
        None => format!("{}: {why}", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// A directory to put a theme in, made fresh for one test.
    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join(format!("obelus-check-theme-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the directory");
        directory
    }

    /// A theme Obelus can read, picture and all, is said to be one.
    ///
    /// Deliberate break: the name checked against the built-in themes the
    /// wrong way round, which refuses every name that is not one.
    #[test]
    fn a_theme_obelus_can_read_passes() {
        let theme = scratch("rain");
        image::RgbaImage::new(4, 4)
            .save(theme.join("rain.png"))
            .expect("the picture");
        std::fs::write(
            theme.join("theme.toml"),
            "base = \"dark\"\nbackground = \"#101820\"\n\n[wallpaper]\nfile = \"rain.png\"\nfit = \"tile\"\n",
        )
        .expect("the theme");
        assert_eq!(super::check(&theme), Vec::<String>::new());
    }

    /// What Obelus could not use is said, each at the line it is on, and a
    /// name a built-in theme has and a picture that will not decode with
    /// them.
    ///
    /// Deliberate breaks: the built-in names not asked (`nord` passes), the
    /// picture not decoded (a file of words passes as one), the line
    /// counted from nought (every place is a line early), a misspelt name
    /// marked at what it is set to rather than at the name (column 13
    /// rather than 1), the sort taken out (line 9 comes before line 3), and
    /// the picture said with no place (it sorts to the top).
    #[test]
    fn what_obelus_cannot_use_is_said_where_it_is() {
        let theme = scratch("nord");
        std::fs::write(theme.join("rain.png"), "not a picture").expect("the picture");
        std::fs::write(
            theme.join("theme.toml"),
            "base = \"dark\"\nbackground = \"blue\"\nbackgound = \"#111111\"\n\n[wallpaper]\nfile = \"rain.png\"\n\
             \n[syntax]\nkeyword = \"nope\"\n",
        )
        .expect("the theme");
        let said = super::check(&theme);
        let file = theme.join("theme.toml").display().to_string();
        assert!(
            said.iter()
                .any(|line| line.contains("already called `nord`")),
            "{said:#?}"
        );
        assert!(
            said.iter()
                .any(|line| line.starts_with(&format!("{file}:2:"))),
            "the colour that is not one, on the second line: {said:#?}"
        );
        assert!(
            said.iter()
                .any(|line| line.starts_with(&format!("{file}:3:1:"))),
            "the misspelt name, at the start of the third: {said:#?}"
        );
        assert!(
            said.iter().any(|line| line.contains("will not decode")),
            "{said:#?}"
        );
        assert_eq!(said.len(), 5, "{said:#?}");
        // In the order of the file, and the picture where it is named: what
        // is about the whole theme, then lines 2, 3, 6 and 9 -- where the
        // colours are read before the names nobody asked for and before
        // the picture, so 9 is found second.
        assert!(said[0].contains("already called"), "{said:#?}");
        assert!(said[1].starts_with(&format!("{file}:2:")), "{said:#?}");
        assert!(said[2].starts_with(&format!("{file}:3:1:")), "{said:#?}");
        assert!(
            said[3].starts_with(&format!("{file}:6:")) && said[3].contains("will not decode"),
            "the picture, at the line that names it: {said:#?}"
        );
        assert!(said[4].starts_with(&format!("{file}:9:")), "{said:#?}");
    }

    /// A theme named from inside it -- `.`, or its `theme.toml` -- is the
    /// theme, by the directory's name, and what is said about it keeps the
    /// path as it was given.
    ///
    /// The only test here that moves the working directory, which none of
    /// the others reads: they name their themes by whole paths.
    ///
    /// Deliberate break: the names read from the path as given, which has
    /// none to read in either case -- both are then "No theme is here".
    #[test]
    fn a_theme_named_from_inside_it_is_the_theme() {
        // A directory of its own, where the other test's `nord` is not.
        let theme = scratch("inside/nord");
        std::fs::write(theme.join("theme.toml"), "background = \"#101820\"\n").expect("the theme");
        let was = std::env::current_dir().expect("the working directory");
        std::env::set_current_dir(&theme).expect("into the theme");
        let here = super::check(std::path::Path::new("."));
        let file = super::check(std::path::Path::new("theme.toml"));
        std::env::set_current_dir(was).expect("back out");
        // Named for the built-in theme so there is something to say, and so
        // that what is said shows the name was read.
        // Joined the way the platform joins them: `.\theme.toml` on Windows.
        let given = std::path::Path::new(".").join("theme.toml");
        assert_eq!(
            here,
            [format!(
                "{}: A theme built into Obelus is already called `nord`",
                given.display()
            )],
            "{here:#?}"
        );
        assert_eq!(file.len(), 1, "{file:#?}");
        assert!(file[0].starts_with("theme.toml: "), "{file:#?}");
    }

    /// A picture from outside the theme's directory is said, because it is
    /// not carried with the theme -- whether it climbs out with `..` or is
    /// named by its whole path -- and one in a directory inside it is not.
    ///
    /// Deliberate break: the question asked of the picture's path as the
    /// file wrote it rather than as the disk has it, which takes the one
    /// climbing out for one inside.
    #[test]
    fn a_picture_outside_the_theme_is_said() {
        let root = scratch("outside");
        let theme = root.join("rain");
        std::fs::create_dir_all(theme.join("pictures")).expect("the directories");
        image::RgbaImage::new(2, 2)
            .save(root.join("away.png"))
            .expect("the picture outside");
        image::RgbaImage::new(2, 2)
            .save(theme.join("pictures/here.png"))
            .expect("the picture inside");
        let file = theme.join("theme.toml");

        std::fs::write(&file, "[wallpaper]\nfile = \"../away.png\"\n").expect("the theme");
        let said = super::check(&theme);
        assert_eq!(said.len(), 1, "{said:#?}");
        assert!(
            said[0].contains("outside the theme's directory"),
            "{said:#?}"
        );

        std::fs::write(&file, "[wallpaper]\nfile = \"pictures/here.png\"\n").expect("the theme");
        assert_eq!(super::check(&theme), Vec::<String>::new());
    }

    /// Something that is not a theme is said to be none, rather than read
    /// as an empty one -- which would pass.
    ///
    /// Deliberate break: a directory taken for a theme whether it holds a
    /// `theme.toml` or not.
    #[test]
    fn what_is_not_a_theme_is_said_to_be_none() {
        let empty = scratch("empty");
        let said = super::check(&empty);
        assert_eq!(said.len(), 1, "{said:#?}");
        assert!(said[0].contains("No theme is here"), "{said:#?}");
    }
}
