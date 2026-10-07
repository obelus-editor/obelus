//! JetBrains' extension for background work, as claude-agent-acp speaks it.
//!
//! Every word of it is here and nowhere else: the `jetbrains.air` namespace,
//! the three `async_task_*` updates, their camel-cased fields and the method
//! that stops one. The adapter documents all of it as experimental
//! (`docs/air-extensions.md` in claude-agent-acp), and it may change or go
//! without notice -- which is what [`super::tasks`] is shaped around. When
//! the protocol has words of its own for this, this file is deleted and a
//! search for `jetbrains` finds nothing.
//!
//! What was read to write it: claude-agent-acp's `src/async-tasks.ts` and its
//! document, at the commit of 2026-10-05.
//!
//! **Only the version Obelus was written against is spoken.** An agent that
//! offers a later one may have changed what the fields mean; reading it as
//! the first is a guess, and a wrong guess about background work is a server
//! the reader is told has stopped.

use std::path::PathBuf;

use agent_client_protocol::schema::v1::Meta;

use super::tasks::{News, State};

/// The version of the extension Obelus reads.
const VERSION: u64 = 1;

/// The one capability of it Obelus asks for.
///
/// The others change how ordinary things are sent -- a tool call's input,
/// a plan, a diff -- and Obelus draws those already, from the protocol's own
/// fields. Asking for them would trade what it draws now for a dialect.
const ASYNC_TASKS: &str = "asyncTasks";

/// The method that asks for one to be stopped.
pub(super) const STOP: &str = "_session/async_task/stop";

/// Says in the handshake that Obelus reads background work this way.
pub(super) fn declare(meta: &mut Meta) {
    meta.insert(
        "jetbrains".to_string(),
        serde_json::json!({
            "air": {
                "version": VERSION,
                "capabilities": [ASYNC_TASKS],
            }
        }),
    );
}

/// The extension's own corner of a `_meta`, where there is one.
fn air(meta: Option<&Meta>) -> Option<&serde_json::Value> {
    meta?.get("jetbrains")?.get("air")
}

/// Whether the agent's answer to `initialize` says it speaks this.
///
/// Its version must be the one Obelus reads and its list of capabilities
/// must name the one Obelus asked for. Anything else -- no corner, a corner
/// of the wrong shape, a later version -- is an agent that does not.
pub(super) fn offered(meta: Option<&Meta>) -> bool {
    let Some(air) = air(meta) else {
        return false;
    };
    let version = air.get("version").and_then(serde_json::Value::as_u64);
    let named = air
        .get("capabilities")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|all| all.iter().any(|one| one.as_str() == Some(ASYNC_TASKS)));
    let offered = version == Some(VERSION) && named;
    if !offered && version.is_some() {
        tracing::info!(
            ?version,
            named,
            "the agent speaks JetBrains' extension, but not the background work Obelus reads"
        );
    }
    offered
}

/// Whether a tool call says the work it started goes on after it.
pub(super) fn backgrounded(meta: Option<&Meta>) -> bool {
    air(meta)
        .and_then(|air| air.get("asyncTasks"))
        .and_then(|tasks| tasks.get("backgrounded"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// What an update says about background work, if it is one of this
/// extension's.
pub(super) fn read(update: &serde_json::Value) -> Option<Result<News, String>> {
    let kind = update.get("sessionUpdate")?.as_str()?;
    let read = match kind {
        "async_task_spawned" => began(update),
        "async_task_progress" => changed(update, None),
        "async_task_state_update" => {
            match update.get("state").and_then(serde_json::Value::as_str) {
                Some(state) => changed(update, Some(state_of(state))),
                None => Err("a state update with no state".to_string()),
            }
        }
        _ => return None,
    };
    Some(read)
}

/// The id every one of them carries.
fn id_of(update: &serde_json::Value) -> Result<String, String> {
    update
        .get("asyncTaskId")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "no asyncTaskId".to_string())
}

/// A field that is words, where it is there and is words.
fn words(update: &serde_json::Value, field: &str) -> Option<String> {
    update
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|words| !words.is_empty())
        .map(str::to_string)
}

fn began(update: &serde_json::Value) -> Result<News, String> {
    let id = id_of(update)?;
    // A name is what the row is; one without is drawn by its description,
    // and one with neither by the id, which is ugly and still true.
    let about = words(update, "description");
    let name = words(update, "name")
        .or_else(|| about.clone())
        .unwrap_or_else(|| id.clone());
    Ok(News::Began {
        name,
        kind: words(update, "taskType").unwrap_or_default(),
        about,
        call: words(update, "toolCallId"),
        output: words(update, "outputFilePath").map(PathBuf::from),
        stoppable: update
            .get("canStop")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        id,
    })
}

fn changed(update: &serde_json::Value, state: Option<State>) -> Result<News, String> {
    Ok(News::Changed {
        id: id_of(update)?,
        state,
        summary: words(update, "summary"),
        about: words(update, "description"),
        call: words(update, "toolCallId"),
        output: words(update, "outputFilePath").map(PathBuf::from),
    })
}

fn state_of(state: &str) -> State {
    match state {
        "running" => State::Running,
        "paused" => State::Paused,
        "completed" => State::Done,
        "failed" => State::Failed,
        "stopped" => State::Stopped,
        other => State::Other(other.to_string()),
    }
}

/// The parameters of the request that stops one.
pub(super) fn stop(session: &str, id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session, "asyncTaskId": id })
}

