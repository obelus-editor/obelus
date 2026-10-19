//! What GitHub draws and the source only spells: HTML, and the shortcodes
//! an emoji is written as.
//!
//! **A tag is the asking, not the words.** A pull request's template and a
//! bot's comment are as often HTML as markdown -- a table to lay out a
//! checklist, a paragraph pushed to the right, a comment telling the author
//! what to write -- and GitHub draws what they ask for. Drawn as written, a
//! page ended in `</td> </tr> </tbody> </table>`. So a tag goes the way the
//! stars round a word in italics go: what it asked for is kept where it can
//! be -- a break where a block ends, a link where it points somewhere, the
//! words of a picture where it has some -- and the rest is not on the page.
//! What is not kept is the look: a row is not drawn on the right because
//! somebody asked for it in HTML.
//!
//! **A comment is not there at all**, which is what it is on GitHub: the
//! notes a template leaves its author.
//!
//! **A shortcode is the emoji it names**, by GitHub's own table, and only
//! where it is words: `:x:` in a span of code is code.
//!
//! **Only what GitHub lets through is a tag.** Its sanitizer keeps a list
//! of names and nothing else ([`ALLOWED`]); anything else between `<` and
//! `>` is words. Which matters most where it is not a page of GitHub's at
//! all: an agent writing `Vec<String>` without its backticks wrote a type,
//! and a tag called `String` taken out of it left `Vec`.
//!
//! **What a tag asks for, it gets where the page already draws it.** Bold,
//! italics, code and a line through are what the stars and backticks ask
//! for and are drawn the same; a heading is a heading, a rule a rule; a
//! picture is its words and a link to it, behind the mark a camera is,
//! because Obelus draws no photographs.

use std::ops::Range;

/// One tag, or one comment, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tag {
    /// Where it is, from its `<` to its `>`.
    pub(crate) range: Range<usize>,
    /// Its name, lowercase and without the slash: `a`, `br`, `td`. Empty for
    /// a comment.
    pub(crate) name: String,
    /// Whether it closes something: `</a>`.
    pub(crate) closing: bool,
    /// Where it points, where it is a link -- or a picture -- that points
    /// somewhere on the web.
    pub(crate) href: Option<String>,
    /// Which of its own bytes are the words a picture is read as, counted
    /// from its `<`: what it says it shows, or where it is where it says
    /// nothing.
    pub(crate) words: Option<Range<usize>>,
}

/// What a tag asks the words between it and its end to look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// `<b>`, `<strong>`.
    Bold,
    /// `<i>`, `<em>`, and the names that mean a word set apart.
    Italic,
    /// `<code>`, `<kbd>`, and the names that mean what a machine says.
    Code,
    /// `<s>`, `<del>`, `<strike>`.
    Struck,
    /// `<h1>` to `<h6>`.
    Heading(u8),
}

/// Every name GitHub's sanitizer keeps -- `github/markup`'s list, and the
/// two a picture with a dark and a light version is written in.
pub(crate) const ALLOWED: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "h7",
    "h8",
    "br",
    "b",
    "i",
    "strong",
    "em",
    "a",
    "pre",
    "code",
    "img",
    "tt",
    "div",
    "ins",
    "del",
    "sup",
    "sub",
    "p",
    "ol",
    "ul",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "blockquote",
    "dl",
    "dt",
    "dd",
    "kbd",
    "q",
    "samp",
    "var",
    "hr",
    "ruby",
    "rt",
    "rp",
    "li",
    "tr",
    "td",
    "th",
    "s",
    "strike",
    "summary",
    "details",
    "caption",
    "figure",
    "figcaption",
    "abbr",
    "bdo",
    "cite",
    "dfn",
    "mark",
    "small",
    "span",
    "time",
    "wbr",
    "picture",
    "source",
];

/// The mark a picture's words are put behind: a camera, which says "a
/// picture was here" in both a terminal and a window, and is two cells in
/// either -- the picture frame some would reach for is one cell where it is
/// drawn as text and two where it is drawn as an emoji.
pub(crate) const PICTURE: &str = "\u{1f4f7} ";

