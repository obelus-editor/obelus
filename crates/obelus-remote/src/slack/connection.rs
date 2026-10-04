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

use std::sync::Arc;

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
    let mut waiting = waiting.drain().collect::<Vec<Out>>().into_iter();
    while let Some(out) = match waiting.next() {
        Some(out) => Some(out),
        None => said.recv().await,
    } {
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
    let callbacks = SlackSocketModeListenerCallbacks::new().with_push_events(pushed);
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
}
