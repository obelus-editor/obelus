//! Reading a log, which is a file with a shape rather than a language.
//!
//! A log has no grammar to parse and does not want one: every line is one
//! entry, every entry has the same parts in the same order, and what a
//! reader wants is those parts in *columns* -- the levels down one edge and
//! the messages starting at the same place, so a screenful can be skimmed
//! instead of read. That is a reading of the bytes, like markdown's, so it
//! is a buffer's `Mode::Preview` rather than a
//! colouring of the text.
//!
//! Four formats, tried in order, because a log file does not say which it
//! is. Three are written out here -- Obelus's own, and syslog's two -- and
//! they are written out because each is a fixed run of fields with fixed
//! separators, which is less code than reading somebody else's parser. The
//! fourth is `access_log_parser`, which is a real parser of a real format
//! and earns its place: an access log's fields are typed, and its status
//! code is the one thing a reader of one is looking for.
//!
//! A line no format claims is kept as it is. That is what makes the reading
//! safe to try on anything: an unknown file reads as it did before, and the
//! columns appear only where something knew what the line was.
//!
//! A log's format is decided by its lines, not its name. `syslog`,
//! `access.log`, `obelus.2026-09-11.log` -- the extension says nothing, so
//! `log::format_of` offers the first twenty lines to each format and takes the
//! one that claims a majority. Only lines starting at the left edge count: an
//! indented line is how every log writes what ran on -- a panic's second line,
//! a stack trace -- and counting those against a format is how a real log fails
//! to be recognised as one. A line no format claims is kept as it was written,
//! which is what makes the reading safe to try on anything: the worst it can do
//! is show the file.
//!
//! Three of the four formats are written out in `log.rs` -- Obelus's own
//! `tracing` layout and syslog's two -- because each is a fixed run of fields
//! with fixed separators, which is less code than reading somebody else's
//! parser. The fourth is `access_log_parser`, which earns the dependency: an
//! access log's fields are typed and its status code is the level. `rsyslog`
//! was measured and left out -- it parses RFC 5424 and fails on the RFC 3164
//! line that is actually in `/var/log/syslog`, which is the file a reader
//! opens.
//!
//! **What Obelus writes, Obelus has to be able to read.** The log gained a
//! process id at the front of every line so that sessions running at once
//! could be told apart, and this reader of that format was not told. It
//! split on the first space expecting a timestamp, got a number, refused
//! every line, and the file Obelus writes was the one file it could not give
//! a reading -- so `ctrl+t` was greyed out on it. A format with a writer and
//! a reader in the same program has a test that the one reads the other
//! (`obelus_can_read_its_own_log`, in `obelus-app`), or they drift and the
//! symptom turns up somewhere that looks unrelated.

use obelus_row::{Ink, Row, Span};

/// How much an entry matters.
///
/// The one thing every format has some version of -- a level, a severity, a
/// status code -- and the reason a log is read in colour at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something is wrong.
    Error,
    /// Something might be.
    Warning,
    /// The ordinary running commentary.
    Info,
    /// The detail nobody reads until they have to.
    Debug,
}

impl Level {
    /// How a level's word is drawn.
    ///
    /// The two that matter have a look of their own; the two that do not
    /// stay out of the way, which is what makes the first two visible.
    #[must_use]
    pub const fn ink(self) -> Ink {
        match self {
            Self::Error => Ink::Wrong,
            Self::Warning => Ink::Doubtful,
            Self::Info => Ink::Plain,
            Self::Debug => Ink::Aside,
        }
    }

    /// The word to draw, which is not always the word in the file: the
    /// formats spell these differently and a column of one width reads as a
    /// column.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    /// The level a syslog severity number means.
    const fn from_severity(severity: u8) -> Self {
        match severity {
            0..=3 => Self::Error,
            4 => Self::Warning,
            5 | 6 => Self::Info,
            _ => Self::Debug,
        }
    }

    /// And the level a status code means, for a log of requests.
    const fn from_status(status: u16) -> Self {
        match status {
            500..=599 => Self::Error,
            400..=499 => Self::Warning,
            _ => Self::Info,
        }
    }
}