impl Tag {
    /// What of it is taken off the page, as ranges where it is, and what is
    /// put where it was.
    ///
    /// All of it, but for a picture's words: those are bytes of the source
    /// still, so they are words that can be selected and a link can be on.
    pub(crate) fn taken(&self) -> (Vec<Range<usize>>, Option<String>) {
        let at = self.range.start;
        match (&self.words, self.name.as_str()) {
            (Some(words), "img") => (
                vec![at..at + words.start, at + words.end..self.range.end],
                Some(PICTURE.to_string()),
            ),
            _ => (vec![self.range.clone()], self.leaves()),
        }
    }

    /// Whether it is a rule across the page.
    pub(crate) fn is_rule(&self) -> bool {
        self.name == "hr" && !self.closing
    }

    /// What it leaves on the page in its place, if anything.
    ///
    /// A break where it begins or ends a block, because the words either
    /// side of a `</p>` or a `<br>` are two lines; a space before a cell of
    /// a table; and nothing for the rest.
    pub(crate) fn leaves(&self) -> Option<String> {
        // A cell of a table, which is next to the one before it rather than
        // run into it.
        if matches!(self.name.as_str(), "td" | "th") && !self.closing {
            return Some(" ".to_string());
        }
        breaks(&self.name).then(|| "\n".to_string())
    }
}

/// Whether a tag of this name begins or ends a block.
fn breaks(name: &str) -> bool {
    matches!(
        name,
        "br" | "p"
            | "div"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "li"
            | "ul"
            | "ol"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "details"
            | "summary"
            | "hr"
    )
}

/// The tag that `said` is, where it is one: the whole of `said`, from its
/// `<` to its `>`, as the inline grammar hands a tag over.
pub(crate) fn tag(said: &str) -> Option<Tag> {
    if said.starts_with("<!--") {
        return Some(Tag {
            range: 0..said.len(),
            name: String::new(),
            closing: false,
            href: None,
            words: None,
        });
    }
    let inner = said.strip_prefix('<')?.strip_suffix('>')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let name: String = inner
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    // A name GitHub would not keep is not a tag: `Vec<String>` is a type.
    if !ALLOWED.contains(&name.as_str()) {
        return None;
    }
    let web = |to: &str| to.starts_with("https://") || to.starts_with("http://");
    let value = |name: &str| attribute(said, name).map(|range| (range.clone(), &said[range]));
    let (href, words) = match name.as_str() {
        "img" => {
            let src = value("src").filter(|(_, to)| web(to));
            let alt = value("alt").filter(|(_, alt)| !alt.trim().is_empty());
            (
                src.as_ref().map(|(_, to)| (*to).to_string()),
                alt.or(src).map(|(range, _)| range),
            )
        }
        _ => (
            value("href")
                .filter(|(_, to)| web(to))
                .map(|(_, to)| to.to_string()),
            None,
        ),
    };
    Some(Tag {
        range: 0..said.len(),
        name,
        closing,
        href,
        words,
    })
}

/// Where the value of one attribute is inside a tag, quoted either way,
/// counted from the tag's `<`.
fn attribute(said: &str, name: &str) -> Option<Range<usize>> {
    let lower = said.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..)?.find(name) {
        let at = from + at;
        from = at + name.len();
        // A whole attribute's name, not the end of another one's.
        if lower[..at]
            .chars()
            .last()
            .is_some_and(|before| !before.is_whitespace())
        {
            continue;
        }
        let after = &said[from..];
        let rest = after.trim_start().strip_prefix('=')?;
        let rest_at = said.len() - rest.len();
        let value = rest.trim_start();
        let value_at = rest_at + (rest.len() - value.len());
        let quote = value.chars().next()?;
        return match quote {
            '"' | '\'' => {
                let start = value_at + 1;
                Some(start..start + said[start..].find(quote)?)
            }
            _ => Some(
                value_at
                    ..value_at
                        + value
                            .find(|character: char| character.is_whitespace() || character == '>')
                            .unwrap_or(value.len()),
            ),
        };
    }
    None
}

