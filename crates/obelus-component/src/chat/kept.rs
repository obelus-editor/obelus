//! A page of conversation, as the file Obelus keeps it in.
//!
//! **The page is Obelus's, and so is the record of it.** What an agent
//! replays of a conversation is its own account: the reader's words with
//! whatever Obelus put in front of them -- who the agent is talking to,
//! what the note says -- run into them as if they had typed it, none of
//! Obelus's own remarks, and a command's output as the agent saw it rather
//! than as the reader did. Each agent replays differently, and some send
//! nothing back at all. So the page is kept as it was drawn, and an agent
//! taking the conversation up is asked for its context and not for its
//! words (`App::ask_for_a_session`).
//!
//! What is kept is what was said and what the reader decided about it --
//! which rows they opened or closed. Not what was on its way: a row still
//! waiting to go is a queue that dies with the process, and a call still
//! running comes back as one nobody finished, because nothing will finish
//! it. Not the command a row was running either -- the process is gone --
//! but everything it printed, which is the row's own words by then.
//!
//! JSON, because a picture is in it and a conversation is a tree of lists
//! rather than a table; and with a version, because the file outlives the
//! program that wrote it.

use base64::Engine as _;
use obelus_text::marker::Marker;
use serde::{Deserialize, Serialize};

use super::{Chat, Said, Speaker};
use crate::composer::{Attached, Part};

/// Which shape of file this is. A file of another is not read.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Page {
    version: u32,
    said: Vec<Row>,
}

#[derive(Serialize, Deserialize)]
struct Row {
    speaker: String,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parts: Vec<Piece>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    places: Vec<Place>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    change: Vec<Changed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    words: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opened: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_opened: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Piece {
    Words(String),
    Picture { mime: String, bytes: String },
}

#[derive(Serialize, Deserialize)]
struct Place {
    path: std::path::PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct Changed {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    text: String,
}

/// A voice as the file spells it, which is not the enum's own: renaming a
/// variant must not make yesterday's file unreadable.
const fn voice(speaker: Speaker) -> Option<&'static str> {
    Some(match speaker {
        Speaker::Reader => "reader",
        Speaker::Agent => "agent",
        Speaker::Thought => "thought",
        Speaker::Tool => "tool",
        Speaker::Note => "note",
        Speaker::Away => "away",
        // States, not things said: never in the transcript to begin with.
        Speaker::Doing | Speaker::Step => return None,
    })
}

fn speaker(voice: &str) -> Option<Speaker> {
    Some(match voice {
        "reader" => Speaker::Reader,
        "agent" => Speaker::Agent,
        "thought" => Speaker::Thought,
        "tool" => Speaker::Tool,
        "note" => Speaker::Note,
        "away" => Speaker::Away,
        _ => return None,
    })
}

const fn marker_word(marker: Marker) -> &'static str {
    match marker {
        Marker::Added => "added",
        Marker::Modified => "modified",
        Marker::Removed => "removed",
    }
}

fn marker(word: &str) -> Option<Marker> {
    Some(match word {
        "added" => Marker::Added,
        "modified" => Marker::Modified,
        "removed" => Marker::Removed,
        _ => return None,
    })
}

fn row_of(said: &Said) -> Option<Row> {
    if said.unsent {
        return None;
    }
    Some(Row {
        speaker: voice(said.speaker)?.to_string(),
        text: said.text.clone(),
        tag: said.tag.clone(),
        state: said.state.clone(),
        kind: said.kind.clone(),
        parts: said
            .parts
            .iter()
            .map(|part| match part {
                Part::Words(words) => Piece::Words(words.clone()),
                Part::Picture(picture) => Piece::Picture {
                    mime: picture.mime.clone(),
                    bytes: base64::engine::general_purpose::STANDARD.encode(&picture.bytes),
                },
            })
            .collect(),
        places: said
            .places
            .iter()
            .map(|place| Place {
                path: place.path.clone(),
                line: place.line,
            })
            .collect(),
        change: said
            .change
            .iter()
            .map(|line| Changed {
                marker: line.marker.map(|marker| marker_word(marker).to_string()),
                text: line.text.clone(),
            })
            .collect(),
        words: said.words.clone(),
        opened: said.opened,
        run_opened: said.run_opened,
    })
}

