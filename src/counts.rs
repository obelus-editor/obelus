//! How much code is here, by language and by file.
//!
//! The counting itself is [`tokei`], which does the one hard part properly:
//! a comment marker inside a string literal is not a comment, and a nested
//! block comment ends where it really ends. Obelus's own parsers could be
//! asked the same question -- it has fourteen grammars -- but only for the
//! fourteen, and a reader opening a tree wants the whole of it counted,
//! including the shell script and the lock file that no grammar here knows.
//!
//! What this module adds is the shape the view needs: two orderings of the
//! same walk, biggest first, with the paths written relative to the tree
//! they were found in. Nothing here draws, and nothing here is asked on the
//! main thread -- counting a tree is a walk of every file in it, and the one
//! place that must never happen is the draw path.

use std::path::{Path, PathBuf};

use crate::sink::Sink;

/// Lines of one thing, split the three ways every counter splits them.
///
/// Copy and four words wide, so a row can hold one by value and a total can
/// be added up without a borrow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Lines that are code.
    pub code: usize,
    /// Lines that are comment.
    pub comments: usize,
    /// Lines that are neither.
    pub blanks: usize,
}

impl Tally {
    /// All three added up.
    ///
    /// What the share bar is drawn from, rather than `code`: a file of
    /// documentation counts for nothing under `code`, and a bar that drew it
    /// as nothing would say a file that is mostly prose is not there.
    #[must_use]
    pub const fn lines(self) -> usize {
        self.code + self.comments + self.blanks
    }

    /// Adds another tally into this one.
    pub fn add(&mut self, other: Self) {
        self.code += other.code;
        self.comments += other.comments;
        self.blanks += other.blanks;
    }
}

/// One language found in the tree.
#[derive(Clone, Debug)]
pub struct Language {
    /// What tokei calls it, which is what the row says.
    pub name: &'static str,
    /// How many files of it there are.
    pub files: usize,
    /// One extension it is written with, for the glyph to be looked up by.
    ///
    /// An extension rather than the glyph itself: what a `.rs` file looks
    /// like is [`crate::icons`]'s business, and a model that carried a
    /// codepoint would be the second place that decides it.
    pub extension: Option<&'static str>,
    /// Its own lines, not counting what is embedded in it.
    pub tally: Tally,
    /// Languages written *inside* this one, biggest first.
    ///
    /// Rust's doc comments are Markdown, a Vue file has three languages in
    /// it, and a notebook is mostly not the language it is named after. The
    /// counts are kept apart rather than folded in, because "31 501 lines of
    /// Rust, and 7 036 lines of prose inside it" is two facts and a reader
    /// asking how big a project is wants both of them.
    pub children: Vec<Child>,
}

/// A language embedded in another.
#[derive(Clone, Debug)]
pub struct Child {
    /// What tokei calls it.
    pub name: &'static str,
    /// How many files of the parent language contain any of it.
    pub files: usize,
    /// Its lines, across all of them.
    pub tally: Tally,
}

/// One file.
#[derive(Clone, Debug)]
pub struct File {
    /// Where it is, relative to the tree that was counted.
    pub path: PathBuf,
    /// Which language it was counted as.
    pub language: &'static str,
    /// Its lines, including any embedded in it: a file's size is its size,
    /// and splitting it by which language each line is in is the language
    /// list's job rather than this one's.
    pub tally: Tally,
}

/// A whole tree, counted.
#[derive(Clone, Debug, Default)]
pub struct Counted {
    /// Every language, biggest first.
    pub languages: Vec<Language>,
    /// Every file, biggest first.
    pub files: Vec<File>,
    /// What all of it adds up to.
    pub total: Tally,
}

impl Counted {
    /// Whether the walk found nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The lines of the biggest row on a page, for the bars to be drawn
    /// against.
    ///
    /// The biggest row rather than the total: against the total, every bar in
    /// a project with one dominant language is a single cell, which is a
    /// column of nothing. Against the biggest, the top row is full and the
    /// rest are read off it.
    #[must_use]
    pub fn widest(&self, files: bool) -> usize {
        if files {
            self.files.iter().map(|file| file.tally.lines()).max()
        } else {
            self.languages
                .iter()
                .map(|language| language.tally.lines())
                .max()
        }
        .unwrap_or(0)
    }

