//! What an agent offers to be set, kept between sittings.
//!
//! An agent says what it can be set to in a session and nowhere else: the
//! list arrives with `session/new` and belongs to that conversation. So the
//! settings page, which is about what a conversation should *start* on, has
//! nothing to draw until the reader has talked to the agent at least once --
//! and asking them to open a conversation before they can say how their
//! conversations should open is the wrong way round.
//!
//! The answer is a copy on disk, beside the install: what the agent last
//! said it offers. Beside it rather than in the settings file, because it
//! is not a preference -- it is the agent's own statement about itself, and
//! removing an agent is removing a directory.
//!
//! What is *not* kept is which value each was on. That is a fact about one
//! conversation and would read here as a fact about now, which by the next
//! sitting it is not. So the type on disk is not a session's [`Setting`]:
//! it is the offer without the answer, and nothing can mistake one for the
//! other.
//!
//! Read field by field, and forgivingly, for the reason the registry is:
//! this file is written by whichever Obelus last talked to the agent, which
//! may be a newer one than the one reading it.

use std::path::Path;

use serde_json::Value as Json;

use crate::acp::{Category, Kind, Setting, Value};

/// What the file is called, inside the agent's own directory.
pub const OPTIONS: &str = "options.json";

/// One thing an agent says it can be set to.
///
/// A [`Setting`] a conversation away: the values it offers, what to call
/// them, and the one it was itself on when it said -- which between them
/// are everything the settings page needs to let a reader say what a new
/// conversation should start on, and everything the conversation's own row
/// needs to say what the next turn will run on before there is a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// The agent's id for it, which is what a default names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// One line about it, if it said.
    pub about: Option<String>,
    /// Every value it can take, in the agent's own order.
    pub values: Vec<Value>,
    /// Which of the two shapes it is.
    pub kind: Kind,
    /// Which value the agent itself was on when it said this.
    ///
    /// Not what the reader chose -- that is theirs and lives in their
    /// settings file -- but what they get when they have chosen nothing,
    /// which is the other half of "what will this conversation run on".
    /// Without it that question could only be answered by opening a
    /// conversation to ask, which is the thing opening a view no longer
    /// does.
    ///
    /// Only ever from a conversation nobody has said anything in, which is
    /// the whole of what makes it true: the list arrives as part of some
    /// conversation, and the value that one is on is a fact about that
    /// conversation at that moment. Kept from a conversation the reader has
    /// been changing, it would be read next week as what the agent starts
    /// on -- which is the mistake the file's shape was built to avoid, and
    /// why [`Offer::of`] does not fill this in and
    /// [`remember_what_it_starts_on`] is a door of its own.
    ///
    /// `None` for a file written before this was, for an agent Obelus has
    /// only ever met through the reader's own conversations, and for one
    /// that sent a value it does not offer: a name that is not in `values`
    /// is a name nothing can be drawn from.
    pub current: Option<String>,
    /// What sort of thing it is about, as the agent itself says.
    ///
    /// Kept for the one question that cannot be answered without it: which
    /// of these is the *mode*, which is the one a key steps and the one the
    /// conversation's row draws first. Every category Obelus has not heard
    /// of is `Other`, so a file written by a newer Obelus reads here as
    /// silence rather than as a mistake.
    pub category: Category,
}

impl Offer {
    /// The offer inside a session's setting.
    #[must_use]
    pub fn of(setting: &Setting) -> Self {
        Self {
            id: setting.id.clone(),
            name: setting.name.clone(),
            about: setting.about.clone(),
            values: setting.values.clone(),
            kind: setting.kind,
            // Not from here: see [`Offer::current`].
            current: None,
            category: setting.category,
        }
    }

    /// The same, from a conversation nobody has said anything in.
    ///
    /// Which is the one kind whose value means anything a week later: it is
    /// what the agent itself opens on, and so what the next conversation
    /// will run on where the reader has pinned nothing.
    #[must_use]
    pub fn of_a_fresh_one(setting: &Setting) -> Self {
        Self {
            current: setting
                .values
                .iter()
                .any(|value| value.id == setting.current)
                .then(|| setting.current.clone()),
            ..Self::of(setting)
        }
    }

    /// What one of its values is called, for a row that names one.
    ///
    /// `None` for an id the agent does not offer any more, which is a
    /// thing a reader's settings file can hold: the value was there when
    /// they chose it and the agent has been updated since.
    #[must_use]
    pub fn name_of(&self, value: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|offered| offered.id == value)
            .map(|offered| offered.name.as_str())
    }
}

