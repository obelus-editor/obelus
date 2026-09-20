//! Who calls this, and what this calls.
//!
//! Two questions with one shape, which is why they are one thing here: a
//! server is asked to *prepare* the name under the cursor, and the item it
//! hands back is what every later question is asked about. Each answer is
//! made of more items, and each of those can be asked in turn -- so the
//! answer is a tree that is only ever built as far as the reader opens it.
//!
//! The protocol is lazy by construction: one request per item per
//! direction. That is worth saying because it decides the interface. A
//! fold is exactly a request boundary, so a tree that folds costs one
//! question per thing the reader actually asks about, and one that does
//! not costs a question per row nobody reads.
//!
//! An item is carried whole, as it arrived. The protocol says a `data`
//! field is preserved between preparing an item and asking about it, and a
//! server may put anything there -- so the thing sent back has to be the
//! thing that came, not a rebuilt copy of the parts obelus happened to
//! read.

use lsp_types::{CallHierarchyItem, ServerCapabilities};
use serde_json::Value;

/// Which way round the question is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Who calls this.
    Callers,
    /// What this calls.
    Calls,
}

impl Direction {
    /// Both, in the order their tabs sit in.
    ///
    /// Callers first: "who reaches this" is the question a reader has when
    /// they are looking at something they did not expect to be running,
    /// and that is what brings them here.
    pub const ALL: [Self; 2] = [Self::Callers, Self::Calls];

    /// What to call it on a tab.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Callers => "Callers",
            Self::Calls => "Calls",
        }
    }

    /// What to say about an item with nothing this way.
    #[must_use]
    pub const fn nothing(self) -> &'static str {
        match self {
            Self::Callers => "Nothing calls that",
            Self::Calls => "That calls nothing",
        }
    }

    /// The request that asks it.
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Callers => "callHierarchy/incomingCalls",
            Self::Calls => "callHierarchy/outgoingCalls",
        }
    }
}

/// One name in the tree, and where choosing it goes.
#[derive(Clone, Debug)]
pub struct Called {
    /// What it is called.
    pub name: String,
    /// What sort of thing it is, as the colour it will be drawn in.
    pub kind: obelus_text::kind::SyntaxKind,
    /// Which file it is in.
    pub path: std::path::PathBuf,
    /// Which line and column to go to, counted from zero.
    ///
    /// The call itself where the answer named one, and the name of the
    /// item where it did not. A reader asking who calls this wants the
    /// line that makes the call, not the first line of whoever makes it:
    /// the call is the thing they came to see.
    pub at: (u32, u32),
    /// Where what [`Called::at`] points at ends.
    ///
    /// The two together are a span rather than a point, which is what the
    /// preview under the list marks: the call itself where the answer
    /// named one, and the item's own name where it did not. Without the
    /// end there is nothing to mark, and a reader looking at the preview
    /// has to find the call in the line themselves.
    pub end: (u32, u32),
    /// Where the item itself is, whatever [`Called::at`] was set to.
    ///
    /// What says two rows are the same thing, which is the question a
    /// tree that can be opened for ever has to answer: a caller's row
    /// points at the call, and the same function calling from three
    /// places is three rows pointing at one item.
    pub own: (u32, u32),
    /// The item as it arrived, to ask the next question with.
    pub item: Value,
}

/// Whether the server answers the call hierarchy at all.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.call_hierarchy_provider.is_some()
}

/// The item a server prepared, if it prepared one.
///
/// One rather than the list the protocol allows: the reader put the cursor
/// on one name, and a tree with two roots is an answer to a question
/// nobody asked. The first is the one the server thought likeliest.
#[must_use]
pub fn prepared(result: &Result<Value, String>) -> Option<Value> {
    let Ok(value) = result else {
        return None;
    };
    value.as_array()?.first().cloned()
}

/// What one item is called and where it is, without asking anything else.
#[must_use]
pub fn root_of(item: &Value) -> Option<Called> {
    let parsed = serde_json::from_value::<CallHierarchyItem>(item.clone()).ok()?;
    Some(Called {
        name: parsed.name,
        kind: super::outline::kind_of(parsed.kind),
        path: super::path_of_uri(parsed.uri.as_str())?,
        at: (
            parsed.selection_range.start.line,
            parsed.selection_range.start.character,
        ),
        end: (
            parsed.selection_range.end.line,
            parsed.selection_range.end.character,
        ),
        own: (
            parsed.selection_range.start.line,
            parsed.selection_range.start.character,
        ),
        item: item.clone(),
    })
}

/// What a server said about one item, in one direction.
///
/// The positions stay in the protocol's own units. They are places in
/// files that are mostly not open, and converting them needs the text of
/// the file they are in -- which is a thing the caller has and this does
/// not.
#[must_use]
pub fn called_in(result: &Result<Value, String>, direction: Direction) -> Vec<Called> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Some(answers) = value.as_array() else {
        return Vec::new();
    };
    answers
        .iter()
        .filter_map(|answer| one_call(answer, direction))
        .collect()
}