/// What each pair of tags that asks for a look asks for, over the words
/// between them -- an open and the close of the same name after it, inner
/// pairs inside outer ones.
pub(crate) fn styles(tags: &[Tag]) -> Vec<(Range<usize>, Style)> {
    let style = |name: &str| match name {
        "b" | "strong" => Some(Style::Bold),
        "i" | "em" | "cite" | "var" | "dfn" => Some(Style::Italic),
        "code" | "kbd" | "tt" | "samp" => Some(Style::Code),
        "s" | "strike" | "del" => Some(Style::Struck),
        "h1" => Some(Style::Heading(1)),
        "h2" => Some(Style::Heading(2)),
        "h3" => Some(Style::Heading(3)),
        "h4" => Some(Style::Heading(4)),
        "h5" => Some(Style::Heading(5)),
        "h6" => Some(Style::Heading(6)),
        _ => None,
    };
    let mut open: Vec<(&str, usize, Style)> = Vec::new();
    let mut found = Vec::new();
    for tag in tags {
        let Some(looks) = style(&tag.name) else {
            continue;
        };
        match tag.closing {
            false => open.push((&tag.name, tag.range.end, looks)),
            true => {
                if let Some(at) = open.iter().rposition(|(name, ..)| *name == tag.name) {
                    let (_, from, looks) = open.remove(at);
                    if from < tag.range.start {
                        found.push((from..tag.range.start, looks));
                    }
                }
            }
        }
    }
    found
}

/// Every tag and comment in a block of HTML, in order.
///
/// A block the grammar has already said is HTML, so a `<` that begins a
/// name, a slash or a comment is a tag; one that does not -- `a < b` -- is
/// text.
pub(crate) fn tags(text: &str) -> Vec<Tag> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find('<')) {
        let start = from + at;
        let rest = &text[start..];
        let end = match rest.starts_with("<!--") {
            true => rest.find("-->").map(|end| end + 3),
            false => rest.find('>').map(|end| end + 1),
        };
        let Some(end) = end else {
            break;
        };
        match tag(&rest[..end]) {
            Some(mut tag) => {
                tag.range = start..start + end;
                found.push(tag);
                from = start + end;
            }
            None => from = start + 1,
        }
    }
    found
}

/// Every link a block's tags make: the words between an `<a href>` that
/// points somewhere and the `</a>` after it, and a picture's own words,
/// which go to the picture.
pub(crate) fn links(tags: &[Tag]) -> Vec<(Range<usize>, String)> {
    let mut found: Vec<(Range<usize>, String)> = tags
        .iter()
        .filter(|tag| tag.name == "img")
        .filter_map(|tag| {
            let words = tag.words.as_ref()?;
            let to = tag.href.clone()?;
            Some((
                tag.range.start + words.start..tag.range.start + words.end,
                to,
            ))
        })
        .collect();
    for (at, open) in tags.iter().enumerate() {
        let (false, "a", Some(to)) = (open.closing, open.name.as_str(), open.href.as_ref()) else {
            continue;
        };
        if let Some(close) = tags[at + 1..]
            .iter()
            .find(|tag| tag.closing && tag.name == "a")
            && open.range.end < close.range.start
        {
            found.push((open.range.end..close.range.start, to.clone()));
        }
    }
    found
}

/// Every shortcode in a text that names an emoji, and the emoji.
///
/// `:` a name `:`, the name in the letters, digits and marks GitHub's table
/// writes them in. A pair of colons round anything else -- a time, a path
/// on Windows, Rust's `::` -- is left as it is.
pub(crate) fn shortcodes(text: &str) -> Vec<(Range<usize>, &'static str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find(':')) {
        let start = from + at;
        let rest = &text[start + 1..];
        let name: &str = &rest[..rest
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '+' | '-'))
            })
            .unwrap_or(rest.len())];
        let closed = rest[name.len()..].starts_with(':');
        match (closed, emojis::get_by_shortcode(name)) {
            (true, Some(emoji)) if !name.is_empty() => {
                let end = start + 1 + name.len() + 1;
                found.push((start..end, emoji.as_str()));
                from = end;
            }
            _ => from = start + 1,
        }
    }
    found
}

/// Every character reference HTML writes a character as, and the
/// character: the handful a block of HTML is written with, and a number.
pub(crate) fn references(text: &str) -> Vec<(Range<usize>, String)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find('&')) {
        let start = from + at;
        let rest = &text[start + 1..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            from = start + 1;
            continue;
        };
        let name = &rest[..end];
        let character = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    name.strip_prefix('#')
                        .and_then(|decimal| decimal.parse().ok())
                })
                .and_then(char::from_u32),
        };
        match character {
            Some(character) => {
                found.push((start..start + 1 + end + 1, character.to_string()));
                from = start + 1 + end + 1;
            }
            None => from = start + 1,
        }
    }
    found
}
