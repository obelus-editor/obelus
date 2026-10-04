//! Slack, connected: Socket Mode in, the Web API out.
//!
//! **One task and two channels.** What Obelus wants said arrives on one
//! channel and is said in the order it arrived -- a thread's first message
//! before its replies, which Slack would otherwise let race -- and what Slack
//! says goes out as `Event`s through the sink. Dropping the sender is how it
//! is stopped: the task sees its channel close, shuts the socket and ends.
//!
//! **What is heard is only what a person wrote in a channel.** The
//! app's own messages come back to it as events too, and an edit, a join or
//! a deleted message is a message with a subtype; none of those is somebody
//! talking, so none of them is passed on.
//!
//! **A question is a message with buttons, and a press is its answer.** A
//! button for each answer where one press is the whole of it; otherwise a
//! list and a box, and a button that sends what they hold -- Slack hands the
//! state of every input on the message over with the press. The press
//! names the question by Obelus's number for it, and the message is edited
//! closed once Obelus says what became of it.

use std::{collections::HashMap, sync::Arc};

use obelus_sink::Sink;
use serde_json::{Value, json};
use slack_morphism::{errors::SlackClientError, listener::HttpStatusCode, prelude::*};

use crate::{
    Event, State,
    model::{Out, Question, Where},
};

/// What the listener's callbacks share: where what they hear goes.
///
/// Through the listener's own store of state, because its callbacks are
/// plain functions and cannot close over anything.
struct Listening {
    sink: Arc<dyn Sink<Event>>,
}

/// Connects with these tokens and returns where to send what is to be said.
///
/// Says where it has got to through the sink as it goes: connecting, then
/// connected, or a token refused -- after which it stops, because trying a
/// refused token again is asking the same question of the same answer. A
/// Slack that cannot be reached is tried again, every few seconds, by the
/// socket's own reconnecting.
#[must_use]
pub fn start(
    app_token: String,
    bot_token: String,
    sink: Arc<dyn Sink<Event>>,
) -> tokio::sync::mpsc::UnboundedSender<Out> {
    let (out, said) = tokio::sync::mpsc::unbounded_channel();
    obelus_runtime::handle().spawn(run(app_token, bot_token, sink, said));
    out
}