/// One line of a log, once a format has claimed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// When it happened, as the format wrote it.
    pub when: String,
    /// How much it matters, when the format says.
    pub level: Option<Level>,
    /// Who said it: a module, a program, a host, a client.
    pub who: String,
    /// What was said.
    pub said: String,
    /// The named values after it, for the formats that have them.
    pub fields: Vec<(String, String)>,
}

/// The formats Obelus can read, in the order they are tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// What Obelus writes: `tracing`'s own layout.
    Ours,
    /// Syslog as the wire carries it, RFC 5424.
    Syslog,
    /// Syslog as it sits in a file, RFC 3164.
    Classic,
    /// A web server's access log, common or combined.
    Access,
}

impl Format {
    /// Every format, in the order a line is offered to them.
    ///
    /// Most particular first: Obelus's own layout and RFC 5424 both begin
    /// with a timestamp no other format would produce, and the classic
    /// syslog line is the loosest of the four -- offered last so it cannot
    /// claim a line one of the others would have read properly.
    pub const ALL: [Self; 4] = [Self::Ours, Self::Syslog, Self::Access, Self::Classic];

    /// The entry a line is, if it is one of this format's.
    #[must_use]
    pub fn read(self, line: &str) -> Option<Entry> {
        match self {
            Self::Ours => ours(line),
            Self::Syslog => syslog(line),
            Self::Classic => classic(line),
            Self::Access => access(line),
        }
    }
}

/// Which format a file is in, if any of them claims it.
///
/// By reading the file rather than by its name: a syslog file is called
/// `syslog`, an access log `access.log`, and Obelus's own `obelus.log` --
/// the extension says nothing, so the lines are asked instead.
///
/// A majority of the first lines, not one of them: a source file with a log
/// line quoted in it is not a log, and a log whose first line is a banner
/// still is.
///
/// The lines that count are the ones starting at the left edge. An indented
/// line is how every log writes what ran on -- a panic's second line, a
/// stack trace, an exception -- and counting those against a format is how
/// a real log fails to be recognised as one.
///
/// Cheap: four formats over twenty lines, and the answer is kept with the
/// rendering rather than worked out again.
#[must_use]
pub fn format_of(source: &str) -> Option<Format> {
    /// How many lines to ask about.
    const ASKED: usize = 20;
    /// And how many of them a format has to claim, in hundredths.
    const ENOUGH: usize = 60;

    let lines: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.starts_with([' ', '\t']))
        .take(ASKED)
        .collect();
    if lines.len() < 2 {
        return None;
    }
    Format::ALL.into_iter().find(|format| {
        let claimed = lines
            .iter()
            .filter(|line| format.read(line).is_some())
            .count();
        claimed * 100 >= lines.len() * ENOUGH
    })
}

/// `2026-09-11T02:49:00.854699Z  INFO obelus::app: said this key=value`
fn ours(line: &str) -> Option<Entry> {
    // Which Obelus said it, first: one file holds every session, and
    // several of them run at once -- a reader following one has to be able
    // to tell it from the others. Written by the subscriber, so it is there
    // on every line of Obelus's own; kept as a field, where the rest of
    // what a line carries by name already goes.
    let (whose, line) = match line.split_once(' ') {
        Some((first, rest)) if first.bytes().all(|byte| byte.is_ascii_digit()) => {
            (Some(first), rest)
        }
        _ => (None, line),
    };
    let (stamp, rest) = line.split_once(' ')?;
    // A timestamp, and a strict one: this is the format Obelus writes, so
    // there is no need to guess at what a date looks like.
    let when = time_of_day(stamp)?;
    let rest = rest.trim_start();
    let (level, rest) = rest.split_once(' ')?;
    let level = match level {
        "ERROR" => Level::Error,
        "WARN" => Level::Warning,
        "INFO" => Level::Info,
        "DEBUG" | "TRACE" => Level::Debug,
        _ => return None,
    };
    let (who, said) = rest.trim_start().split_once(": ")?;
    let (said, mut fields) = split_fields(said);
    if let Some(whose) = whose {
        fields.insert(0, ("pid".to_string(), whose.to_string()));
    }
    Some(Entry {
        when,
        level: Some(level),
        who: who.to_string(),
        said,
        fields,
    })
}