    /// Turns tokei's answer into the two orderings the view reads.
    fn from_tokei(languages: &tokei::Languages, root: &Path) -> Self {
        let mut counted = Self::default();

        for (kind, language) in languages.iter() {
            let tally = Tally {
                code: language.code,
                comments: language.comments,
                blanks: language.blanks,
            };
            counted.total.add(tally);

            let mut children: Vec<Child> = language
                .children
                .iter()
                .map(|(kind, reports)| {
                    let mut tally = Tally::default();
                    for report in reports {
                        tally.add(Tally {
                            code: report.stats.code,
                            comments: report.stats.comments,
                            blanks: report.stats.blanks,
                        });
                    }
                    Child {
                        name: kind.name(),
                        files: reports.len(),
                        tally,
                    }
                })
                .collect();
            children.sort_by_key(|child| std::cmp::Reverse(child.tally.lines()));

            counted.languages.push(Language {
                name: kind.name(),
                files: language.reports.len(),
                extension: extension_of(*kind),
                tally,
                children,
            });

            for report in &language.reports {
                counted.files.push(File {
                    // Relative, because every path obelus shows is: an
                    // absolute one in a column is the same prefix repeated
                    // down the screen, pushing the part that differs off the
                    // end of the row.
                    path: report
                        .name
                        .strip_prefix(root)
                        .unwrap_or(&report.name)
                        .to_path_buf(),
                    language: kind.name(),
                    tally: Tally {
                        code: report.stats.code,
                        comments: report.stats.comments,
                        blanks: report.stats.blanks,
                    },
                });
            }
        }

        // Biggest first on both pages. A count read top-down is a list of
        // what this project is mostly made of, which is the question; in the
        // alphabet's order it is a list of names, which is not.
        counted
            .languages
            .sort_by_key(|language| std::cmp::Reverse(language.tally.lines()));
        counted
            .files
            .sort_by_key(|file| std::cmp::Reverse(file.tally.lines()));
        counted
    }
}

/// One extension a language is written with, as tokei lists them.
///
/// The first, which is the usual one: `rs` before whatever else Rust
/// answers to. A language tokei lists no extension for -- a Makefile, a
/// Dockerfile -- has none, and the view falls back to the generic glyph.
fn extension_of(kind: tokei::LanguageType) -> Option<&'static str> {
    tokei::LanguageType::list()
        .iter()
        .find(|(listed, _)| *listed == kind)
        .and_then(|(_, extensions)| extensions.first().copied())
}