/// One of them: the item, and the place the call is made.
fn one_call(answer: &Value, direction: Direction) -> Option<Called> {
    // `from` for who calls this and `to` for what this calls, which is the
    // only difference between the two answers.
    let item = match direction {
        Direction::Callers => answer.get("from")?,
        Direction::Calls => answer.get("to")?,
    };
    let mut called = root_of(item)?;
    // Where the call is written, where the answer says. For a caller the
    // ranges are in the caller's own file, which is where this is going;
    // for a callee they are in *this* file, and the item's own name is the
    // better place to land -- so only the first is taken.
    if direction == Direction::Callers
        && let Some(first) = answer
            .get("fromRanges")
            .and_then(Value::as_array)
            .and_then(|ranges| ranges.first())
        && let Some(at) = place_at(first.get("start"))
    {
        called.at = at;
        // The end with the start, or the mark under the list is a span of
        // no width and marks nothing.
        called.end = place_at(first.get("end")).unwrap_or(at);
    }
    Some(called)
}

/// One end of a range, in the protocol's own units.
fn place_at(place: Option<&Value>) -> Option<(u32, u32)> {
    let place = place?;
    let line = place.get("line").and_then(Value::as_u64)?;
    let column = place.get("character").and_then(Value::as_u64)?;
    Some((
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, line: u32) -> Value {
        serde_json::json!({
            "name": name,
            "kind": 12,
            "uri": crate::fake::uri("/tmp/one.rs"),
            "range": { "start": { "line": line, "character": 0 },
                       "end": { "line": line + 3, "character": 1 } },
            // The name itself, which is what a callee's row marks.
            "selectionRange": { "start": { "line": line, "character": 3 },
                                "end": { "line": line, "character": 3 + name.len() } },
            "data": { "server": "said this" }
        })
    }

    /// The one the server thought likeliest, and nothing about the rest:
    /// the reader put the cursor on one name.
    #[test]
    fn preparing_takes_the_first_and_keeps_it_whole() {
        let first = prepared(&Ok(serde_json::json!([item("run", 10), item("other", 40)])))
            .expect("an item");
        assert_eq!(first["name"], "run");
        // Whole, because what goes back to the server has to be what came:
        // `data` is the server's own and it is preserved between asking to
        // prepare and asking about it.
        assert_eq!(first["data"]["server"], "said this");

        assert!(prepared(&Ok(serde_json::json!([]))).is_none());
        assert!(prepared(&Ok(serde_json::json!(null))).is_none());
        assert!(prepared(&Err("no".to_string())).is_none());
    }

    /// A row is a span rather than a point: what the preview under the
    /// list marks is the call itself, and a span of no width marks
    /// nothing.
    #[test]
    fn a_row_carries_the_whole_call_rather_than_its_first_column() {
        let found = called_in(
            &Ok(serde_json::json!([{
                "from": item("main", 80),
                "fromRanges": [{ "start": { "line": 85, "character": 4 },
                                 "end": { "line": 85, "character": 7 } }]
            }])),
            Direction::Callers,
        );
        assert_eq!(found[0].at, (85, 4));
        assert_eq!(found[0].end, (85, 7), "the call has no width to mark");

        // And a callee, whose span is the item's own name.
        let found = called_in(
            &Ok(serde_json::json!([{ "to": item("helper", 200), "fromRanges": [] }])),
            Direction::Calls,
        );
        assert_eq!(found[0].at, (200, 3));
        assert_eq!(found[0].end, (200, 9), "the name has no width to mark");
    }

    /// A caller is somewhere to go, and where to go is the call rather
    /// than the first line of whoever makes it.
    #[test]
    fn a_caller_lands_on_the_call() {
        let found = called_in(
            &Ok(serde_json::json!([{
                "from": item("main", 80),
                "fromRanges": [{ "start": { "line": 85, "character": 4 },
                                 "end": { "line": 85, "character": 7 } }]
            }])),
            Direction::Callers,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "main");
        assert_eq!(
            found[0].at,
            (85, 4),
            "it landed on the caller, not the call"
        );
    }

    /// What this calls is somewhere else, and the ranges an answer carries
    /// are in *this* file -- so the item's own name is where to go.
    #[test]
    fn a_callee_lands_on_its_own_name() {
        let found = called_in(
            &Ok(serde_json::json!([{
                "to": item("helper", 200),
                "fromRanges": [{ "start": { "line": 12, "character": 8 },
                                 "end": { "line": 12, "character": 14 } }]
            }])),
            Direction::Calls,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].at,
            (200, 3),
            "it landed on the call site, which is in the file being read"
        );
    }

    /// Where the call is and where the thing making it is are two
    /// different places, and the second is what says two rows are one
    /// thing.
    #[test]
    fn a_caller_remembers_where_it_itself_is() {
        let found = called_in(
            &Ok(serde_json::json!([{
                "from": item("main", 80),
                "fromRanges": [{ "start": { "line": 85, "character": 4 },
                                 "end": { "line": 85, "character": 7 } }]
            }])),
            Direction::Callers,
        );
        assert_eq!(found[0].at, (85, 4), "where to go");
        assert_eq!(found[0].own, (80, 3), "what it is");
    }

    /// A server with nothing to say says it in several ways.
    #[test]
    fn nothing_said_is_nobody_calling() {
        for answer in [
            Ok(serde_json::json!(null)),
            Ok(serde_json::json!([])),
            Err("no".to_string()),
        ] {
            assert!(called_in(&answer, Direction::Callers).is_empty());
        }
    }
}