/// `<34>1 2003-10-11T22:14:15.003Z host su 1234 ID47 - said this`
fn syslog(line: &str) -> Option<Entry> {
    let (priority, rest) = priority(line)?;
    // The version, which RFC 5424 fixed at 1 and which is what tells this
    // format from the classic one.
    let rest = rest.strip_prefix("1 ")?;
    let mut parts = rest.splitn(6, ' ');
    let when = time_of_day(parts.next()?)?;
    let host = parts.next()?;
    let app = parts.next()?;
    let process = parts.next()?;
    let _message_id = parts.next()?;
    let said = parts.next().unwrap_or_default();
    // The structured data comes first in what is left, and Obelus shows it
    // as what it is: named values.
    let (said, fields) = structured(said);
    Some(Entry {
        when,
        level: Some(Level::from_severity(priority % 8)),
        who: named(&[host, app, process]),
        said,
        fields,
    })
}

/// `Sep 11 02:49:00 host prog[123]: said this`
///
/// The loosest of the four, and the one in `/var/log`. The month name and
/// the two colons in the clock are the whole of what makes it recognisable,
/// which is why it is offered a line last.
fn classic(line: &str) -> Option<Entry> {
    /// The months, as syslog writes them.
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    // A priority is allowed in front of it: some daemons write one into the
    // file as well as onto the wire.
    let (priority, rest) = match priority(line) {
        Some((priority, rest)) => (Some(priority), rest),
        None => (None, line),
    };
    // Split on the blanks and ignore the empty ones: the day is
    // space-padded to two columns, so `Sep  1` has two of them.
    let mut parts = rest.split(' ').filter(|part| !part.is_empty());
    let month = parts.next()?;
    if !MONTHS.contains(&month) {
        return None;
    }
    let day = parts.next()?;
    let clock = parts.next()?;
    if clock.len() != 8 || clock.matches(':').count() != 2 {
        return None;
    }
    // What follows the clock, which is at the head of the line, so its
    // first appearance is the one just read.
    let (_, rest) = rest.split_once(clock)?;
    let rest = rest.trim_start();
    let (who, said) = match rest.split_once(": ") {
        Some((who, said)) => (who.to_string(), said.to_string()),
        // A line with no program in it is still a line.
        None => (String::new(), rest.to_string()),
    };
    Some(Entry {
        when: format!("{month} {day} {clock}"),
        level: priority.map(|priority| Level::from_severity(priority % 8)),
        who,
        said,
        fields: Vec::new(),
    })
}

/// `127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET / HTTP/1.0" 200 2326`
fn access(line: &str) -> Option<Entry> {
    use access_log_parser::{LogEntry, LogType, RequestResult, parse};

    // Combined first, because a combined line parses as neither anything
    // else nor as common -- and a common line fails combined, which is what
    // makes trying both in this order the whole of the decision.
    let entry = [LogType::CombinedLog, LogType::CommonLog]
        .into_iter()
        .find_map(|kind| parse(kind, line).ok())?;
    let said = |request: &RequestResult| match request {
        RequestResult::Valid(request) => {
            format!("{} {}", request.method(), request.uri())
        }
        // What the parser could not make sense of, as it was written: a
        // malformed request line is exactly the line a reader of an access
        // log is looking for.
        RequestResult::InvalidRequest(raw) => (*raw).to_string(),
        RequestResult::InvalidPath(path, _) => path.to_string(),
    };
    let (when, level, who, what, mut fields) = match &entry {
        LogEntry::CommonLog(common) => (
            common.timestamp,
            Level::from_status(common.status_code.as_u16()),
            common.ip.to_string(),
            said(&common.request),
            vec![
                ("status".to_string(), common.status_code.to_string()),
                ("bytes".to_string(), common.bytes.to_string()),
            ],
        ),
        LogEntry::CombinedLog(combined) => (
            combined.timestamp,
            Level::from_status(combined.status_code.as_u16()),
            combined.ip.to_string(),
            said(&combined.request),
            vec![
                ("status".to_string(), combined.status_code.to_string()),
                ("bytes".to_string(), combined.bytes.to_string()),
            ],
        ),
        // The other kinds the parser knows -- CloudFront, a cloud's own --
        // are not what this is offered, and a line Obelus cannot describe
        // is better left as it was written.
        _ => return None,
    };
    if let LogEntry::CombinedLog(combined) = &entry {
        if let Some(referrer) = combined.referrer.as_ref() {
            fields.push(("from".to_string(), referrer.to_string()));
        }
        if let Some(agent) = combined.user_agent {
            fields.push(("agent".to_string(), agent.to_string()));
        }
    }
    Some(Entry {
        when: when.format("%H:%M:%S").to_string(),
        level: Some(level),
        who,
        said: what,
        fields,
    })
}

