//! Snippets: what a server sends when a completion is a shape rather than a
//! word.
//!
//! `println!($1)` is one candidate whose text has a hole in it. The protocol
//! calls the format TextMate's, and the part of it that matters is small:
//! `$1`, `${1:a default}`, `$0` for where to end up, `\$` for a real dollar.
//! Everything else -- variables like `$TM_FILENAME`, choices like
//! `${1|a,b|}`, transformations -- is either something a completion item
//! almost never carries or something Obelus would have to invent an answer
//! for, so it expands to nothing and the text around it survives.
//!
//! What is deliberately not here is *mirroring*: a snippet with `$1` twice
//! does not keep the two copies equal as the reader types. That needs the
//! stops to be written back to on every edit, which is a second mechanism;
//! two `$1`s are two stops in the order they appear, which is wrong in the
//! rare snippet that has them and comprehensible everywhere else.

use obelus_text::coordinates::{CharOffset, End, Replacement};

/// A snippet, expanded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// What goes into the document.
    pub text: String,
    /// Where the reader is sent, in tab order, as character offsets into
    /// [`Parsed::text`].
    pub stops: Vec<(usize, usize)>,
}

/// Expands a snippet into the text it puts in and the stops it leaves.
///
/// The numbering decides the order and the offsets decide nothing: `$2`
/// before `$1` in the source is still the second stop. `$0` is last however
/// it is numbered, because it is not a stop to fill in but the place to be
/// when the filling in is over.
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let mut text = String::new();
    // The number each stop carries, alongside where it landed, so the order
    // can be worked out once every one of them is known.
    let mut stops: Vec<(u32, usize, usize)> = Vec::new();
    let mut characters = source.chars().peekable();

    while let Some(character) = characters.next() {
        // A dollar the snippet means literally.
        if character == '\\'
            && let Some(next) = characters.peek().copied()
            && matches!(next, '$' | '}' | '\\')
        {
            characters.next();
            text.push(next);
            continue;
        }
        if character != '$' {
            text.push(character);
            continue;
        }

        match characters.peek().copied() {
            // `${1:default}`, `${1}`, or `${name}` -- a variable, which
            // expands to nothing.
            Some('{') => {
                characters.next();
                let mut inside = String::new();
                let mut depth = 1usize;
                for character in characters.by_ref() {
                    match character {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    inside.push(character);
                }
                let (name, default) = match inside.split_once(':') {
                    Some((name, default)) => (name, default),
                    None => (inside.as_str(), ""),
                };
                let at = text.chars().count();
                // A default is itself a snippet -- `${1:${2:inner}}` is
                // legal -- and the one thing worth taking from it is its
                // text. Its own stops are dropped: a stop inside a stop is
                // a place to be while already in a place to be.
                let filled = parse(default).text;
                text.push_str(&filled);
                if let Ok(number) = name.parse::<u32>() {
                    stops.push((number, at, at + filled.chars().count()));
                }
            }
            // `$1`, or `$name` for a variable.
            Some(digit) if digit.is_ascii_digit() => {
                let mut number = String::new();
                while let Some(digit) = characters.peek().copied() {
                    if !digit.is_ascii_digit() {
                        break;
                    }
                    number.push(digit);
                    characters.next();
                }
                let at = text.chars().count();
                if let Ok(number) = number.parse::<u32>() {
                    stops.push((number, at, at));
                }
            }
            Some(letter) if letter.is_alphabetic() || letter == '_' => {
                while let Some(letter) = characters.peek().copied() {
                    if !(letter.is_alphanumeric() || letter == '_') {
                        break;
                    }
                    characters.next();
                }
            }
            // A dollar with nothing a snippet would put after it.
            _ => text.push('$'),
        }
    }

    // In tab order: by number, with zero last, and ties in the order they
    // were written. A stable sort is what makes the tie rule hold.
    stops.sort_by_key(|(number, _, _)| match number {
        0 => u32::MAX,
        number => *number,
    });
    Parsed {
        text,
        stops: stops.into_iter().map(|(_, at, end)| (at, end)).collect(),
    }
}

/// The stops of a snippet the reader is filling in.
///
/// Offsets into the document rather than into the snippet, because the
/// moment the text is in, the snippet is gone: what is left is a handful of
/// places in a file that has to go on being edited around them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filling {
    /// Each stop, as it stands now.
    stops: Vec<(CharOffset, CharOffset)>,
    /// Which one the reader is on. `None` before the first tab.
    at: Option<usize>,
}

impl Filling {
    /// The stops of a snippet just put in at `offset`.
    ///
    /// `None` where there is nothing to fill in: a snippet with no stops is
    /// text, and holding a reader in a snippet they cannot move about in
    /// would make `tab` stop indenting for no reason.
    #[must_use]
    pub fn new(offset: CharOffset, stops: &[(usize, usize)]) -> Option<Self> {
        if stops.is_empty() {
            return None;
        }
        Some(Self {
            stops: stops
                .iter()
                .map(|(at, end)| (offset.saturating_add(*at), offset.saturating_add(*end)))
                .collect(),
            at: None,
        })
    }

    /// Where the reader is, if they are on a stop.
    #[must_use]
    pub fn current(&self) -> Option<(CharOffset, CharOffset)> {
        self.stops.get(self.at?).copied()
    }

    /// Moves to the next stop, or says there is none.
    pub fn forward(&mut self) -> Option<(CharOffset, CharOffset)> {
        let next = match self.at {
            None => 0,
            Some(at) => at + 1,
        };
        self.at = Some(next.min(self.stops.len()));
        self.stops.get(next).copied()
    }

    /// And back to the one before.
    pub fn back(&mut self) -> Option<(CharOffset, CharOffset)> {
        let at = self.at?.checked_sub(1)?;
        self.at = Some(at);
        self.stops.get(at).copied()
    }

    /// Whether every stop has been visited.
    ///
    /// What ends the filling in: a `tab` past the last stop is a `tab`, and
    /// a reader who has gone through the holes is typing ordinary text
    /// again.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.at.is_some_and(|at| at >= self.stops.len())
    }

    /// Moves the stops across an edit -- see [`Replacement::carry`].
    ///
    /// The stop being typed into is the case that matters: its finish
    /// follows what is put in, so the hole becomes what was typed.
    pub fn keep_across(&mut self, edit: Replacement) {
        for (start, end) in &mut self.stops {
            *start = edit.carry(*start, End::Start);
            *end = edit.carry(*end, End::Finish);
        }
    }
}