fn said_of(row: Row) -> Result<Said, String> {
    let speaker =
        speaker(&row.speaker).ok_or_else(|| format!("no such voice as {:?}", row.speaker))?;
    let parts = row
        .parts
        .into_iter()
        .map(|piece| match piece {
            Piece::Words(words) => Ok(Part::Words(words)),
            Piece::Picture { mime, bytes } => base64::engine::general_purpose::STANDARD
                .decode(bytes)
                .map(|bytes| Part::Picture(Attached { mime, bytes }))
                .map_err(|error| error.to_string()),
        })
        .collect::<Result<_, _>>()?;
    let change = row
        .change
        .into_iter()
        .map(|line| {
            let marker = match line.marker {
                Some(word) => {
                    Some(marker(&word).ok_or_else(|| format!("no such mark as {word:?}"))?)
                }
                None => None,
            };
            Ok::<_, String>(obelus_git::change::Line {
                marker,
                text: line.text,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(Said {
        speaker,
        text: row.text,
        tag: row.tag,
        state: row.state,
        kind: row.kind,
        parts,
        places: row
            .places
            .into_iter()
            .map(|place| obelus_agent::acp::Place {
                path: place.path,
                line: place.line,
            })
            .collect(),
        change,
        ran: None,
        words: row.words,
        opened: row.opened,
        run_opened: row.run_opened,
        unsent: false,
        afar: false,
    })
}

impl Chat {
    /// What of this page is worth keeping, as the text of the file it is
    /// kept in.
    #[must_use]
    pub fn to_keep(&self) -> String {
        let page = Page {
            version: VERSION,
            said: self.said.iter().filter_map(row_of).collect(),
        };
        serde_json::to_string(&page).unwrap_or_default()
    }

    /// Puts back a page [`Chat::to_keep`] wrote, above whatever is on this
    /// one.
    ///
    /// Above rather than in place of: what is on the page already was said
    /// in this sitting before the conversation was taken up -- a message
    /// typed while the agent was starting -- and it came after.
    ///
    /// Nothing changes where the file will not read: half a page put back
    /// is a page that says less happened than did, and the agent's own
    /// account is still there to be asked for instead.
    ///
    /// # Errors
    ///
    /// What was wrong with the file.
    pub fn bring_back(&mut self, kept: &str) -> Result<(), String> {
        let page: Page = serde_json::from_str(kept).map_err(|error| error.to_string())?;
        if page.version != VERSION {
            return Err(format!("written as version {}", page.version));
        }
        let mut said = page
            .said
            .into_iter()
            .map(said_of)
            .collect::<Result<Vec<_>, _>>()?;
        // Nothing will finish what was running when the page was written:
        // the turn went with the process that was having it.
        for row in &mut said {
            if matches!(row.state.as_deref(), Some("pending" | "in_progress")) {
                row.state = Some("cancelled".to_string());
            }
        }
        said.append(&mut self.said);
        self.said = said;
        self.forget_the_layout();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page with one of every kind of thing on it, each carrying the
    /// fields that are only ever set on that kind.
    fn a_full_page() -> Chat {
        let mut chat = Chat::new();
        chat.asked(&[
            Part::Words("look at this ".to_string()),
            Part::Picture(Attached {
                mime: "image/png".to_string(),
                bytes: vec![0, 1, 2, 255],
            }),
        ]);
        chat.chunk(Speaker::Thought, "where would that be");
        chat.chunk(Speaker::Agent, "it is in **here**");
        chat.tool(
            &obelus_agent::acp::Call {
                id: "t1".to_string(),
                title: "Edit lib.rs".to_string(),
                kind: "edit".to_string(),
                places: vec![obelus_agent::acp::Place {
                    path: "src/lib.rs".into(),
                    line: Some(12),
                }],
                change: Some(obelus_agent::acp::Change {
                    path: "src/lib.rs".into(),
                    before: Some("a\nb\n".to_string()),
                    after: "a\nc\n".to_string(),
                }),
                ran: None,
                said: vec!["done".to_string()],
                backgrounded: false,
            },
            "completed",
        );
        chat.note("Stopped");
        chat.away("q1", "Sign in", "https://example.com");
        chat
    }

    /// What Obelus writes, Obelus reads: every field of every row comes
    /// back as it went.
    ///
    /// Broken deliberately by dropping `opened` from `row_of` (the reader's
    /// fold came back as Obelus's own choice) and by spoiling a picture's
    /// encoding on the way out (the file would not read).
    #[test]
    fn a_page_comes_back_as_it_went() {
        let mut chat = a_full_page();
        chat.said[2].opened = Some(false);
        chat.said[3].run_opened = Some(true);
        let mut back = Chat::new();
        back.bring_back(&chat.to_keep()).expect("the page");

        assert_eq!(back.said.len(), chat.said.len());
        for (went, came) in chat.said.iter().zip(&back.said) {
            assert_eq!(came.speaker, went.speaker);
            assert_eq!(came.text, went.text);
            assert_eq!(came.tag, went.tag);
            assert_eq!(came.kind, went.kind);
            assert_eq!(came.parts, went.parts);
            assert_eq!(came.places, went.places);
            assert_eq!(came.change, went.change);
            assert_eq!(came.words, went.words);
            assert_eq!(came.opened, went.opened);
            assert_eq!(came.run_opened, went.run_opened);
        }
        assert_eq!(back.said[3].state.as_deref(), Some("completed"));
        assert!(
            !back.said[3].change.is_empty(),
            "the call had a change to come back with"
        );
    }

    /// What was waiting to go is not kept, and what was running comes back
    /// stopped.
    ///
    /// Broken deliberately by taking the `unsent` check out of `row_of`:
    /// the words waiting came back as if they had been said.
    #[test]
    fn what_had_not_happened_yet_does_not_come_back_as_if_it_had() {
        let mut chat = a_full_page();
        chat.said.last_mut().expect("a row").state = Some("in_progress".to_string());
        chat.will_say(&[Part::Words("and another thing".to_string())]);
        let mut back = Chat::new();
        back.bring_back(&chat.to_keep()).expect("the page");

        assert!(
            back.said
                .iter()
                .all(|said| !said.text.contains("and another thing")),
            "words that never went came back"
        );
        assert_eq!(
            back.said.last().and_then(|said| said.state.as_deref()),
            Some("cancelled")
        );
    }

    /// A page brought back still says where the agent last wrote, which is
    /// how the conversation learns which checkout it was working in.
    ///
    /// Broken deliberately by keeping a call's `kind` out of the file: the
    /// edit came back as a call of no kind, and with no change to show for
    /// itself it said it had written nothing.
    #[test]
    fn a_page_brought_back_says_where_it_last_wrote() {
        let mut chat = a_full_page();
        chat.said[3].change.clear();
        let mut back = Chat::new();
        back.bring_back(&chat.to_keep()).expect("the page");
        assert_eq!(back.last_to_write().as_deref(), Some("t1"));
    }

    /// What was said on the page before it was brought back stays, under
    /// what was kept.
    ///
    /// Broken deliberately by replacing the page with what was kept: the
    /// reader's message typed while the agent was starting went.
    #[test]
    fn what_is_on_the_page_already_comes_after_what_was_kept() {
        let kept = a_full_page().to_keep();
        let mut chat = Chat::new();
        chat.asked(&[Part::Words("typed while it started".to_string())]);
        chat.bring_back(&kept).expect("the page");
        assert_eq!(
            chat.said.last().map(|said| said.text.as_str()),
            Some("typed while it started")
        );
        assert_eq!(chat.said[0].speaker, Speaker::Reader);
        assert_eq!(chat.said[0].text, a_full_page().said[0].text);
    }

    /// A file that will not read leaves the page as it was.
    #[test]
    fn a_page_that_will_not_read_changes_nothing() {
        let mut chat = a_full_page();
        let before = chat.said.len();
        assert!(
            chat.bring_back("{\"version\":1,\"said\":[{\"speaker\":\"nobody\",\"text\":\"\"}]}")
                .is_err()
        );
        assert!(chat.bring_back("{\"version\":99,\"said\":[]}").is_err());
        assert!(chat.bring_back("not json").is_err());
        assert_eq!(chat.said.len(), before);
    }
}