/// Counts everything under `root`, on its own thread.
///
/// A thread and an event, like every other source: the walk reads every file
/// in the tree, which on a big one is long enough to drop a frame and on a
/// cold cache is long enough to be noticed. The view opens empty and says so
/// until this lands.
///
/// Nothing cancels it. The walk is one shot per opening of the view rather
/// than one per keystroke, so there is no generation to check and nothing to
/// go stale under a reader who keeps typing -- and a count that arrives after
/// the view has closed is dropped by the handler, which is what every other
/// late answer here does.
pub fn spawn_count(root: &Path, sender: impl Sink<Box<Counted>>) {
    let root = root.to_path_buf();
    crate::runtime::handle().spawn_blocking(move || {
        let mut languages = tokei::Languages::new();
        // No excluded paths of its own: what to leave out is
        // `.gitignore`'s answer, which the walk already obeys, and a
        // second list here would be obelus disagreeing with the file
        // list about what is in the project.
        languages.get_statistics(&[&root], &[], &tokei::Config::default());
        let counted = Counted::from_tokei(&languages, &root);
        tracing::debug!(
            files = counted.files.len(),
            languages = counted.languages.len(),
            code = counted.total.code,
            "counted the tree"
        );
        // The receiver is gone, which means the loop has ended.
        let _ = sender.send(Box::new(counted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts this crate's own `src`, which is the one tree every test
    /// machine has.
    fn count(root: &Path) -> Counted {
        let mut languages = tokei::Languages::new();
        languages.get_statistics(&[root], &[], &tokei::Config::default());
        Counted::from_tokei(&languages, root)
    }

    /// Both orderings, and the paths a row is written with.
    ///
    /// Over the fixtures rather than `src`, because they are written in five
    /// languages and `src` is written in one: an ordering of a list with one
    /// thing in it is every ordering at once, and the test that had it could
    /// not fail.
    ///
    /// Broken deliberately by removing the two `sort_by_key` calls: the
    /// biggest-first assertions failed, because tokei hands its languages
    /// back in a map's order and its reports back in the order the walk
    /// found them.
    #[test]
    fn a_tree_is_counted_biggest_first_with_relative_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let counted = count(&root);
        assert!(
            counted.languages.len() > 1,
            "a tree of one language cannot be in the wrong order"
        );

        assert!(!counted.is_empty(), "nothing was counted under src");
        assert!(
            counted.total.code > 0,
            "a tree of Rust counted as no code at all"
        );

        for pair in counted.languages.windows(2) {
            assert!(
                pair[0].tally.lines() >= pair[1].tally.lines(),
                "{} came before the bigger {}",
                pair[0].name,
                pair[1].name
            );
        }
        for pair in counted.files.windows(2) {
            assert!(
                pair[0].tally.lines() >= pair[1].tally.lines(),
                "{} came before the bigger {}",
                pair[0].path.display(),
                pair[1].path.display()
            );
        }

        // Relative to the tree that was counted, so a row is the part that
        // differs and not the reader's home directory repeated.
        for file in &counted.files {
            assert!(
                file.path.is_relative(),
                "{} is written absolutely",
                file.path.display()
            );
        }
        assert!(
            counted
                .files
                .iter()
                .any(|file| file.path == Path::new("sample.rs")),
            "a fixture was not among the ones counted"
        );
    }

    /// The bars are drawn against the biggest row, not the total.
    ///
    /// Both halves assert the *equality*, not that nothing exceeds it: a sum
    /// is at least as big as every row it is a sum of, so a test that only
    /// asked for an upper bound would pass on the very bug it is here for.
    ///
    /// Broken deliberately by returning the sum instead of the maximum:
    /// `widest` then exceeded the biggest row, which is the shape of the bug
    /// -- every bar on the page a single cell.
    #[test]
    fn the_widest_row_is_what_bars_are_measured_against() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let counted = count(&root);
        assert!(
            counted.languages.len() > 1 && counted.files.len() > 1,
            "one row is its own maximum and its own sum"
        );

        assert_eq!(
            counted.widest(false),
            counted
                .languages
                .first()
                .map_or(0, |language| language.tally.lines()),
            "the widest language is not the biggest one"
        );
        assert_eq!(
            counted.widest(true),
            counted.files.first().map_or(0, |file| file.tally.lines()),
            "the widest file is not the biggest one"
        );
    }

    /// A language written inside another is kept apart from it.
    ///
    /// Broken deliberately by folding the children's lines into the parent's
    /// tally: this crate's Rust then counted its doc comments twice, and the
    /// assertion that the parent's own comments are fewer than the whole
    /// file's failed.
    #[test]
    fn an_embedded_language_is_counted_beside_its_parent_not_inside_it() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let counted = count(&root);

        let rust = counted
            .languages
            .iter()
            .find(|language| language.name == "Rust")
            .expect("no Rust under src");
        let markdown = rust
            .children
            .iter()
            .find(|child| child.name == "Markdown")
            .expect("no Markdown inside the Rust: every module here has doc comments");

        assert!(
            markdown.tally.lines() > 0,
            "the embedded Markdown counted as nothing"
        );
        // The parent's own tally is its own: the child's lines are not in
        // it, so the two added up are more than either.
        assert!(
            rust.tally.lines() > markdown.tally.lines(),
            "the parent is not bigger than what is embedded in it"
        );
    }
}
