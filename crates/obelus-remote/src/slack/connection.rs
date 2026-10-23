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
    sending::{Failed, Failure, PAUSES, Saying, Stopped, tried},
    waiting::Waiting,
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
    let mut waiting = Waiting::default();
    let listener = loop {
        let (state, why) = match connect(&client, &bot, &app, &sink).await {
            Ok(listener) => break listener,
            Err(wrong) => wrong,
        };
        let _ = sink.send(Event::connection(state, Some(why)));
        if !paused(state, &sink, &mut said, &mut waiting).await {
            return;
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
    'connected: loop {
        let saying = match waiting.next() {
            Some(saying) => saying,
            None => match said.recv().await {
                Some(out) => Saying::from(out),
                None => break,
            },
        };
        let Err(stopped) = say(&session, &sink, &mut cards, &mut waiting, saying).await else {
            continue;
        };
        let why = stopped.why.clone();
        tracing::warn!(%why, "Slack stopped answering; what is to be said waits for it");
        waiting.stopped(stopped, &sink);
        let _ = sink.send(Event::connection(State::Unreachable, Some(why)));
        // The socket mends itself; the Web API is asked whether it answers
        // again, with the bot token, which is also what would say it had
        // been revoked meanwhile.
        loop {
            let state = match session.auth_test().await {
                Ok(_) => break,
                Err(error) => {
                    let state = match refused(&error) {
                        true => State::Refused,
                        false => State::Unreachable,
                    };
                    let _ = sink.send(Event::connection(state, Some(error.to_string())));
                    state
                }
            };
            if !paused(state, &sink, &mut said, &mut waiting).await {
                break 'connected;
            }
        }
        let _ = sink.send(Event::connection(State::Connected, None));
    }
    listener.shutdown().await;
}

/// Waits before Slack is tried again, keeping what is to be said meanwhile;
/// `false` where the window let go first.
async fn paused(
    state: State,
    sink: &Arc<dyn Sink<Event>>,
    said: &mut tokio::sync::mpsc::UnboundedReceiver<Out>,
    waiting: &mut Waiting,
) -> bool {
    let again = match state {
        State::Refused => 60,
        _ => 10,
    };
    let pause = tokio::time::sleep(std::time::Duration::from_secs(again));
    tokio::pin!(pause);
    loop {
        tokio::select! {
            () = &mut pause => return true,
            out = said.recv() => match out {
                Some(out) => waiting.keep(Saying::from(out), sink),
                None => return false,
            },
        }
    }
}

/// The connection everything is said through.
type Session<'a> = SlackClientSession<'a, SlackClientHyperHttpsConnector>;