async fn run(
    app_token: String,
    bot_token: String,
    sink: Arc<dyn Sink<Event>>,
    mut said: tokio::sync::mpsc::UnboundedReceiver<Out>,
) {
    let _ = sink.send(Event::connection(State::Connecting, None));
    let connector = match SlackClientHyperConnector::new() {
        Ok(connector) => connector,
        Err(error) => {
            tracing::warn!(%error, "no connector to reach Slack with");
            let _ = sink.send(Event::connection(
                State::Unreachable,
                Some(error.to_string()),
            ));
            return;
        }
    };
    let client = Arc::new(SlackClient::new(connector));
    let bot = SlackApiToken::new(bot_token.into());
    let app = SlackApiToken::new(app_token.into());

    // Tried until it is up, a refusal less often: a machine started before
    // its network is a machine that would otherwise never connect, and a
    // reader who mends a token starts a new connection anyway. What is to
    // be said meanwhile waits, in order -- as much of it as is kept -- and
    // a window letting go ends it.
    let mut waiting = crate::waiting::Waiting::default();
    let listener = loop {
        let (state, why) = match connect(&client, &bot, &app, &sink).await {
            Ok(listener) => break listener,
            Err(wrong) => wrong,
        };
        let _ = sink.send(Event::connection(state, Some(why)));
        let again = match state {
            State::Refused => 60,
            _ => 10,
        };
        let pause = tokio::time::sleep(std::time::Duration::from_secs(again));
        tokio::pin!(pause);
        loop {
            tokio::select! {
                () = &mut pause => break,
                out = said.recv() => match out {
                    Some(out) => waiting.keep(out, &sink),
                    None => return,
                },
            }
        }
    };
    // Connected once the socket is up, which `start` waits for. Slack's
    // own hello would say it a moment later, but the crate does not name
    // the type a listener for it would have to take.
    let _ = sink.send(Event::connection(State::Connected, None));

    let session = client.open_session(&bot);
    // Each question's message, by Obelus's number for it: where it is, and
    // what it was about, to edit it closed with.
    let mut cards: HashMap<u64, (SlackChannelId, SlackTs, String)> = HashMap::new();
    let mut waiting = waiting.drain().collect::<Vec<Out>>().into_iter();
    while let Some(out) = match waiting.next() {
        Some(out) => Some(out),
        None => said.recv().await,
    } {
        // What became of a question that never went up as a card is said
        // in words.
        let out = match out {
            Out::Settle { asked, .. } if !cards.contains_key(&asked) => out.in_words(),
            out => out,
        };
        match out {
            Out::Say {
                room,
                thread,
                to,
                text,
                notify,
            } => {
                let text = match notify {
                    true => format!("<@{to}> {text}"),
                    false => text,
                };
                if let Err(error) = session
                    .chat_post_message(&posted(
                        SlackChannelId::new(room),
                        text,
                        Some(SlackTs::new(thread)),
                    ))
                    .await
                {
                    tracing::warn!(%error, "Slack would not take a message");
                }
            }
            Out::Open { asked, room, head } => {
                let channel = SlackChannelId::new(room);
                match session
                    .chat_post_message(&posted(channel.clone(), head.in_words(), None))
                    .await
                {
                    Ok(posted) => {
                        let link = session
                            .chat_get_permalink(&SlackApiChatGetPermalinkRequest::new(
                                channel,
                                posted.ts.clone(),
                            ))
                            .await
                            .ok()
                            .map(|answer| answer.permalink.to_string());
                        let _ = sink.send(Event::Opened {
                            asked,
                            thread: posted.ts.to_string(),
                            link,
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Slack would not start a thread");
                        let _ = sink.send(Event::Unopened {
                            asked,
                            waited: false,
                        });
                    }
                }
            }
            // Slack edits a message for as long as it is there, so the
            // head is the first message's text, said again.
            Out::Retitle { room, thread, head } => {
                let mut content = SlackMessageContent::new();
                content.markdown_text = Some(head.in_words());
                if let Err(error) = session
                    .chat_update(&SlackApiChatUpdateRequest::new(
                        SlackChannelId::new(room),
                        content,
                        SlackTs::new(thread),
                    ))
                    .await
                {
                    tracing::warn!(%error, "Slack would not say what a thread is again");
                }
            }
            Out::Name { id } => {
                match session
                    .users_info(&SlackApiUsersInfoRequest::new(SlackUserId::new(id.clone())))
                    .await
                {
                    Ok(answer) => {
                        let user = answer.user;
                        let name = user
                            .profile
                            .as_ref()
                            .and_then(|profile| profile.display_name.clone())
                            .filter(|name| !name.is_empty())
                            .or(user.real_name.clone())
                            .or(user.name.clone())
                            .unwrap_or_else(|| id.clone());
                        let _ = sink.send(Event::Named { id, name });
                    }
                    Err(error) => tracing::warn!(%error, "Slack would not say who that is"),
                }
            }
            Out::Ask {
                room,
                thread,
                to,
                asked,
                question,
            } => {
                let channel = SlackChannelId::new(room);
                let mut content = SlackMessageContent::new();
                // What a notification shows, which blocks are not.
                content.text = Some(format!("<@{to}> {}", question.about));
                content.blocks = Some(asking(asked, &question, &to));
                let mut request = SlackApiChatPostMessageRequest::new(channel.clone(), content);
                request.thread_ts = Some(SlackTs::new(thread));
                match session.chat_post_message(&request).await {
                    Ok(posted) => {
                        cards.insert(asked, (channel, posted.ts, question.about));
                    }
                    Err(error) => tracing::warn!(%error, asked, "Slack would not take a question"),
                }
            }
            Out::Settle { asked, said, .. } => {
                if let Some((channel, ts, about)) = cards.remove(&asked) {
                    let mut content = SlackMessageContent::new();
                    content.text = Some(about.clone());
                    content.blocks = Some(settled(&about, &said));
                    if let Err(error) = session
                        .chat_update(&SlackApiChatUpdateRequest::new(channel, content, ts))
                        .await
                    {
                        tracing::warn!(%error, asked, "Slack would not close a question");
                    }
                }
            }
        }
    }
    listener.shutdown().await;
}

/// The bot token checked and the socket up, or the state that says why not
/// and what Slack said.
async fn connect(
    client: &Arc<SlackHyperClient>,
    bot: &SlackApiToken,
    app: &SlackApiToken,
    sink: &Arc<dyn Sink<Event>>,
) -> Result<SlackClientSocketModeListener<SlackClientHyperHttpsConnector>, (State, String)> {
    // The bot token first, because a wrong one is the commonest way this
    // goes wrong and the socket would not say so: it is connected with the
    // other token, and a bad bot token is found out at the first reply.
    if let Err(error) = client.open_session(bot).auth_test().await {
        tracing::warn!(%error, "Slack refused the bot token, or could not be reached");
        let state = match refused(&error) {
            true => State::Refused,
            false => State::Unreachable,
        };
        return Err((state, error.to_string()));
    }
    let environment = Arc::new(
        SlackClientEventsListenerEnvironment::new(client.clone())
            .with_error_handler(said_wrong)
            .with_user_state(Listening { sink: sink.clone() }),
    );
    let callbacks = SlackSocketModeListenerCallbacks::new()
        .with_push_events(pushed)
        .with_interaction_events(pressed);
    let listener = SlackClientSocketModeListener::new(
        &SlackClientSocketModeConfig::new(),
        environment,
        callbacks,
    );
    if let Err(error) = listener.listen_for(app).await {
        tracing::warn!(%error, "Slack refused the app token, or could not be reached");
        let state = match refused(&error) {
            true => State::Refused,
            false => State::Unreachable,
        };
        return Err((state, error.to_string()));
    }
    listener.start().await;
    Ok(listener)
}

/// A message in markdown, which Slack draws itself.
fn posted(
    channel: SlackChannelId,
    text: String,
    thread: Option<SlackTs>,
) -> SlackApiChatPostMessageRequest {
    let mut content = SlackMessageContent::new();
    content.markdown_text = Some(text);
    let mut request = SlackApiChatPostMessageRequest::new(channel, content);
    request.thread_ts = thread;
    request
}

/// Whether Slack said no to a token, rather than not being there to ask.
fn refused(error: &SlackClientError) -> bool {
    matches!(
        error,
        SlackClientError::ApiError(said)
            if matches!(said.code.as_str(), "invalid_auth" | "not_authed" | "account_inactive" | "token_revoked")
    )
}

async fn pushed(
    event: SlackPushEventCallback,
    _: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> UserCallbackResult<()> {
    let SlackEventCallbackBody::Message(message) = event.event else {
        return Ok(());
    };
    // Somebody writing, in a channel, and nothing else: see the module's
    // own note. Which channel is Obelus's to judge.
    if message.subtype.is_some()
        || message.sender.bot_id.is_some()
        || message.origin.channel_type == Some(SlackChannelType::new("im".to_string()))
    {
        return Ok(());
    }
    let (Some(from), Some(room), Some(text)) = (
        message.sender.user,
        message.origin.channel,
        message.content.and_then(|content| content.text),
    ) else {
        return Ok(());
    };
    // A message outside any thread starts one, whose replies hang under
    // it: Slack has no group of topics, and a channel whose every message
    // is a thread is the nearest thing to one.
    let at = match message.origin.thread_ts {
        Some(thread) => Where::Thread(thread.to_string()),
        None => Where::Fresh(message.origin.ts.to_string()),
    };
    if let Some(listening) = states.read().await.get_user_state::<Listening>() {
        let _ = listening.sink.send(Event::Heard {
            from: from.to_string(),
            room: room.to_string(),
            at,
            text: unescaped(&text),
        });
    }
    Ok(())
}

/// A question as blocks: what it is about, calling the reader, then a
/// button for each answer where one press is the whole answer, or a list
/// and a box with a button that sends them. Every press names the process
/// that asked and the question, by `asked` -- with the answer's id after
/// another colon, on a button that is one.
fn asking(asked: u64, question: &Question, to: &str) -> Vec<SlackBlock> {
    let plain = |text: &str| json!({ "type": "plain_text", "text": text });
    let mut blocks = vec![json!({
        "type": "section",
        "text": { "type": "mrkdwn", "text": format!("<@{to}> {}", question.about) },
    })];
    if !question.several && question.words.is_none() && !question.choices.is_empty() {
        let buttons: Vec<Value> = question
            .choices
            .iter()
            .enumerate()
            .map(|(at, (id, name))| {
                json!({
                    "type": "button",
                    "text": plain(name),
                    "action_id": format!("choice-{at}"),
                    "value": format!("{}:{asked}:{id}", crate::this_process()),
                })
            })
            .collect();
        blocks.push(json!({ "type": "actions", "block_id": "answers", "elements": buttons }));
    } else {
        if !question.choices.is_empty() {
            let options: Vec<Value> = question
                .choices
                .iter()
                .map(|(id, name)| json!({ "text": plain(name), "value": id }))
                .collect();
            blocks.push(json!({
                "type": "input",
                "block_id": "chosen",
                "optional": !question.needed,
                "label": plain("Choose"),
                "element": {
                    "type": match question.several {
                        true => "multi_static_select",
                        false => "static_select",
                    },
                    "action_id": "chosen",
                    "options": options,
                },
            }));
        }
        if let Some((name, required)) = &question.words {
            blocks.push(json!({
                "type": "input",
                "block_id": "words",
                "optional": !required,
                "label": plain(name),
                "element": { "type": "plain_text_input", "action_id": "words" },
            }));
        }
        blocks.push(json!({
            "type": "actions",
            "block_id": "send",
            "elements": [{
                "type": "button",
                "style": "primary",
                "text": plain("Send"),
                "action_id": "send",
                "value": format!("{}:{asked}", crate::this_process()),
            }],
        }));
    }
    serde_json::from_value(Value::Array(blocks)).unwrap_or_default()
}

/// The same question closed: what it was about, and what became of it in
/// place of anything to press.
fn settled(about: &str, said: &str) -> Vec<SlackBlock> {
    serde_json::from_value(json!([
        { "type": "section", "text": { "type": "mrkdwn", "text": about } },
        { "type": "context", "elements": [{ "type": "mrkdwn", "text": said }] },
    ]))
    .unwrap_or_default()
}

async fn pressed(
    event: SlackInteractionEvent,
    _: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> UserCallbackResult<()> {
    let SlackInteractionEvent::BlockActions(event) = event else {
        return Ok(());
    };
    let Some(answered) = answer_of(&event) else {
        return Ok(());
    };
    if let Some(listening) = states.read().await.get_user_state::<Listening>() {
        let _ = listening.sink.send(answered);
    }
    Ok(())
}

/// The answer a press makes: which question, the ids chosen -- the one on
/// a button, or what the list held when it was sent -- and the words in the
/// box.
fn answer_of(event: &SlackInteractionBlockActionsEvent) -> Option<Event> {
    let from = event.user.as_ref()?.id.to_string();
    let value = event.actions.as_ref()?.first()?.value.clone()?;
    let Some(value) = value
        .strip_prefix(crate::this_process())
        .and_then(|value| value.strip_prefix(':'))
    else {
        tracing::info!("a press on a message another window put up");
        return None;
    };
    let (asked, chosen) = match value.split_once(':') {
        Some((asked, id)) => (asked, vec![id.to_string()]),
        None => (value, Vec::new()),
    };
    let asked = asked.parse().ok()?;
    let held = |block: &str| {
        event
            .state
            .as_ref()?
            .values
            .get(&SlackBlockId::new(block.to_string()))?
            .get(&SlackActionId::new(block.to_string()))
            .cloned()
    };
    let chosen = match (chosen.is_empty(), held("chosen")) {
        (true, Some(list)) => list
            .selected_options
            .unwrap_or_default()
            .into_iter()
            .chain(list.selected_option)
            .map(|option| option.value)
            .collect(),
        _ => chosen,
    };
    let words = held("words")
        .and_then(|words| words.value)
        .map(|words| words.trim().to_string())
        .filter(|words| !words.is_empty());
    tracing::info!(asked, ?chosen, "a question answered on its message");
    Some(Event::Answered {
        from,
        asked,
        chosen,
        words,
    })
}

/// What the listener could not handle, for the log.
fn said_wrong(
    error: Box<dyn std::error::Error + Send + Sync>,
    _: Arc<SlackHyperClient>,
    _: SlackClientEventsUserState,
) -> HttpStatusCode {
    tracing::warn!(%error, "something Slack sent could not be handled");
    HttpStatusCode::OK
}

/// What somebody wrote, as they wrote it. Slack escapes three characters in
/// what it hands over, and code is made of them: `a < b` would reach the
/// agent as `a &lt; b`. The ampersand last, so that `&amp;lt;` -- somebody
/// writing the escape itself -- stays `&lt;`.
fn unescaped(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Broken deliberately by undoing the ampersand first: `&amp;lt;`
    /// came out as `<`, which is not what was written.
    #[test]
    fn what_slack_escapes_is_given_back_as_written() {
        assert_eq!(
            unescaped("if a &lt; b &amp;&amp; c &gt; d"),
            "if a < b && c > d"
        );
        assert_eq!(unescaped("&amp;lt;"), "&lt;");
    }

    /// A question one press answers is a button per answer, carrying the
    /// question's number and the agent's id; one that takes several, or
    /// words, is a list and a box and a button that sends them -- and both
    /// are blocks Slack's types take, not an empty message.
    ///
    /// Broken deliberately three ways. Drawing a list whatever the
    /// question: the permission had no buttons. Putting the name where the
    /// id goes: the press carried "Allow once". And a block Slack's types
    /// do not take -- the multi-select in an `actions` block: the form came
    /// out as no blocks at all.
    #[test]
    fn a_question_is_buttons_or_a_form() {
        let question = |several: bool, words: Option<(String, bool)>| Question {
            about: "Read the file?".to_string(),
            choices: vec![
                ("once".to_string(), "Allow once".to_string()),
                ("never".to_string(), "Reject".to_string()),
            ],
            several,
            needed: true,
            words,
        };
        let drawn =
            |question: &Question| serde_json::to_value(asking(7, question, "U1")).expect("blocks");

        let blocks = drawn(&question(false, None));
        assert_eq!(blocks[0]["text"]["text"], "<@U1> Read the file?");
        assert_eq!(blocks[1]["type"], "actions", "{blocks:#}");
        let by = crate::this_process();
        assert_eq!(blocks[1]["elements"][0]["value"], format!("{by}:7:once"));
        assert_eq!(blocks[1]["elements"][1]["value"], format!("{by}:7:never"));

        let blocks = drawn(&question(true, Some(("Other".to_string(), false))));
        assert_eq!(blocks.as_array().map(Vec::len), Some(4), "{blocks:#}");
        assert_eq!(blocks[1]["element"]["type"], "multi_static_select");
        assert_eq!(blocks[1]["element"]["options"][1]["value"], "never");
        assert_eq!(blocks[2]["element"]["type"], "plain_text_input");
        assert_eq!(blocks[2]["optional"], true);
        assert_eq!(blocks[3]["elements"][0]["value"], format!("{by}:7"));
    }

    /// A press is heard as the answer it makes: a button's question and
    /// id, or the send button's question with what the list and the box
    /// held -- trimmed, and only where there is something in the box.
    ///
    /// Broken deliberately three ways. Reading only the button's value: what
    /// the list held arrived as nothing chosen. Reading only several from
    /// the list: the one chosen from a single list was dropped. And not
    /// reading whose message it was: the press on the closed window's
    /// question answered this one's.
    #[test]
    fn a_press_is_heard_as_its_answer() {
        let press = |value: &str, state: Value| {
            let event: SlackInteractionEvent = serde_json::from_value(json!({
                "type": "block_actions",
                "team": { "id": "T1" },
                "user": { "id": "U1" },
                "api_app_id": "A1",
                "container": { "type": "message", "message_ts": "1.2", "channel_id": "C1" },
                "trigger_id": "t",
                "actions": [{ "type": "button", "action_id": "send", "block_id": "send", "value": value }],
                "state": { "values": state },
            }))
            .expect("a press");
            let SlackInteractionEvent::BlockActions(event) = event else {
                panic!("not a press");
            };
            match answer_of(&event) {
                Some(Event::Answered {
                    from,
                    asked,
                    chosen,
                    words,
                }) => Some((from, asked, chosen, words)),
                _ => None,
            }
        };
        let by = crate::this_process();
        assert_eq!(press("another:7:once", json!({})), None);
        assert_eq!(
            press(&format!("{by}:7:once"), json!({})),
            Some(("U1".to_string(), 7, vec!["once".to_string()], None))
        );
        assert_eq!(
            press(
                &format!("{by}:8"),
                json!({
                    "chosen": { "chosen": { "type": "multi_static_select", "selected_options": [
                        { "text": { "type": "plain_text", "text": "App" }, "value": "app" },
                        { "text": { "type": "plain_text", "text": "UI" }, "value": "ui" },
                    ] } },
                    "words": { "words": { "type": "plain_text_input", "value": " the docs too  " } },
                })
            ),
            Some((
                "U1".to_string(),
                8,
                vec!["app".to_string(), "ui".to_string()],
                Some("the docs too".to_string())
            ))
        );
        assert_eq!(
            press(
                &format!("{by}:9"),
                json!({
                    "chosen": { "chosen": { "type": "static_select", "selected_option":
                        { "text": { "type": "plain_text", "text": "App" }, "value": "app" } } },
                    "words": { "words": { "type": "plain_text_input", "value": null } },
                })
            ),
            Some(("U1".to_string(), 9, vec!["app".to_string()], None))
        );
    }
}
