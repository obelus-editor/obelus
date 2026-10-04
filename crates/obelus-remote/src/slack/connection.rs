//! Slack, connected: Socket Mode in, the Web API out.
//!
//! **One task and two channels.** What Obelus wants said arrives on one
//! channel and is said in the order it arrived -- a thread's first message
//! before its replies, which Slack would otherwise let race -- and what Slack
//! says goes out as `Event`s through the sink. Dropping the sender is how it
//! is stopped: the task sees its channel close, shuts the socket and ends.
//!
//! **What is heard is only what a person wrote in a direct message.** The
//! app's own messages come back to it as events too, and an edit, a join or
//! a deleted message is a message with a subtype; none of those is somebody
//! talking, so none of them is passed on.

use std::{collections::HashMap, sync::Arc};

use obelus_sink::Sink;
use slack_morphism::{errors::SlackClientError, listener::HttpStatusCode, prelude::*};

use crate::{
    Event, State,
    model::{Out, Where},
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
    let _ = sink.send(Event::Connection(State::Connecting));
    let connector = match SlackClientHyperConnector::new() {
        Ok(connector) => connector,
        Err(error) => {
            tracing::warn!(%error, "no connector to reach Slack with");
            let _ = sink.send(Event::Connection(State::Unreachable));
            return;
        }
    };
    let client = Arc::new(SlackClient::new(connector));
    let bot = SlackApiToken::new(bot_token.into());

    // The bot token first, because a wrong one is the commonest way this
    // goes wrong and the socket would not say so: it is connected with the
    // other token, and a bad bot token is found out at the first reply.
    match client.open_session(&bot).auth_test().await {
        Ok(_) => {}
        Err(error) if refused(&error) => {
            tracing::warn!(%error, "Slack refused the bot token");
            let _ = sink.send(Event::Connection(State::Refused));
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "Slack could not be reached");
            let _ = sink.send(Event::Connection(State::Unreachable));
            return;
        }
    }

    let environment = Arc::new(
        SlackClientEventsListenerEnvironment::new(client.clone())
            .with_error_handler(said_wrong)
            .with_user_state(Listening { sink: sink.clone() }),
    );
    let callbacks = SlackSocketModeListenerCallbacks::new().with_push_events(pushed);
    let listener = SlackClientSocketModeListener::new(
        &SlackClientSocketModeConfig::new(),
        environment,
        callbacks,
    );
    if let Err(error) = listener
        .listen_for(&SlackApiToken::new(app_token.into()))
        .await
    {
        tracing::warn!(%error, "Slack refused the app token");
        let _ = sink.send(Event::Connection(match refused(&error) {
            true => State::Refused,
            false => State::Unreachable,
        }));
        return;
    }
    listener.start().await;
    // Connected once the socket is up, which `start` waits for. Slack's
    // own hello would say it a moment later, but the crate does not name
    // the type a listener for it would have to take.
    let _ = sink.send(Event::Connection(State::Connected));

    // Which direct message is whose, asked once a person: Slack names the
    // conversation with somebody by an id of its own, and every message
    // to them needs it.
    let mut directs: HashMap<String, SlackChannelId> = HashMap::new();
    let session = client.open_session(&bot);
    while let Some(out) = said.recv().await {
        match out {
            Out::Say {
                to,
                at,
                text,
                notify,
            } => {
                let Some(channel) = direct(&session, &mut directs, &to).await else {
                    continue;
                };
                let text = match notify {
                    true => format!("<@{to}> {text}"),
                    false => text,
                };
                let thread = match at {
                    Where::Top => None,
                    // Never said to: a fresh thread is only heard, and
                    // Slack has no room to start one in.
                    Where::Thread(thread) | Where::Fresh(thread) => Some(SlackTs::new(thread)),
                };
                if let Err(error) = session
                    .chat_post_message(&posted(channel, text, thread))
                    .await
                {
                    tracing::warn!(%error, "Slack would not take a message");
                }
            }
            Out::Open { asked, to, head } => {
                let text = head.in_words();
                let Some(channel) = direct(&session, &mut directs, &to).await else {
                    continue;
                };
                match session
                    .chat_post_message(&posted(channel.clone(), text, None))
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
                    Err(error) => tracing::warn!(%error, "Slack would not start a thread"),
                }
            }
            // Slack edits a message for as long as it is there, so the
            // head is the first message's text, said again.
            Out::Retitle { to, thread, head } => {
                let Some(channel) = direct(&session, &mut directs, &to).await else {
                    continue;
                };
                let mut content = SlackMessageContent::new();
                content.markdown_text = Some(head.in_words());
                if let Err(error) = session
                    .chat_update(&SlackApiChatUpdateRequest::new(
                        channel,
                        content,
                        SlackTs::new(thread),
                    ))
                    .await
                {
                    tracing::warn!(%error, "Slack would not say what a thread is again");
                }
            }
            // The threads stay in the direct message.
            Out::Room { .. } => {}
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
        }
    }
    listener.shutdown().await;
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

/// The direct message with somebody, opened the first time it is needed.
async fn direct(
    session: &SlackClientSession<'_, SlackClientHyperHttpsConnector>,
    directs: &mut HashMap<String, SlackChannelId>,
    to: &str,
) -> Option<SlackChannelId> {
    if let Some(channel) = directs.get(to) {
        return Some(channel.clone());
    }
    let mut asked = SlackApiConversationsOpenRequest::new();
    asked.users = Some(vec![SlackUserId::new(to.to_string())]);
    match session.conversations_open(&asked).await {
        Ok(opened) => {
            let channel = opened.channel.id;
            directs.insert(to.to_string(), channel.clone());
            Some(channel)
        }
        Err(error) => {
            tracing::warn!(%error, "Slack would not open a direct message");
            None
        }
    }
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
    // Somebody writing, in a direct message, and nothing else: see the
    // module's own note.
    if message.subtype.is_some()
        || message.sender.bot_id.is_some()
        || message.origin.channel_type != Some(SlackChannelType::new("im".to_string()))
    {
        return Ok(());
    }
    let (Some(from), Some(text)) = (
        message.sender.user,
        message.content.and_then(|content| content.text),
    ) else {
        return Ok(());
    };
    let at = match message.origin.thread_ts {
        Some(thread) => Where::Thread(thread.to_string()),
        None => Where::Top,
    };
    if let Some(listening) = states.read().await.get_user_state::<Listening>() {
        let _ = listening.sink.send(Event::Heard {
            from: from.to_string(),
            at,
            text,
        });
    }
    Ok(())
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