/// Does one thing Obelus asked for: done, refused and said so, or handed
/// back to be done once Slack answers again.
async fn say(
    session: &Session<'_>,
    sink: &Arc<dyn Sink<Event>>,
    cards: &mut HashMap<u64, (SlackChannelId, SlackTs, String)>,
    waiting: &mut Waiting,
    saying: Saying,
) -> Result<(), Stopped> {
    let stopped = |why: String, out: Out| Stopped {
        why,
        again: Box::new(saying.again(out)),
    };
    // What became of a question that never went up as a card is said in
    // words.
    let out = match saying.out.clone() {
        Out::Settle { asked, .. } if !cards.contains_key(&asked) => saying.out.clone().in_words(),
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
            let words = waiting.prefaced(&thread, &text);
            let words = match notify {
                true => format!("<@{to}> {words}"),
                false => words,
            };
            let request = posted(
                SlackChannelId::new(room.clone()),
                words,
                Some(SlackTs::new(thread.clone())),
            );
            match post(session, request, &saying.id).await {
                Ok(_) => waiting.said_in(&thread),
                Err(Failed::Refused(why)) => {
                    tracing::warn!(thread, %why, "Slack would not take a message");
                    waiting.refused(&thread, &why, sink);
                }
                Err(Failed::Down(why)) => {
                    return Err(stopped(
                        why,
                        Out::Say {
                            room,
                            thread,
                            to,
                            text,
                            notify,
                        },
                    ));
                }
            }
        }
        Out::Open { asked, room, head } => {
            let channel = SlackChannelId::new(room.clone());
            let request = posted(channel.clone(), head.in_words(), None);
            match post(session, request, &saying.id).await {
                Ok(ts) => {
                    // Asked once: a thread with no link is still a thread.
                    let link = session
                        .chat_get_permalink(&SlackApiChatGetPermalinkRequest::new(
                            channel,
                            ts.clone(),
                        ))
                        .await
                        .ok()
                        .map(|answer| answer.permalink.to_string());
                    let _ = sink.send(Event::Opened {
                        asked,
                        thread: ts.to_string(),
                        link,
                    });
                }
                Err(Failed::Refused(why)) => {
                    tracing::warn!(%why, "Slack would not start a thread");
                    let _ = sink.send(Event::Unopened {
                        asked,
                        waited: false,
                    });
                }
                Err(Failed::Down(why)) => {
                    return Err(stopped(why, Out::Open { asked, room, head }));
                }
            }
        }
        // Slack edits a message for as long as it is there, so the head is
        // the first message's text, said again.
        Out::Retitle { room, thread, head } => {
            let mut content = SlackMessageContent::new();
            content.markdown_text = Some(head.in_words());
            let request = SlackApiChatUpdateRequest::new(
                SlackChannelId::new(room.clone()),
                content,
                SlackTs::new(thread.clone()),
            );
            match tried(&PAUSES, |_| async {
                session.chat_update(&request).await.map_err(failure)
            })
            .await
            {
                Ok(_) => {}
                Err(Failed::Refused(why)) => {
                    tracing::warn!(%why, "Slack would not say what a thread is again");
                }
                Err(Failed::Down(why)) => {
                    return Err(stopped(why, Out::Retitle { room, thread, head }));
                }
            }
        }
        Out::Name { id } => {
            let request = SlackApiUsersInfoRequest::new(SlackUserId::new(id.clone()));
            match tried(&PAUSES, |_| async {
                session.users_info(&request).await.map_err(failure)
            })
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
                Err(Failed::Refused(why)) => {
                    tracing::warn!(%why, "Slack would not say who that is")
                }
                Err(Failed::Down(why)) => return Err(stopped(why, Out::Name { id })),
            }
        }
        Out::Ask {
            room,
            thread,
            to,
            asked,
            question,
        } => {
            let channel = SlackChannelId::new(room.clone());
            let mut content = SlackMessageContent::new();
            // What a notification shows, which blocks are not.
            content.text = Some(format!("<@{to}> {}", question.about));
            content.blocks = Some(asking(asked, &question, &to));
            let mut request = SlackApiChatPostMessageRequest::new(channel.clone(), content);
            request.thread_ts = Some(SlackTs::new(thread.clone()));
            match post(session, request, &saying.id).await {
                Ok(ts) => {
                    cards.insert(asked, (channel, ts, question.about));
                }
                Err(Failed::Down(why)) => {
                    return Err(stopped(
                        why,
                        Out::Ask {
                            room,
                            thread,
                            to,
                            asked,
                            question,
                        },
                    ));
                }
                // Said in words rather than not at all: see Feishu's.
                Err(Failed::Refused(why)) => {
                    tracing::warn!(%why, asked, "Slack would not take a question");
                    let _ = sink.send(Event::Unasked { asked });
                    Box::pin(say(
                        session,
                        sink,
                        cards,
                        waiting,
                        saying.instead(Out::Say {
                            room,
                            thread,
                            to,
                            text: question.in_words(),
                            notify: true,
                        }),
                    ))
                    .await?;
                }
            }
        }
        Out::Settle {
            room,
            thread,
            to,
            asked,
            said,
        } => {
            if let Some((channel, ts, about)) = cards.get(&asked).cloned() {
                let mut content = SlackMessageContent::new();
                content.text = Some(about.clone());
                content.blocks = Some(settled(&about, &said));
                let request = SlackApiChatUpdateRequest::new(channel, content, ts);
                match tried(&PAUSES, |_| async {
                    session.chat_update(&request).await.map_err(failure)
                })
                .await
                {
                    Ok(_) => {
                        cards.remove(&asked);
                    }
                    Err(Failed::Refused(why)) => {
                        tracing::warn!(%why, asked, "Slack would not close a question");
                        cards.remove(&asked);
                    }
                    Err(Failed::Down(why)) => {
                        return Err(stopped(
                            why,
                            Out::Settle {
                                room,
                                thread,
                                to,
                                asked,
                                said,
                            },
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// What a message Obelus posts is marked with, so that a try that timed
/// out can be asked after: Slack has no key that makes a second posting
/// the first one, as Feishu's `uuid` does, but it keeps a message's
/// metadata and gives it back when asked.
const SAID: &str = "obelus_said";

/// Posts `request` under `id`, asked again while Slack does not answer --
/// and before each asking again, whether the last was taken after all, so
/// that a reply that was lost on its way back is not a message twice.
async fn post(
    session: &Session<'_>,
    mut request: SlackApiChatPostMessageRequest,
    id: &str,
) -> Result<SlackTs, Failed> {
    request.content.metadata = Some(SlackMessageMetadata {
        event_type: SAID.to_string(),
        event_payload: Some(HashMap::from([("id".to_string(), id.to_string())])),
    });
    // From a little before the first try, for a clock that is not Slack's.
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(60);
    let since = format!("{since}.000000");
    tried(&PAUSES, |tries| {
        let request = &request;
        let since = &since;
        async move {
            if tries > 0
                && let Some(ts) = landed(session, request, since, id).await.map_err(failure)?
            {
                tracing::info!(id, "a message Slack had taken after all");
                return Ok(ts);
            }
            session
                .chat_post_message(request)
                .await
                .map(|posted| posted.ts)
                .map_err(failure)
        }
    })
    .await
}

/// Where the message `request` posts under `id` is, if Slack has it.
///
/// Read as JSON rather than as the crate's messages: those take metadata's
/// values to be strings, and another app's in the same channel need not be,
/// which would make the whole answer unreadable.
async fn landed(
    session: &Session<'_>,
    request: &SlackApiChatPostMessageRequest,
    since: &str,
    id: &str,
) -> Result<Option<SlackTs>, SlackClientError> {
    let channel = request.channel.to_string();
    let answer: Value = match &request.thread_ts {
        Some(thread) => {
            let thread = thread.to_string();
            let asked = vec![
                ("channel", Some(channel.as_str())),
                ("ts", Some(thread.as_str())),
                ("oldest", Some(since)),
                ("include_all_metadata", Some("true")),
            ];
            session
                .http_session_api
                .http_get("conversations.replies", &asked, None)
                .await?
        }
        None => {
            let asked = vec![
                ("channel", Some(channel.as_str())),
                ("oldest", Some(since)),
                ("include_all_metadata", Some("true")),
            ];
            session
                .http_session_api
                .http_get("conversations.history", &asked, None)
                .await?
        }
    };
    Ok(found(&answer, id))
}

/// The message in an answer of Slack's that was posted under `id`.
fn found(answer: &Value, id: &str) -> Option<SlackTs> {
    answer["messages"]
        .as_array()?
        .iter()
        .find(|message| {
            message["metadata"]["event_type"] == SAID
                && message["metadata"]["event_payload"]["id"] == id
        })
        .and_then(|message| message["ts"].as_str())
        .map(|ts| SlackTs::new(ts.to_string()))
}

/// Codes Slack answers with that say ask again later.
const AGAIN: &[&str] = &[
    "ratelimited",
    "internal_error",
    "fatal_error",
    "service_unavailable",
    "request_timeout",
];

/// Why one try did not do it, and whether to ask again: no answer, an
/// answer that says later, or a token gone bad meanwhile -- which the
/// connection is asked about again, and says -- are all worth it; anything
/// else Slack said no to is the same no next time.
fn failure(error: SlackClientError) -> Failure {
    let why = error.to_string();
    match &error {
        SlackClientError::RateLimitError(limit) => Failure::Again {
            why,
            after: limit.retry_after,
        },
        SlackClientError::HttpError(http)
            if http.status_code.is_server_error() || http.status_code.as_u16() == 429 =>
        {
            Failure::again(why)
        }
        SlackClientError::HttpProtocolError(_) | SlackClientError::EndOfStream(_) => {
            Failure::again(why)
        }
        SlackClientError::ApiError(said) if AGAIN.contains(&said.code.as_str()) => {
            Failure::again(why)
        }
        _ if refused(&error) => Failure::again(why),
        _ => Failure::Refused(why),
    }
}

// The bot token checked and the socket up, or the state that says why not
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

/// What Slack takes in a section's text, less room for calling the reader,
/// and in a button's or an option's: a message past either is refused whole.
const ABOUT: usize = 2900;
const NAME: usize = 75;

/// A question as blocks: what it is about, calling the reader, then a
/// button for each answer where one press is the whole answer, or a list
/// and a box with a button that sends them -- and beside those a button
/// that gives up on it, as escape does on the card here. Every press names
/// the process that asked and the question, by `asked` -- with the answer's
/// id after another colon, on a button that is one.
fn asking(asked: u64, question: &Question, to: &str) -> Vec<SlackBlock> {
    let plain = |text: &str| json!({ "type": "plain_text", "text": text });
    let cancel = json!({
        "type": "button",
        "text": plain("Cancel"),
        "action_id": "cancel",
        "value": format!("{}:{asked}", crate::this_process()),
    });
    let mut blocks = vec![json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": format!("<@{to}> {}", crate::capped(&question.about, ABOUT)),
        },
    })];
    if !question.several && question.words.is_none() && !question.choices.is_empty() {
        let buttons: Vec<Value> = question
            .choices
            .iter()
            .enumerate()
            .map(|(at, (id, name))| {
                json!({
                    "type": "button",
                    "text": plain(&crate::capped(name, NAME)),
                    "action_id": format!("choice-{at}"),
                    "value": format!("{}:{asked}:{id}", crate::this_process()),
                })
            })
            .chain([cancel])
            .collect();
        blocks.push(json!({ "type": "actions", "block_id": "answers", "elements": buttons }));
    } else {
        if !question.choices.is_empty() {
            let options: Vec<Value> = question
                .choices
                .iter()
                .map(|(id, name)| json!({ "text": plain(&crate::capped(name, NAME)), "value": id }))
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
                "label": plain(&crate::capped(name, NAME)),
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
            }, cancel],
        }));
    }
    serde_json::from_value(Value::Array(blocks)).unwrap_or_default()
}