/// The `<34>` in front of a syslog line, and what is after it.
fn priority(line: &str) -> Option<(u8, &str)> {
    let rest = line.strip_prefix('<')?;
    let (priority, rest) = rest.split_once('>')?;
    Some((priority.parse().ok()?, rest))
}

/// The clock out of an ISO timestamp, which is all a screen has room for.
///
/// The date is the same all day and the year is the same all year: a column
/// of them is a column of noise, and the file's own first line says which
/// day it is.
fn time_of_day(stamp: &str) -> Option<String> {
    let (_, clock) = stamp.split_once('T')?;
    let clock = clock.trim_end_matches('Z');
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction)) => (clock, fraction.get(..3).unwrap_or(fraction)),
        None => (clock, ""),
    };
    if clock.matches(':').count() != 2 {
        return None;
    }
    Some(match fraction.is_empty() {
        true => clock.to_string(),
        false => format!("{clock}.{fraction}"),
    })
}

/// The parts of a name that are there, joined.
fn named(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty() && **part != "-")
        .copied()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The `key=value` fields at the end of a message, and the message without
/// them.
///
/// Split on the blanks outside quotes, because a value can hold one --
/// `term=Some("xterm-256color")` is one field -- and a word with no `=` in
/// it belongs to the message.
fn split_fields(said: &str) -> (String, Vec<(String, String)>) {
    let mut fields = Vec::new();
    let mut message = said;
    for token in tokens(said).into_iter().rev() {
        let word = &said[token.clone()];
        let Some((name, value)) = word.split_once('=') else {
            break;
        };
        if name.is_empty() || name.contains(['"', ' ']) {
            break;
        }
        fields.push((name.to_string(), value.to_string()));
        message = said[..token.start].trim_end();
    }
    fields.reverse();
    (message.to_string(), fields)
}

/// Syslog's structured data, as named values, and the message after it.
fn structured(said: &str) -> (String, Vec<(String, String)>) {
    let Some(rest) = said.strip_prefix('[') else {
        return (said.trim_start_matches("- ").to_string(), Vec::new());
    };
    let Some((data, message)) = rest.split_once(']') else {
        return (said.to_string(), Vec::new());
    };
    let mut fields = Vec::new();
    for token in tokens(data) {
        let word = &data[token];
        if let Some((name, value)) = word.split_once('=') {
            fields.push((name.to_string(), value.trim_matches('"').to_string()));
        }
    }
    (message.trim().to_string(), fields)
}

