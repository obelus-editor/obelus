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
    /// Where it points, where it is a link that points somewhere on the
    /// web.
    pub(crate) href: Option<String>,
    /// What a picture's words are, where it has some.
    pub(crate) alt: Option<String>,
}

impl Tag {
    /// What it leaves on the page in its place, if anything.
    ///
    /// A break where it begins or ends a block, because the words either
    /// side of a `</p>` or a `<br>` are two lines; a space before a cell of
    /// a table; a picture's words, which are what a reader without the
    /// picture is given; and nothing for the rest.
    pub(crate) fn leaves(&self) -> Option<String> {
        if self.name == "img" {
            return self.alt.clone().filter(|alt| !alt.trim().is_empty());
        }
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
            alt: None,
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
    if name.is_empty() || !name.starts_with(|first: char| first.is_ascii_alphabetic()) {
        return None;
    }
    let href = attribute(inner, "href")
        .filter(|to| to.starts_with("https://") || to.starts_with("http://"));
    let alt = attribute(inner, "alt");
    Some(Tag {
        range: 0..said.len(),
        name,
        closing,
        href,
        alt,
    })
}

/// The value of one attribute inside a tag, quoted either way.
fn attribute(inner: &str, name: &str) -> Option<String> {
    let lower = inner.to_ascii_lowercase();
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
        let rest = inner[from..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next()?;
        return match quote {
            '"' | '\'' => {
                let rest = &rest[1..];
                Some(rest[..rest.find(quote)?].to_string())
            }
            _ => Some(
                rest.chars()
                    .take_while(|character| !character.is_whitespace() && *character != '>')
                    .collect(),
            ),
        };
    }
    None
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
/// points somewhere and the `</a>` after it, and where it points.
pub(crate) fn links(tags: &[Tag]) -> Vec<(Range<usize>, String)> {
    let mut found = Vec::new();
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