/// The same question closed: what it was about, and what became of it in
/// place of anything to press.
fn settled(about: &str, said: &str) -> Vec<SlackBlock> {
    // Nothing for a question with nothing said about it -- a permission
    // whose call said it already -- because Slack refuses a section with no
    // text, and the message would stay open.
    let mut blocks = Vec::new();
    if !about.is_empty() {
        blocks.push(json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": crate::capped(about, ABOUT) },
        }));
    }
    blocks.push(json!({ "type": "context", "elements": [{ "type": "mrkdwn", "text": said }] }));
    serde_json::from_value(Value::Array(blocks)).unwrap_or_default()
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
    let action = event.actions.as_ref()?.first()?;
    let value = action.value.clone()?;
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
    if action.action_id.0 == "cancel" {
        tracing::info!(asked, "a question given up on on its message");
        return Some(Event::Cancelled { from, asked });
    }
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
        assert_eq!(
            blocks[1]["elements"][2]["action_id"], "cancel",
            "{blocks:#}"
        );
        assert_eq!(blocks[1]["elements"][2]["value"], format!("{by}:7"));

        let blocks = drawn(&question(true, Some(("Other".to_string(), false))));
        assert_eq!(blocks.as_array().map(Vec::len), Some(4), "{blocks:#}");
        assert_eq!(blocks[1]["element"]["type"], "multi_static_select");
        assert_eq!(blocks[1]["element"]["options"][1]["value"], "never");
        assert_eq!(blocks[2]["element"]["type"], "plain_text_input");
        assert_eq!(blocks[2]["optional"], true);
        assert_eq!(blocks[3]["elements"][0]["value"], format!("{by}:7"));
        assert_eq!(
            blocks[3]["elements"][1]["action_id"], "cancel",
            "{blocks:#}"
        );

        // Closed, a question with nothing said about it is the line saying
        // what became of it and no empty section, which Slack refuses.
        let closed = serde_json::to_value(settled("", "\u{2714} Allow once")).expect("blocks");
        assert_eq!(closed.as_array().map(Vec::len), Some(1), "{closed:#}");
        assert_eq!(closed[0]["type"], "context");

        // A name past what Slack takes on a button is cut to fit, or the
        // whole question is refused.
        let mut long = question(false, None);
        long.choices[0].1 = "a".repeat(200);
        let blocks = drawn(&long);
        let shown = blocks[1]["elements"][0]["text"]["text"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(shown.chars().count(), 75, "{blocks:#}");
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

    /// The cancel button is heard as giving up, not as an answer of
    /// nothing -- which a permission takes as a refusal and a question as
    /// empty -- though its value is the send button's.
    ///
    /// Broken deliberately by not reading which button it was: the press
    /// came back as `Answered` with nothing chosen.
    #[test]
    fn cancel_is_heard_as_giving_up() {
        let by = crate::this_process();
        let event: SlackInteractionEvent = serde_json::from_value(json!({
            "type": "block_actions",
            "team": { "id": "T1" },
            "user": { "id": "U1" },
            "api_app_id": "A1",
            "container": { "type": "message", "message_ts": "1.2", "channel_id": "C1" },
            "trigger_id": "t",
            "actions": [{ "type": "button", "action_id": "cancel", "block_id": "answers", "value": format!("{by}:7") }],
        }))
        .expect("a press");
        let SlackInteractionEvent::BlockActions(event) = event else {
            panic!("not a press");
        };
        let heard = answer_of(&event);
        assert!(
            matches!(&heard, Some(Event::Cancelled { from, asked: 7 }) if from == "U1"),
            "{heard:?}"
        );
    }

    /// A message posted under an id is found by it, among messages with
    /// metadata of every shape -- another app's need not be strings.
    ///
    /// Broken deliberately twice. Matching on the event type alone: the
    /// first message Obelus had ever posted there was taken for this one.
    /// And reading the answer through the crate's types: another app's
    /// number in its metadata made the answer unreadable and nothing was
    /// found.
    #[test]
    fn a_message_is_found_by_its_id() {
        let answer = json!({
            "ok": true,
            "messages": [
                { "ts": "1.0", "metadata": { "event_type": "theirs", "event_payload": { "n": 3 } } },
                { "ts": "2.0", "metadata": { "event_type": SAID, "event_payload": { "id": "a-1" } } },
                { "ts": "3.0", "metadata": { "event_type": SAID, "event_payload": { "id": "a-2" } } },
                { "ts": "4.0" },
            ],
        });
        assert_eq!(found(&answer, "a-2"), Some(SlackTs::new("3.0".to_string())));
        assert_eq!(found(&answer, "a-3"), None);
    }

    /// What Slack answers is asked again where it says later or did not
    /// answer, and taken as a no where it said no.
    ///
    /// Broken deliberately twice. Dropping Slack's own wait: a rate limit
    /// was asked again after a second, when Slack had said thirty. And
    /// asking every API error again: `msg_too_long` was asked four times.
    #[test]
    fn what_slack_answers_is_asked_again_or_refused() {
        use slack_morphism::errors::{
            SlackClientApiError, SlackClientHttpError, SlackRateLimitError,
        };
        let api =
            |code: &str| SlackClientError::ApiError(SlackClientApiError::new(code.to_string()));
        let limited = SlackClientError::RateLimitError(
            SlackRateLimitError::new().with_retry_after(std::time::Duration::from_secs(30)),
        );
        assert!(matches!(
            failure(limited),
            Failure::Again { after: Some(after), .. } if after == std::time::Duration::from_secs(30)
        ));
        assert!(matches!(failure(api("ratelimited")), Failure::Again { .. }));
        assert!(matches!(
            failure(api("internal_error")),
            Failure::Again { .. }
        ));
        assert!(matches!(
            failure(api("token_revoked")),
            Failure::Again { .. }
        ));
        assert!(matches!(failure(api("msg_too_long")), Failure::Refused(_)));
        assert!(matches!(
            failure(SlackClientError::HttpError(SlackClientHttpError::new(
                reqwest::StatusCode::BAD_GATEWAY
            ))),
            Failure::Again { .. }
        ));
    }
}