/// Whether the answer to it says it stopped.
pub(super) fn stopped(answer: &serde_json::Value) -> Option<bool> {
    answer.get("stopped").and_then(serde_json::Value::as_bool)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(value: serde_json::Value) -> Meta {
        value.as_object().cloned().expect("an object")
    }

    /// Only the version Obelus reads, naming the capability it asked for,
    /// is taken as speaking it.
    ///
    /// Deliberate break: drop the version check from `offered`, and an agent
    /// on version 2 is read as version 1.
    #[test]
    fn only_the_version_obelus_reads_is_spoken() {
        let speaks = |value| offered(Some(&meta(value)));
        assert!(speaks(serde_json::json!({
            "jetbrains": { "air": { "version": 1, "capabilities": ["sessionFailure", "asyncTasks"] } }
        })));
        assert!(!speaks(serde_json::json!({
            "jetbrains": { "air": { "version": 2, "capabilities": ["asyncTasks"] } }
        })));
        assert!(!speaks(serde_json::json!({
            "jetbrains": { "air": { "version": 1, "capabilities": ["diffPatch"] } }
        })));
        assert!(!speaks(serde_json::json!({
            "jetbrains": { "air": { "version": "1", "capabilities": "asyncTasks" } }
        })));
        assert!(!offered(None));
    }

    /// What Obelus declares is what it then recognises: the two halves are
    /// one shape.
    ///
    /// Deliberate break: misspell the capability in `declare`, and an agent
    /// answering in kind is not recognised.
    #[test]
    fn what_obelus_declares_is_what_it_recognises() {
        let mut declared = Meta::new();
        declare(&mut declared);
        assert!(offered(Some(&declared)));
    }

    /// The three updates are read into Obelus's own words, and anything
    /// else is left for the protocol.
    ///
    /// Deliberate break: read `completed` as running in `state_of`, and the
    /// finished task never ends.
    #[test]
    fn the_three_updates_are_read() {
        let spawned = read(&serde_json::json!({
            "sessionUpdate": "async_task_spawned",
            "asyncTaskId": "t-1",
            "name": "npm run dev",
            "taskType": "shell",
            "description": "the dev server",
            "showInTranscript": true,
            "canStop": true,
            "outputFilePath": "/tmp/tasks/t-1.output",
            "toolCallId": "toolu_1"
        }));
        assert_eq!(
            spawned,
            Some(Ok(News::Began {
                id: "t-1".to_string(),
                name: "npm run dev".to_string(),
                kind: "shell".to_string(),
                about: Some("the dev server".to_string()),
                call: Some("toolu_1".to_string()),
                output: Some(PathBuf::from("/tmp/tasks/t-1.output")),
                stoppable: true,
            }))
        );
        let ended = read(&serde_json::json!({
            "sessionUpdate": "async_task_state_update",
            "asyncTaskId": "t-1",
            "state": "completed",
            "summary": "exited 0"
        }));
        assert!(matches!(
            ended,
            Some(Ok(News::Changed {
                state: Some(State::Done),
                summary: Some(_),
                ..
            }))
        ));
        let moved = read(&serde_json::json!({
            "sessionUpdate": "async_task_progress",
            "asyncTaskId": "t-1",
            "summary": "34/61"
        }));
        assert!(matches!(moved, Some(Ok(News::Changed { state: None, .. }))));
        assert_eq!(
            read(&serde_json::json!({ "sessionUpdate": "agent_message_chunk" })),
            None
        );
    }

    /// One of its updates that cannot be read is said to be unreadable, not
    /// passed on as though it were the protocol's.
    ///
    /// Deliberate break: answer `None` for an update with no id, and it goes
    /// on to the protocol's own parsing, which knows nothing of it.
    #[test]
    fn an_unreadable_update_is_said_to_be_one() {
        assert!(matches!(
            read(&serde_json::json!({ "sessionUpdate": "async_task_spawned", "name": "x" })),
            Some(Err(_))
        ));
        assert!(matches!(
            read(
                &serde_json::json!({ "sessionUpdate": "async_task_state_update", "asyncTaskId": "t" })
            ),
            Some(Err(_))
        ));
    }

    /// A state Obelus has not heard of is kept as it was said.
    ///
    /// Deliberate break: map unknown states to `Done`, and a task in a new
    /// state is shown as finished while it runs.
    #[test]
    fn a_state_obelus_has_not_heard_of_is_kept() {
        assert_eq!(state_of("queued"), State::Other("queued".to_string()));
        assert!(!state_of("queued").is_over());
    }

    /// A tool call says its work goes on only where the marker says so.
    ///
    /// Deliberate break: read the marker's key as `background`, and a
    /// backgrounded command says it is done.
    #[test]
    fn a_tool_call_says_its_work_goes_on() {
        let marked = meta(serde_json::json!({
            "jetbrains": { "air": { "asyncTasks": { "backgrounded": true } } }
        }));
        assert!(backgrounded(Some(&marked)));
        assert!(!backgrounded(Some(&meta(
            serde_json::json!({ "claudeCode": {} })
        ))));
        assert!(!backgrounded(None));
    }
}