/// The words of a line, with a quoted run counting as one word.
fn tokens(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut tokens = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (index, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if index > start {
                    tokens.push(start..index);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if line.len() > start {
        tokens.push(start..line.len());
    }
    tokens
}

/// How much room a run-on row needs for words.
///
/// Below this the columns are given up and the text starts at the left
/// edge: a screen too narrow to hold the columns and a few words has no
/// columns worth lining up with.
const LEAST_MESSAGE: usize = 16;

/// Where a row that ran on starts.
///
/// Under the message, which is the whole point of the columns -- and the
/// one place that decides it, because the wrapping and the lines no format
/// claimed both have to agree or the reading is ragged.
fn indent_of(columns: usize, width: u16) -> usize {
    let room = usize::from(width.max(20));
    match room.saturating_sub(columns) >= LEAST_MESSAGE {
        true => columns,
        false => 0,
    }
}

/// How wide a column of names is allowed to get.
///
/// A module path or a host and program can be long, and a column as wide as
/// the longest one would push the messages off the screen. Cut from the
/// left, because the end of `obelus::app::documents` is the part that says
/// which it is.
const WIDEST_WHO: usize = 22;

/// Lays a log out for a width.
///
/// Columns, which is the whole point: the levels down one edge and the
/// messages starting at the same place, so a screenful is skimmed rather
/// than read. The columns are as wide as the file needs and no wider, so a
/// log of one module does not pay for a column sized to a name it does not
/// have.
///
/// A line no format claims is kept as it is, in a row of its own: that is
/// what makes this safe to run over anything, and it is what the second line
/// of a message that ran on should look like anyway.
#[must_use]
pub fn render(source: &str, width: u16) -> Vec<Row> {
    let Some(format) = format_of(source) else {
        return source.lines().map(plain).collect();
    };
    let read: Vec<Option<Entry>> = source.lines().map(|line| format.read(line)).collect();

    // The columns, measured over the whole file rather than the screenful:
    // a column that changed width as the reader scrolled would move every
    // message on every row.
    let when = read
        .iter()
        .flatten()
        .map(|entry| entry.when.chars().count())
        .max()
        .unwrap_or(0);
    let level = read
        .iter()
        .flatten()
        .filter_map(|entry| entry.level)
        .map(|level| level.word().chars().count())
        .max()
        .unwrap_or(0);
    let who = read
        .iter()
        .flatten()
        .map(|entry| entry.who.chars().count().min(WIDEST_WHO))
        .max()
        .unwrap_or(0);

    // Where a message starts, which is where a line that ran on belongs:
    // the columns before it, and a blank after each.
    let columns: usize = [when, level, who]
        .iter()
        .filter(|column| **column > 0)
        .map(|column| column + 1)
        .sum();

    let mut rows = Vec::with_capacity(read.len());
    let mut claimed = false;
    for (line, entry) in source.lines().zip(read) {
        let Some(entry) = entry else {
            // A line no format claimed, after one it did, is the line above
            // it continued -- a panic's second line, a stack trace -- so it
            // is indented to where that message started. One at the top of
            // a file is a banner and stays where it was written.
            rows.push(match claimed {
                true => continued(line, columns, width),
                false => plain(line),
            });
            continue;
        };
        claimed = true;
        let mut spans = vec![Span::new(format!("{:<when$} ", entry.when), Ink::Aside)];
        if level > 0 {
            spans.push(Span::new(
                format!(
                    "{:<level$} ",
                    entry.level.map(Level::word).unwrap_or_default()
                ),
                entry.level.map_or(Ink::Plain, Level::ink),
            ));
        }
        if who > 0 {
            spans.push(Span::new(
                format!("{:<who$} ", cut(&entry.who, who)),
                Ink::Name,
            ));
        }
        spans.push(Span::new(entry.said.clone(), Ink::Plain));
        for (name, value) in &entry.fields {
            spans.push(Span::new(format!(" {name}="), Ink::Key));
            spans.push(Span::new(value.clone(), Ink::Code));
        }
        rows.extend(wrapped(spans, width, columns));
    }
    rows
}

/// A row of somebody else's text, uncoloured.
fn plain(line: &str) -> Row {
    Row::of(vec![Span::new(line.to_string(), Ink::Plain)])
}

/// The same, indented to where the message above it started.
fn continued(line: &str, columns: usize, width: u16) -> Row {
    Row::of(vec![Span::new(
        format!(
            "{}{}",
            " ".repeat(indent_of(columns, width)),
            line.trim_start()
        ),
        Ink::Plain,
    )])
}

/// As much of a name as the column allows, cut from the left.
fn cut(name: &str, room: usize) -> String {
    let width = name.chars().count();
    if width <= room {
        return name.to_string();
    }
    let kept: String = name
        .chars()
        .skip(width.saturating_sub(room.saturating_sub(1)))
        .collect();
    format!("\u{2026}{kept}")
}

/// One entry's spans, cut into rows that fit the width.
///
/// What runs on is indented to `columns`, where the message started, so the
/// columns stay columns: a wrapped line that began at the left edge would
/// read as a new entry.
fn wrapped(spans: Vec<Span>, width: u16, columns: usize) -> Vec<Row> {
    let room = usize::from(width.max(20));
    // Where the message starts, which the layout above measured: reading it
    // back off the runs would mean telling a level's ink from a message's,
    // and the quiet level is drawn as plain text on purpose.
    let indent = indent_of(columns, width);
    let mut rows = Vec::new();
    let mut row: Vec<Span> = Vec::new();
    let mut filled = 0;
    for span in spans {
        let mut text = span.text.as_ref();
        loop {
            let left = room.saturating_sub(filled);
            let wide = text.chars().count();
            // Moved to the next row whole, when it would fit on one: a path
            // or a value cut down the middle is one nobody recognises, and
            // this is the common case -- a field at the edge of the screen.
            if wide > left && filled > indent && wide <= room.saturating_sub(indent) {
                rows.push(Row::of(std::mem::take(&mut row)));
                row.push(Span::new(" ".repeat(indent), Ink::Plain));
                filled = indent;
                continue;
            }
            let mut taken = wide.min(room.saturating_sub(filled));
            // On a blank where there is one: a path cut in the middle is a
            // path a reader cannot recognise, and the message is the part
            // of a log line that is read as words.
            if taken < text.chars().count() {
                let head: String = text.chars().take(taken + 1).collect();
                if let Some(blank) = head.rfind(' ').filter(|at| *at > 0) {
                    taken = head[..blank].chars().count();
                }
            }
            if taken > 0 {
                let head: String = text.chars().take(taken).collect();
                filled += taken;
                row.push(Span::new(head, span.ink));
                text = text[text
                    .char_indices()
                    .nth(taken)
                    .map_or(text.len(), |(at, _)| at)..]
                    .trim_start_matches(' ');
            }
            if text.is_empty() {
                break;
            }
            rows.push(Row::of(std::mem::take(&mut row)));
            // The indent is drawn as blanks in the message's own ink, so a
            // run-on row is the message continued rather than a column with
            // nothing in it.
            row.push(Span::new(" ".repeat(indent), Ink::Plain));
            filled = indent;
        }
    }
    if !row.is_empty() {
        rows.push(Row::of(row));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The columns are the point: one clock, one level, one name, and every
    /// message starting in the same place.
    #[test]
    fn the_messages_line_up() {
        let source = "\
2026-09-11T02:49:00.854699Z  INFO ob: starting files=1
2026-09-11T02:49:02.893231Z  WARN obelus::app::documents: watching path=/tmp
";
        let rows = render(source, 90);
        let drawn = |row: &Row| -> String { row.spans.iter().map(|span| &*span.text).collect() };
        assert_eq!(
            drawn(&rows[0]).find("starting"),
            drawn(&rows[1]).find("watching"),
            "the messages do not start in the same column: {rows:?}"
        );
        // The short name is padded to the long one, not cut to it.
        assert!(drawn(&rows[0]).contains("ob    "), "{:?}", drawn(&rows[0]));
        assert!(drawn(&rows[1]).contains("obelus::app::documents "));
        // And the levels are drawn in the colour of what they are.
        let ink = |row: &Row, word: &str| {
            row.spans
                .iter()
                .find(|span| span.text.trim() == word)
                .map(|span| span.ink)
        };
        assert_eq!(ink(&rows[1], "warn"), Some(Ink::Doubtful));
        assert_eq!(Level::Error.ink(), Ink::Wrong);
        assert_eq!(
            Level::Info.ink(),
            Ink::Plain,
            "the quiet level was given a look of its own"
        );
    }

    /// An entry too wide for the screen keeps its columns: what wraps is
    /// indented to where the message started, so a run-on row cannot be
    /// read as an entry of its own.
    #[test]
    fn a_wide_entry_wraps_under_its_message() {
        let source = "\
2026-09-11T02:49:00.854699Z  INFO obelus::buffer: opened a file with a long name path=/one/two/three/four/five/six.rs
2026-09-11T02:49:01.000000Z  INFO obelus::buffer: short
";
        let rows = render(source, 60);
        let drawn = |row: &Row| -> String { row.spans.iter().map(|span| &*span.text).collect() };
        assert!(rows.len() > 2, "nothing wrapped: {rows:?}");
        let message = drawn(&rows[0]).find("opened").expect("the message");
        let ran_on = drawn(&rows[1]);
        assert_eq!(
            ran_on.len() - ran_on.trim_start().len(),
            message,
            "what wrapped is not under the message: {ran_on:?}"
        );
    }

    /// A line that ran on sits under the message it belongs to, and a
    /// banner at the top of a file stays where it was written.
    #[test]
    fn what_ran_on_is_indented_and_a_banner_is_not() {
        let source = "\
a banner nobody parses
2026-09-11T02:49:03.001000Z ERROR obelus::logging: panicked
  left: 6
2026-09-11T02:49:04.123456Z DEBUG obelus::app: carried on
";
        let rows = render(source, 80);
        let drawn = |row: &Row| -> String { row.spans.iter().map(|span| &*span.text).collect() };
        assert_eq!(drawn(&rows[0]), "a banner nobody parses");
        let message = drawn(&rows[1]).find("panicked").expect("the message");
        let continued = drawn(&rows[2]);
        assert_eq!(
            continued.find("left: 6"),
            Some(message),
            "what ran on is not under the message: {continued:?}"
        );
    }

    /// A file no format claims is kept as it was written.
    ///
    /// Which is what makes the reading safe to offer: the worst it can do
    /// is show the file.
    #[test]
    fn a_file_that_is_not_a_log_is_left_alone() {
        let source = "fn main() {\n    println!(\"hi\");\n}\n";
        let rows = render(source, 80);
        let drawn: Vec<String> = rows
            .iter()
            .map(|row| row.spans.iter().map(|span| &*span.text).collect())
            .collect();
        assert_eq!(drawn, source.lines().collect::<Vec<&str>>());
    }

    /// The format Obelus writes, which is the one it does not have to guess
    /// about: the parts are fixed and so is what separates them.
    #[test]
    fn our_own_format_is_read() {
        let entry = Format::Ours
            .read("2026-09-11T02:49:00.854699Z  INFO obelus::app: opened bytes=6580 lines=165")
            .expect("our own line");
        assert_eq!(entry.when, "02:49:00.854");
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.who, "obelus::app");
        assert_eq!(entry.said, "opened");
        assert_eq!(
            entry.fields,
            [
                ("bytes".to_string(), "6580".to_string()),
                ("lines".to_string(), "165".to_string())
            ]
        );

        // A value with a blank in it is one field, and a message with a
        // colon in it keeps its colon.
        let entry = Format::Ours
            .read("2026-09-11T02:49:03.001Z ERROR obelus::logging: panicked: oh dear term=\"xterm 256\"")
            .expect("our own line");
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.said, "panicked: oh dear");
        assert_eq!(
            entry.fields,
            [("term".to_string(), "\"xterm 256\"".to_string())]
        );
    }

    /// Syslog on the wire: a priority, the version that tells this format
    /// from the classic one, and structured data as named values.
    #[test]
    fn syslog_on_the_wire_is_read() {
        let entry = Format::Syslog
            .read("<34>1 2003-10-11T22:14:15.003Z mymachine su 1234 ID47 - 'su root' failed")
            .expect("a 5424 line");
        assert_eq!(entry.when, "22:14:15.003");
        // Severity 2 of facility 4: critical, which is an error.
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.who, "mymachine su 1234");
        assert_eq!(entry.said, "'su root' failed");

        let entry = Format::Syslog
            .read("<165>1 2003-10-11T22:14:15Z host evntslog - ID47 [exampleSDID@32473 iut=\"3\"] started")
            .expect("a 5424 line with data");
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.said, "started");
        assert_eq!(entry.fields, [("iut".to_string(), "3".to_string())]);
    }

    /// And syslog as it sits in a file, which is the one with no level in it.
    #[test]
    fn classic_syslog_is_read() {
        let entry = Format::Classic
            .read("Sep 11 02:49:00 host prog[123]: hello from the plain form")
            .expect("a 3164 line");
        assert_eq!(entry.when, "Sep 11 02:49:00");
        assert_eq!(entry.level, None, "a level was invented");
        assert_eq!(entry.who, "host prog[123]");
        assert_eq!(entry.said, "hello from the plain form");

        // The day is space-padded to two columns.
        let entry = Format::Classic
            .read("Sep  1 02:49:00 host prog: hello")
            .expect("a padded day");
        assert_eq!(entry.when, "Sep 1 02:49:00");
        // And a priority in the file gives the level the text does not have.
        let entry = Format::Classic
            .read("<11>Sep 11 02:49:00 host prog: it failed")
            .expect("a priority in a file");
        assert_eq!(entry.level, Some(Level::Error));
    }

    /// An access log, through the parser that knows the format: the status
    /// code is the level, because it is what a reader of one is looking for.
    #[test]
    fn an_access_log_is_read() {
        let entry = Format::Access
            .read(
                r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /a.gif HTTP/1.0" 500 2326 "http://x/" "Mozilla/5.0""#,
            )
            .expect("a combined line");
        assert_eq!(entry.when, "13:55:36");
        assert_eq!(entry.level, Some(Level::Error), "a 500 is not an error");
        assert_eq!(entry.who, "127.0.0.1");
        assert_eq!(entry.said, "GET /a.gif");
        assert!(
            entry
                .fields
                .contains(&("agent".to_string(), "Mozilla/5.0".to_string())),
            "{:?}",
            entry.fields
        );

        // The common form has no referrer and no agent, and is read by the
        // same call trying the two shapes in order.
        let entry = Format::Access
            .read(r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /a.gif HTTP/1.0" 404 2326"#)
            .expect("a common line");
        assert_eq!(entry.level, Some(Level::Warning), "a 404 is not a warning");
        assert_eq!(entry.fields.len(), 2, "{:?}", entry.fields);
    }

    /// Which format a file is in, decided by reading it.
    ///
    /// A majority of the first lines, so a source file with a log line
    /// quoted in it is not a log -- and each format claims its own rather
    /// than a looser one claiming everything.
    #[test]
    fn a_file_says_which_format_it_is_by_its_lines() {
        let ours = "2026-09-11T02:49:00.854699Z  INFO ob: starting files=1\n\
                    2026-09-11T02:49:02.893231Z  WARN obelus::app: watching path=/tmp\n";
        assert_eq!(format_of(ours), Some(Format::Ours));

        let classic = "Sep 11 02:49:00 host prog[1]: one\nSep 11 02:49:01 host prog[1]: two\n";
        assert_eq!(format_of(classic), Some(Format::Classic));

        let wire = "<34>1 2003-10-11T22:14:15Z host su - - - one\n\
                    <34>1 2003-10-11T22:14:16Z host su - - - two\n";
        assert_eq!(format_of(wire), Some(Format::Syslog));

        let access = "127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] \"GET /a HTTP/1.0\" 200 1\n\
                      127.0.0.1 - - [10/Oct/2000:13:55:37 -0700] \"GET /b HTTP/1.0\" 200 2\n";
        assert_eq!(format_of(access), Some(Format::Access));

        // Not a log: a source file, one that quotes a single log line, and
        // a file with too little in it to tell.
        let source = "fn main() {\n    println!(\"hi\");\n}\n";
        assert_eq!(format_of(source), None);
        let quoting = "// Copied from the log:\n\
                       // 2026-09-11T02:49:00.854699Z  INFO ob: starting\n\
                       fn main() {}\n\
                       let x = 1;\n";
        assert_eq!(format_of(quoting), None);
        assert_eq!(format_of("2026-09-11T02:49:00Z  INFO ob: alone\n"), None);
        assert_eq!(format_of(""), None);
    }
}