/// What reading the file found.
///
/// Three answers, like every other read under an agent's directory: there
/// is none, here it is, or it will not read. The third is not the first --
/// a file another Obelus is in the middle of writing, or one somebody's
/// disk has half of, is not an agent that offers nothing -- and the
/// difference is the whole reason this is an enum: a page that drew "no
/// settings" for it would be a page saying something false, and a writer
/// that took it for "nothing yet" would write over what is there.
#[derive(Clone, Debug)]
pub enum Reading {
    /// Obelus has not talked to this agent yet, or this system has nowhere
    /// to keep the file.
    Nothing,
    /// Here is what it last said it offers.
    Offers(Vec<Offer>),
    /// There is a file and it could not be read, with what went wrong.
    Unreadable(String),
}

/// What an agent last said it offers.
#[must_use]
pub fn read(id: &str, root: &Path) -> Reading {
    let Some(home) = crate::home(id, root) else {
        return Reading::Nothing;
    };
    let text = match std::fs::read_to_string(home.join(OPTIONS)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Reading::Nothing,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let document = match serde_json::from_str::<Json>(&text) {
        Ok(document) => document,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let Some(entries) = document.get("options").and_then(Json::as_array) else {
        return Reading::Unreadable("no options in it".to_string());
    };
    Reading::Offers(entries.iter().filter_map(offer).collect())
}

/// One entry, or nothing if it is missing what a row needs.
fn offer(entry: &Json) -> Option<Offer> {
    let text = |value: &Json, key: &str| {
        value
            .get(key)
            .and_then(Json::as_str)
            .map(std::string::ToString::to_string)
    };
    Some(Offer {
        id: text(entry, "id")?,
        name: text(entry, "name")?,
        about: text(entry, "about"),
        values: entry
            .get("values")
            .and_then(Json::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| {
                        Some(Value {
                            id: text(value, "id")?,
                            name: text(value, "name")?,
                            about: text(value, "about"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        // A shape Obelus has never heard of is not a row it can draw, so
        // the entry goes rather than being guessed at.
        kind: match text(entry, "kind")?.as_str() {
            "select" => Kind::Select,
            "switch" => Kind::Switch,
            _ => return None,
        },
        current: text(entry, "current"),
        category: match text(entry, "category").as_deref() {
            Some("mode") => Category::Mode,
            Some("model") => Category::Model,
            Some("model_config") => Category::ModelConfig,
            Some("thought_level") => Category::ThoughtLevel,
            _ => Category::Other,
        },
    })
}

/// Writes down what an agent offers, if it is not already what is there.
///
/// Called with a session's settings whenever they arrive, which is often:
/// an agent sends the whole list again every time one of them changes. So
/// the file is read first and left alone where it already says this --
/// which is also the read-before-write a directory several Obelus
/// processes share needs.
///
/// An empty list is not written. An agent that offers nothing sends one,
/// and so does one that has not finished starting: the difference cannot be
/// told from here, and a copy emptied by the second would be the settings
/// page going blank on a reader who has set things on it.
pub fn remember(id: &str, settings: &[Setting], root: &Path) -> Result<(), String> {
    write_down(id, settings, root, Offer::of)
}

/// The same, from a conversation nobody has said anything in -- which also
/// writes down what the agent itself opens on.
///
/// One caller, and it has to stay that way: the session `Ask::Offers` opens
/// is made to read this list off and let go again, so nothing has touched
/// it. Every other conversation's settings are that conversation's, and
/// what one of those is on says nothing about what the next one starts on.
pub fn remember_what_it_starts_on(
    id: &str,
    settings: &[Setting],
    root: &Path,
) -> Result<(), String> {
    write_down(id, settings, root, Offer::of_a_fresh_one)
}

/// Writes the list, however the caller makes one of them.
fn write_down(
    id: &str,
    settings: &[Setting],
    root: &Path,
    made: impl Fn(&Setting) -> Offer,
) -> Result<(), String> {
    if settings.is_empty() {
        return Ok(());
    }
    let mut offers: Vec<Offer> = settings.iter().map(made).collect();
    if let Reading::Offers(already) = read(id, root) {
        // What a fresh conversation said, carried over rather than dropped:
        // this is the one thing in the file that did not come from the
        // settings in hand, and a write from a conversation in use would
        // otherwise take it away -- and then put it back the next time the
        // reader chose an agent, which is a fact that comes and goes.
        for offer in &mut offers {
            if offer.current.is_none() {
                offer.current = already
                    .iter()
                    .find(|had| had.id == offer.id)
                    .and_then(|had| had.current.clone());
            }
        }
        if already == offers {
            return Ok(());
        }
    }
    let Some(home) = crate::home(id, root) else {
        return Err(format!("{id} is not a name Obelus can keep a directory of"));
    };
    // A file that will not read is written over, which is the opposite of
    // what Obelus does to a file under a tree's `.obelus`. The difference
    // is whose the contents are: nothing here was written by the reader
    // and every word of it can be had again from the agent, so a broken
    // copy is repaired rather than kept for ever. What must not happen to
    // an unreadable one is being *read* as an agent that offers nothing,
    // which is why `read` has three answers and not two.
    let mut document = serde_json::json!({
        "options": offers
            .iter()
            .map(|offer| serde_json::json!({
                "id": offer.id,
                "name": offer.name,
                "about": offer.about,
                "values": offer
                    .values
                    .iter()
                    .map(|value| serde_json::json!({
                        "id": value.id,
                        "name": value.name,
                        "about": value.about,
                    }))
                    .collect::<Vec<_>>(),
                "kind": match offer.kind {
                    Kind::Select => "select",
                    Kind::Switch => "switch",
                },
                "category": match offer.category {
                    Category::Mode => "mode",
                    Category::Model => "model",
                    Category::ModelConfig => "model_config",
                    Category::ThoughtLevel => "thought_level",
                    Category::Other => "other",
                },
            }))
            .collect::<Vec<_>>(),
    });
    // The one key that is left out where there is nothing to put in it. A
    // `null` would read back the same, and it would also say in the file
    // that Obelus had asked and been told nothing -- which is not what
    // having never asked looks like.
    if let Some(entries) = document.get_mut("options").and_then(Json::as_array_mut) {
        for (entry, offer) in entries.iter_mut().zip(&offers) {
            if let (Some(entry), Some(current)) = (entry.as_object_mut(), offer.current.as_ref()) {
                entry.insert("current".to_string(), Json::String(current.clone()));
            }
        }
    }
    std::fs::create_dir_all(&home).map_err(|error| format!("{home:?}: {error}"))?;
    // Beside it and renamed over it, because another Obelus may be reading
    // this file at this moment: a plain write truncates first, and a reader
    // landing in that gap gets a file that will not parse.
    let path = home.join(OPTIONS);
    let beside = path.with_extension("json.writing");
    std::fs::write(&beside, document.to_string())
        .map_err(|error| format!("{beside:?}: {error}"))?;
    std::fs::rename(&beside, &path).map_err(|error| format!("{path:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{Offer, Reading, read, remember, remember_what_it_starts_on};
    use crate::acp::{Kind, Setting, Value};

    /// A directory of this test's own.
    fn root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("obelus-options-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// One setting as an agent sends it, on whichever value.
    fn setting(id: &str, current: &str) -> Setting {
        Setting {
            id: id.to_string(),
            name: "Mode".to_string(),
            about: Some("What it may do without asking".to_string()),
            values: vec![
                Value {
                    id: "plan".to_string(),
                    name: "Plan".to_string(),
                    about: None,
                },
                Value {
                    id: "accept-edits".to_string(),
                    name: "Accept edits".to_string(),
                    about: Some("Writes files without asking".to_string()),
                },
            ],
            current: current.to_string(),
            kind: Kind::Select,
            category: crate::acp::Category::Mode,
            legacy: false,
        }
    }

    /// What a conversation nobody has touched was on survives, and what a
    /// conversation in use is on does not take it away.
    ///
    /// The two halves of the one field in this file that does not come from
    /// the list in hand. It is what the agent itself opens on, which is
    /// what the next conversation runs on where the reader has pinned
    /// nothing -- and the only honest sample of it is the conversation
    /// `Ask::Offers` opens to read the list off and lets go again.
    ///
    /// Broken deliberately two ways. Having `remember` use
    /// `Offer::of_a_fresh_one` puts a conversation's own value in the file,
    /// which is the mistake the shape of this file exists to prevent: the
    /// second assertion reads back `accept-edits` as what the agent starts
    /// on. Taking the carrying-over out of `write_down` loses it instead --
    /// the first write said it and the second forgets it.
    #[test]
    fn what_a_fresh_conversation_was_on_is_what_the_agent_starts_on() {
        let root = root("starts-on");
        remember_what_it_starts_on("an-agent", &[setting("mode", "plan")], &root)
            .expect("writing what it starts on");
        let Reading::Offers(offers) = read("an-agent", &root) else {
            panic!("the file did not read back");
        };
        assert_eq!(offers[0].current.as_deref(), Some("plan"));

        // A conversation the reader has been changing says nothing about
        // this, and must not be able to.
        remember("an-agent", &[setting("mode", "accept-edits")], &root).expect("writing again");
        let Reading::Offers(offers) = read("an-agent", &root) else {
            panic!("the file did not read back");
        };
        assert_eq!(
            offers[0].current.as_deref(),
            Some("plan"),
            "a conversation in use was taken for what the agent starts on"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What an agent offers survives the file; which value it was on does
    /// not.
    ///
    /// The second half is the point of the file's shape. The list arrives
    /// as part of one conversation and the value it names is true of that
    /// conversation at that moment; kept here it would be read next week
    /// as a fact about now.
    ///
    /// Broken deliberately by giving `Offer` a `current` and writing it:
    /// the value from the session came back out of the file, where the
    /// settings page would have drawn it as what the agent starts on.
    #[test]
    fn what_an_agent_offers_survives_the_file_and_what_it_was_on_does_not() {
        let root = root("survives");
        remember("an-agent", &[setting("mode", "plan")], &root).expect("writing");

        let Reading::Offers(offers) = read("an-agent", &root) else {
            panic!("the file did not read back");
        };
        assert_eq!(offers, vec![Offer::of(&setting("mode", "plan"))]);
        assert_eq!(offers[0].name_of("accept-edits"), Some("Accept edits"));
        assert_eq!(
            offers[0].name_of("yolo"),
            None,
            "a value the agent does not offer was given a name"
        );

        let text =
            std::fs::read_to_string(root.join("an-agent").join(super::OPTIONS)).expect("the file");
        assert!(
            !text.contains("current"),
            "the file says which value the session was on: {text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file that will not read is not an agent with nothing to offer.
    ///
    /// The two were one answer everywhere this was written before, and the
    /// cost is a settings page saying an agent has nothing to be set --
    /// which is a sentence about the agent, and false.
    ///
    /// Broken deliberately by returning `Nothing` for a parse failure: the
    /// half-written file read as an agent that offers nothing.
    #[test]
    fn a_file_that_will_not_read_is_not_an_agent_with_nothing_to_offer() {
        let root = root("unreadable");
        std::fs::create_dir_all(root.join("an-agent")).expect("a directory");
        std::fs::write(
            root.join("an-agent").join(super::OPTIONS),
            "{\"options\": [",
        )
        .expect("the file");

        assert!(
            matches!(read("an-agent", &root), Reading::Unreadable(_)),
            "a file that will not parse was read as an answer"
        );
        // And an agent Obelus has never talked to is the other answer
        // again: nothing to read, and nothing wrong.
        assert!(matches!(read("another-agent", &root), Reading::Nothing));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An agent that says nothing does not empty what it said before.
    ///
    /// The list arrives empty twice: from an agent that has nothing to be
    /// set, and from one that has not finished starting. They cannot be
    /// told apart from here, and the second emptying the file would take
    /// the settings page blank under a reader who has set things on it.
    ///
    /// Broken deliberately by writing the empty list: the page lost every
    /// row the moment a session started.
    #[test]
    fn an_agent_that_says_nothing_does_not_empty_what_it_said_before() {
        let root = root("empty");
        remember("an-agent", &[setting("mode", "plan")], &root).expect("writing");
        remember("an-agent", &[], &root).expect("writing nothing");

        let Reading::Offers(offers) = read("an-agent", &root) else {
            panic!("the file did not read back");
        };
        assert_eq!(offers.len(), 1, "an empty list emptied the file");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same offers again leave the file alone.
    ///
    /// An agent sends the whole list every time one of them changes, so
    /// this is written many times a conversation -- and several Obelus
    /// processes share the directory. A write that changes nothing is a
    /// rename other readers have to survive for no reason.
    ///
    /// Broken deliberately by taking the comparison out of `remember`: the
    /// file was a new one on every call.
    #[cfg(unix)]
    #[test]
    fn the_same_offers_again_leave_the_file_alone() {
        use std::os::unix::fs::MetadataExt as _;

        let root = root("again");
        let path = root.join("an-agent").join(super::OPTIONS);
        remember("an-agent", &[setting("mode", "plan")], &root).expect("writing");
        let first = std::fs::metadata(&path).expect("the file").ino();

        // The same offers, out of a session on a different value: what the
        // session is on is not part of the offer, so this is the same
        // thing said again.
        remember("an-agent", &[setting("mode", "accept-edits")], &root).expect("writing again");
        assert_eq!(
            std::fs::metadata(&path).expect("the file").ino(),
            first,
            "the file was written again for a list that had not changed"
        );

        // And a list that really has changed is written.
        let mut other = setting("mode", "plan");
        other.name = "Way of working".to_string();
        remember("an-agent", &[other], &root).expect("writing a change");
        assert_ne!(
            std::fs::metadata(&path).expect("the file").ino(),
            first,
            "a changed list did not reach the file"
        );

        // Nothing left beside it either.
        let beside: Vec<_> = std::fs::read_dir(root.join("an-agent"))
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != super::OPTIONS)
            .collect();
        assert!(beside.is_empty(), "it left {beside:?} behind");
        let _ = std::fs::remove_dir_all(&root);
    }
}
