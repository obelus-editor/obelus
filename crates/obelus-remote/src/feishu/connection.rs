//! Feishu, connected: the long connection in, the open API out.
//!
//! **One task, two channels, and a connection made again when it drops** --
//! the shape Slack's has, for the same reasons: what is said goes in the
//! order it was asked for, and dropping the sender is what stops it.
//!
//! **The long connection is protobuf frames over a websocket.** The address
//! is asked of `/callback/ws/endpoint` with the app's id and secret; on it,
//! a frame is either control (a ping out, a pong back carrying how often to
//! ping) or data (an event, which has to be answered within three seconds
//! or Feishu sends it again). A big event can arrive in pieces, each saying
//! which of how many it is.
//!
//! **A conversation's topic is a reply to its first message.** A thread
//! here is the first message of a topic, everything after it is a reply to
//! that one with `reply_in_thread`, and what the reader writes in it arrives
//! naming that message as its root.
//!
//! **A question is a card, and the card is the answer.** Buttons where one
//! press answers it, a form where it takes several or the reader's own
//! words; what is pressed comes back over the long connection as a card
//! callback -- the app has to be given `card.action.trigger` by long
//! connection for that -- naming the question by Obelus's number for it,
//! and the card is drawn again closed once Obelus says what became of it.
//!
//! **The topics are in a group the reader made.** In a direct message a
//! topic hangs off a message in it, so every conversation put a card into
//! one stream with everything else said there, and it was a tangle. A group
//! in topic mode is nothing but topics -- a list of them, each opened to
//! read -- so the reader makes one, puts the bot in it and pairs there, and
//! that group is where everything happens. Nothing is heard anywhere else:
//! not a direct message, and not another group, which Obelus tells apart by
//! the group's id rather than here.

use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::{SinkExt as _, StreamExt as _};
use lark_websocket_protobuf::pbbp2::{Frame, Header};
use obelus_sink::Sink;
use prost::Message as _;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    Event, State,
    model::{Head, Out, Question, Turning, Where},
};

/// Where the open API is, by the domain the reader chose.
fn base(domain: &str) -> &'static str {
    match domain {
        "lark" => "https://open.larksuite.com",
        _ => "https://open.feishu.cn",
    }
}

/// Connects with this app's id and secret and returns where to send what is
/// to be said; see [`crate::slack::connection::start`] for the shape.
#[must_use]
pub fn start(
    app_id: String,
    app_secret: String,
    domain: &str,
    sink: Arc<dyn Sink<Event>>,
) -> tokio::sync::mpsc::UnboundedSender<Out> {
    let (out, said) = tokio::sync::mpsc::unbounded_channel();
    let api = Api {
        // A request that never answers would hold up everything after it --
        // what is said goes one thing at a time -- and the connection could
        // not even be let go, since that is noticed between two of them.
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default(),
        base: base(domain),
        app_id,
        app_secret,
        token: tokio::sync::Mutex::new(None),
    };
    obelus_runtime::handle().spawn(run(Arc::new(api), sink, said));
    out
}

/// The open API, with the token it is asked with.
struct Api {
    http: reqwest::Client,
    base: &'static str,
    app_id: String,
    app_secret: String,
    /// The tenant's token and when it stops working. Asked for again a
    /// little before then: a token Feishu has just let lapse is a message
    /// that does not go.
    token: tokio::sync::Mutex<Option<(String, std::time::Instant)>>,
}

/// Why the API would not do something.
#[derive(Debug)]
enum Refusal {
    /// It turned the app's id or secret down.
    Credentials(String),
    /// It took them, and would not give this app a long connection: the
    /// app is not set up for one on Feishu's side.
    Setup(String),
    /// It could not be reached, or said something else.
    Other(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Credentials(why) | Self::Setup(why) | Self::Other(why) => {
                formatter.write_str(why)
            }
        }
    }
}

impl Api {
    /// The tenant's token, asked for when there is none or it is nearly up.
    async fn token(&self) -> Result<String, Refusal> {
        let mut kept = self.token.lock().await;
        if let Some((token, until)) = kept.as_ref()
            && *until > std::time::Instant::now()
        {
            return Ok(token.clone());
        }
        let answer: Value = self
            .http
            .post(format!(
                "{}/open-apis/auth/v3/tenant_access_token/internal",
                self.base
            ))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )
            .body(json!({ "app_id": self.app_id, "app_secret": self.app_secret }).to_string())
            .send()
            .await
            .map_err(|error| Refusal::Other(error.to_string()))
            .map(read)?
            .await?;
        if answer["code"].as_i64() != Some(0) {
            return Err(Refusal::Credentials(
                answer["msg"].as_str().unwrap_or("refused").to_string(),
            ));
        }
        let token = answer["tenant_access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // Five minutes short of what it says, so a message is never sent
        // with a token in its last seconds.
        let lasts = answer["expire"].as_u64().unwrap_or(0).saturating_sub(300);
        *kept = Some((
            token.clone(),
            std::time::Instant::now() + Duration::from_secs(lasts),
        ));
        Ok(token)
    }

    /// Calls the open API with the tenant's token, and hands back `data`.
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Refusal> {
        let token = self.token().await?;
        let mut asked = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token);
        if let Some(body) = body {
            asked = asked
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/json; charset=utf-8",
                )
                .body(body.to_string());
        }
        let answer: Value = asked
            .send()
            .await
            .map_err(|error| Refusal::Other(error.to_string()))
            .map(read)?
            .await?;
        match answer["code"].as_i64() {
            Some(0) => Ok(answer["data"].clone()),
            // With Feishu's own id for the request, which is what its
            // troubleshooting asks for.
            _ => Err(Refusal::Other(format!(
                "{} ({}, log {})",
                answer["msg"].as_str().unwrap_or("refused"),
                answer["code"],
                answer["error"]["log_id"].as_str().unwrap_or("none")
            ))),
        }
    }

    /// Where the long connection is, and how often to ping on it.
    async fn endpoint(&self) -> Result<(String, Duration), Refusal> {
        let answer: Value = self
            .http
            .post(format!("{}/callback/ws/endpoint", self.base))
            .header("locale", "zh")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )
            .body(json!({ "AppID": self.app_id, "AppSecret": self.app_secret }).to_string())
            .send()
            .await
            .map_err(|error| Refusal::Other(error.to_string()))
            .map(read)?
            .await?;
        // Feishu's own client gives up on every code but two, and tries
        // those again: one is busy and the other is its own fault. Every
        // other is about the app -- its long connection not switched on, or
        // its events not subscribed to -- and asking again will not mend it.
        match answer["code"].as_i64() {
            Some(0) => {}
            code => {
                let why = answer["msg"].as_str().unwrap_or("refused").to_string();
                return Err(match code {
                    Some(1 | 1_000_040_343) | None => Refusal::Other(why),
                    Some(_) => Refusal::Setup(why),
                });
            }
        }
        let url = answer["data"]["URL"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if url.is_empty() {
            return Err(Refusal::Other("no address to connect to".to_string()));
        }
        let ping = answer["data"]["ClientConfig"]["PingInterval"]
            .as_u64()
            .unwrap_or(120);
        Ok((url, Duration::from_secs(ping.max(10))))
    }
}

/// What came back, read as JSON.
async fn read(response: reqwest::Response) -> Result<Value, Refusal> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| Refusal::Other(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| Refusal::Other(error.to_string()))
}

/// Text in markdown, as a rich-text message's content: one paragraph that
/// is markdown, which Feishu draws itself -- with the reader called at the
/// front of it where they are to be.
fn rich(text: &str, call: Option<&str>) -> String {
    let text = match call {
        Some(open_id) => format!("<at user_id=\"{open_id}\"></at> {text}"),
        None => text.to_string(),
    };
    json!({
        "zh_cn": {
            "content": [[{ "tag": "md", "text": text }]]
        }
    })
    .to_string()
}

/// A thread's head as a card: its title with the state's mark, in the
/// state's colour, and where it is under it.
///
/// A card rather than a rich message because a card can be said again as
/// often as the turn moves -- a message may be edited twenty times, and a
/// conversation has more turns than that -- for the fourteen days Feishu
/// lets a card be updated.
fn card(head: &Head) -> String {
    let colour = match head.state {
        Some(Turning::Working) => "blue",
        Some(Turning::Waiting) => "orange",
        Some(Turning::Done) => "green",
        Some(Turning::Closed) | None => "grey",
    };
    json!({
        "schema": "2.0",
        "config": { "update_multi": true },
        "header": {
            "title": { "tag": "plain_text", "content": head.titled() },
            "template": colour,
        },
        "body": {
            "elements": [{ "tag": "markdown", "content": head.place }]
        }
    })
    .to_string()
}

/// How much of what a question is about goes on its card, and of each
/// answer's name: Feishu does not say where it stops, so these are Slack's
/// own limits -- the stricter of the two -- rather than a guess at its.
const ABOUT: usize = 2900;
const NAME: usize = 75;

/// A question as a card: what it is about, calling the reader, and then a
/// button for each answer where one press is the whole answer, or a form
/// where it takes several, or words of the reader's own. Every press names
/// the question by `asked`, and the process that asked it by `by`.
fn asking(asked: u64, question: &Question, to: &str) -> String {
    let asked = asked.to_string();
    let by = crate::this_process();
    let mut elements = vec![json!({
        "tag": "markdown",
        "content": format!("<at id={to}></at> {}", crate::capped(&question.about, ABOUT)),
    })];
    let named = |(id, name): &(String, String)| json!({ "text": { "tag": "plain_text", "content": crate::capped(name, NAME) }, "value": id });
    if !question.several && question.words.is_none() && !question.choices.is_empty() {
        for (id, name) in &question.choices {
            elements.push(json!({
                "tag": "button",
                "text": { "tag": "plain_text", "content": crate::capped(name, NAME) },
                "width": "fill",
                "behaviors": [{ "type": "callback", "value": { "asked": asked, "by": by, "chosen": id } }],
            }));
        }
    } else {
        let mut form = Vec::new();
        if !question.choices.is_empty() {
            form.push(json!({
                "tag": match question.several {
                    true => "multi_select_static",
                    false => "select_static",
                },
                "name": "chosen",
                "placeholder": { "tag": "plain_text", "content": "Choose" },
                "required": question.needed,
                "options": question.choices.iter().map(named).collect::<Vec<Value>>(),
            }));
        }
        if let Some((name, required)) = &question.words {
            form.push(json!({
                "tag": "input",
                "name": "words",
                "placeholder": { "tag": "plain_text", "content": crate::capped(name, NAME) },
                "required": required,
            }));
        }
        form.push(json!({
            "tag": "button",
            "name": "send",
            "text": { "tag": "plain_text", "content": "Send" },
            "type": "primary",
            "form_action_type": "submit",
            "behaviors": [{ "type": "callback", "value": { "asked": asked, "by": by } }],
        }));
        elements.push(json!({ "tag": "form", "name": "answer", "elements": form }));
    }
    json!({
        "schema": "2.0",
        "config": { "update_multi": true },
        "body": { "elements": elements },
    })
    .to_string()
}

/// The same card closed: what it was about, and what became of it in
/// place of anything to press.
fn settled(about: &str, said: &str) -> String {
    // Nothing for a question with nothing said about it -- a permission
    // whose call said it already -- rather than an empty element.
    let mut elements = Vec::new();
    if !about.is_empty() {
        elements.push(json!({ "tag": "markdown", "content": crate::capped(about, ABOUT) }));
    }
    elements.push(json!({ "tag": "markdown", "content": said, "text_size": "notation" }));
    json!({
        "schema": "2.0",
        "config": { "update_multi": true },
        "body": { "elements": elements },
    })
    .to_string()
}

async fn run(
    api: Arc<Api>,
    sink: Arc<dyn Sink<Event>>,
    mut said: tokio::sync::mpsc::UnboundedReceiver<Out>,
) {
    let _ = sink.send(Event::connection(State::Connecting, None));
    // The id and secret first, by asking for a token: the long connection
    // would refuse them too, but in words that do not say which. Asked
    // again until they are taken, a refusal less often: a machine started
    // before its network is a machine that would otherwise never connect,
    // and what Feishu calls a refusal includes being busy. What is to be
    // said meanwhile waits, in order -- as much of it as is kept -- and a
    // window letting go ends it.
    let mut waiting = crate::waiting::Waiting::default();
    loop {
        let (state, why, again) = match api.token().await {
            Ok(_) => break,
            Err(Refusal::Credentials(why)) => {
                tracing::warn!(%why, "Feishu refused the app's id or secret");
                (State::Refused, why, 60)
            }
            Err(Refusal::Setup(why) | Refusal::Other(why)) => {
                tracing::warn!(%why, "Feishu could not be reached");
                (State::Unreachable, why, 10)
            }
        };
        let _ = sink.send(Event::connection(state, Some(why)));
        let pause = tokio::time::sleep(Duration::from_secs(again));
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
    }
    // The long connection, on a task of its own, and made again whenever it
    // drops: what it hears goes straight to the sink.
    let listening = obelus_runtime::handle().spawn(listen(api.clone(), sink.clone()));
    // Each question's card, by Obelus's number for it: the message, and
    // what it was about, to draw it closed with.
    let mut cards: HashMap<u64, (String, String)> = HashMap::new();
    for out in waiting.drain() {
        if let Err(why) = say(&api, &sink, &mut cards, out).await {
            tracing::warn!(%why, "Feishu would not take a message");
        }
    }
    while let Some(out) = said.recv().await {
        if let Err(why) = say(&api, &sink, &mut cards, out).await {
            tracing::warn!(%why, "Feishu would not take a message");
        }
    }
    listening.abort();
}

/// Does one thing Obelus asked for.
async fn say(
    api: &Api,
    sink: &Arc<dyn Sink<Event>>,
    cards: &mut HashMap<u64, (String, String)>,
    out: Out,
) -> Result<(), Refusal> {
    match out {
        Out::Ask {
            room,
            thread,
            to,
            asked,
            question,
        } => {
            let posted = api
                .call(
                    reqwest::Method::POST,
                    &format!("/open-apis/im/v1/messages/{thread}/reply"),
                    Some(json!({
                        "msg_type": "interactive",
                        "content": asking(asked, &question, &to),
                        "reply_in_thread": true,
                    })),
                )
                .await;
            match posted {
                Ok(posted) => {
                    if let Some(message) = posted["message_id"].as_str() {
                        tracing::info!(thread, asked, message, "a question put");
                        cards.insert(asked, (message.to_string(), question.about));
                    }
                }
                // Said in words rather than not at all: the reader away from
                // the machine is otherwise left with an agent waiting on a
                // question they never see.
                Err(why) => {
                    tracing::warn!(thread, asked, %why, "a question not put");
                    let _ = sink.send(Event::Unasked { asked });
                    Box::pin(say(
                        api,
                        sink,
                        cards,
                        Out::Say {
                            room,
                            thread,
                            to,
                            text: question.in_words(),
                            notify: true,
                        },
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
        } => match cards.remove(&asked) {
            Some((message, about)) => {
                api.call(
                    reqwest::Method::PATCH,
                    &format!("/open-apis/im/v1/messages/{message}"),
                    Some(json!({ "content": settled(&about, &said) })),
                )
                .await
                .inspect_err(|why| tracing::warn!(message, %why, "a question's card not closed"))?;
            }
            // A card that never went up -- or went up on a connection
            // since let go, which kept the message -- is said in words.
            None => {
                Box::pin(say(
                    api,
                    sink,
                    cards,
                    Out::Settle {
                        room,
                        thread,
                        to,
                        asked,
                        said,
                    }
                    .in_words(),
                ))
                .await?;
            }
        },
        Out::Say {
            thread,
            to,
            text,
            notify,
            ..
        } => {
            // Which thread and what was said, either way: a refusal on its
            // own was a line that could not say which conversation lost
            // its words, or whether it was the question or the turn's end.
            let opening: String = text
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(60)
                .collect();
            let replied = api
                .call(
                    reqwest::Method::POST,
                    &format!("/open-apis/im/v1/messages/{thread}/reply"),
                    Some(json!({
                        "msg_type": "post",
                        "content": rich(&text, notify.then_some(to.as_str())),
                        "reply_in_thread": true,
                    })),
                )
                .await;
            match &replied {
                Ok(data) => tracing::info!(
                    thread,
                    notify,
                    opening,
                    message = data["message_id"].as_str(),
                    root = data["root_id"].as_str(),
                    "said in a thread"
                ),
                Err(why) => {
                    tracing::warn!(thread, notify, opening, %why, "not said in a thread");
                }
            }
            replied?;
        }
        Out::Open { asked, room, head } => {
            let opened = api
                .call(
                    reqwest::Method::POST,
                    "/open-apis/im/v1/messages?receive_id_type=chat_id",
                    Some(json!({
                        "receive_id": room,
                        "msg_type": "interactive",
                        "content": card(&head),
                    })),
                )
                .await
                .map(|data| data["message_id"].as_str().unwrap_or_default().to_string());
            match opened {
                Ok(thread) if !thread.is_empty() => {
                    tracing::info!(asked, thread, "a thread opened");
                    let _ = sink.send(Event::Opened {
                        asked,
                        thread,
                        link: None,
                    });
                }
                failed => {
                    let _ = sink.send(Event::Unopened {
                        asked,
                        waited: false,
                    });
                    failed?;
                }
            }
        }
        // A card's content replaced whole: the head is the card.
        Out::Retitle { thread, head, .. } => {
            api.call(
                reqwest::Method::PATCH,
                &format!("/open-apis/im/v1/messages/{thread}"),
                Some(json!({ "content": card(&head) })),
            )
            .await
            .inspect_err(|why| tracing::warn!(thread, %why, "a thread's head not said again"))?;
        }
        Out::Name { id } => {
            // The name where the app may read it, and the id where it may
            // not: a person on the list by an id is still on the list.
            let name = api
                .call(
                    reqwest::Method::GET,
                    &format!("/open-apis/contact/v3/users/{id}?user_id_type=open_id"),
                    None,
                )
                .await
                .ok()
                .and_then(|data| data["user"]["name"].as_str().map(str::to_string))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.clone());
            let _ = sink.send(Event::Named { id, name });
        }
    }
    Ok(())
}

/// The long connection, made and made again for as long as it is wanted.
async fn listen(api: Arc<Api>, sink: Arc<dyn Sink<Event>>) {
    // Which events have been heard lately, kept across connections: Feishu
    // sends an event again when it thinks the answer did not arrive, and a
    // connection that dropped is the likeliest time for that -- a reply
    // heard twice is a question answered twice.
    let mut seen: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    loop {
        // Connecting again only where it was up a moment ago: a socket that
        // never opened is not on its way back, and saying it was turned
        // the mark for as long as Feishu went on refusing it.
        let (state, why, again) = match api.endpoint().await {
            Ok((url, ping)) => match connected(&url, ping, &mut seen, &sink).await {
                Lost::Dropped(why) => {
                    tracing::warn!(%why, "the long connection to Feishu dropped");
                    (State::Connecting, None, 5)
                }
                Lost::Unmade(why) => {
                    tracing::warn!(%why, "the long connection to Feishu would not open");
                    (State::Unreachable, Some(why), 5)
                }
            },
            Err(Refusal::Setup(why)) => {
                tracing::warn!(%why, "Feishu would not give this app a long connection");
                (State::Declined, Some(why), 60)
            }
            Err(why) => {
                tracing::warn!(%why, "Feishu gave no long connection");
                (State::Unreachable, Some(why.to_string()), 5)
            }
        };
        let _ = sink.send(Event::connection(state, why));
        tokio::time::sleep(Duration::from_secs(again)).await;
    }
}

/// How a long connection ended.
enum Lost {
    /// It never opened.
    Unmade(String),
    /// It was open, and went.
    Dropped(String),
}

/// One long connection, until it drops.
async fn connected(
    url: &str,
    ping: Duration,
    seen: &mut std::collections::VecDeque<String>,
    sink: &Arc<dyn Sink<Event>>,
) -> Lost {
    let service: i32 = url::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "service_id")
                .and_then(|(_, value)| value.parse().ok())
        })
        .unwrap_or(0);
    let socket = match tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async(url),
    )
    .await
    {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(error)) => return Lost::Unmade(error.to_string()),
        Err(_) => return Lost::Unmade("no answer".to_string()),
    };
    let (mut out, mut heard) = socket.split();
    let _ = sink.send(Event::connection(State::Connected, None));
    match listened(&mut out, &mut heard, service, ping, seen, sink).await {
        Ok(()) => Lost::Dropped("closed".to_string()),
        Err(why) => Lost::Dropped(why),
    }
}

/// Hears what comes in on a long connection that is open, until it drops.
async fn listened(
    out: &mut (impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    heard: &mut (
             impl futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin
         ),
    service: i32,
    ping: Duration,
    seen: &mut std::collections::VecDeque<String>,
    sink: &Arc<dyn Sink<Event>>,
) -> Result<(), String> {
    let mut pings = tokio::time::interval(ping);
    // When anything last came back. A ping goes into a socket that is
    // already dead without complaint -- after the machine sleeps, or the
    // network under it changes -- and nothing says so until TCP gives up,
    // a quarter of an hour later, with the row saying `Connected` all the
    // while. Feishu answers every ping, so three without a word back is a
    // connection that has gone.
    let mut heard_last = std::time::Instant::now();
    // Pieces of events that arrived in several, by the event's id.
    let mut pieces: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();
    loop {
        tokio::select! {
            _ = pings.tick() => {
                if heard_last.elapsed() > ping * 3 {
                    return Err("nothing heard for three pings".to_string());
                }
                let frame = Frame {
                    seq_id: 0,
                    log_id: 0,
                    service,
                    method: 0,
                    headers: vec![header("type", "ping")],
                    payload_encoding: None,
                    payload_type: None,
                    payload: None,
                    log_id_new: None,
                };
                out.send(Message::Binary(frame.encode_to_vec().into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            message = heard.next() => {
                let Some(message) = message else {
                    return Err("closed".to_string());
                };
                heard_last = std::time::Instant::now();
                let bytes = match message.map_err(|error| error.to_string())? {
                    Message::Binary(bytes) => bytes,
                    Message::Ping(bytes) => {
                        out.send(Message::Pong(bytes)).await.map_err(|error| error.to_string())?;
                        continue;
                    }
                    Message::Close(_) => return Err("closed by Feishu".to_string()),
                    _ => continue,
                };
                let Ok(mut frame) = Frame::decode(bytes.as_ref()) else {
                    continue;
                };
                // A pong, which is all a control frame from Feishu is.
                if frame.method != 1 {
                    continue;
                }
                let Some(payload) = whole(&mut pieces, &frame) else {
                    continue;
                };
                let answer = heard_frame(&payload, seen, sink);
                // Answered whatever it was, at once: an event not answered
                // within three seconds is sent again, and what was done
                // with it is the application's, which has it already.
                frame.payload = Some(answer.to_string().into_bytes());
                frame.headers.push(header("biz_rt", "0"));
                out.send(Message::Binary(frame.encode_to_vec().into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
}

/// What a data frame said, passed on, and what to answer it with.
///
/// Told apart by the event's own type and not by the frame's: a press on a
/// card arrives over the long connection as an `event` frame like a message
/// does, and reading the frame's type sent every press to the messages,
/// which dropped it. A press is answered with what the reader sees at once;
/// whether it is taken is the application's to say, by closing the card.
fn heard_frame(
    payload: &[u8],
    seen: &mut std::collections::VecDeque<String>,
    sink: &Arc<dyn Sink<Event>>,
) -> Value {
    let pressed = serde_json::from_slice::<Value>(payload)
        .is_ok_and(|event| event["header"]["event_type"] == "card.action.trigger");
    if !pressed {
        heard_event(payload, seen, sink);
        return json!({ "code": 200 });
    }
    heard_press(payload, seen, sink);
    let toast = json!({ "toast": { "type": "info", "content": "Sent" } });
    json!({
        "code": 200,
        "data": base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            toast.to_string(),
        ),
    })
}

fn header(key: &str, value: &str) -> Header {
    Header {
        key: key.to_string(),
        value: value.to_string(),
    }
}

fn value_of<'a>(headers: &'a [Header], key: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.key == key)
        .map(|header| header.value.as_str())
}

/// A frame's payload, whole: at once where it came in one piece, and once
/// the last piece is in where it came in several.
fn whole(pieces: &mut HashMap<String, Vec<Option<Vec<u8>>>>, frame: &Frame) -> Option<Vec<u8>> {
    let payload = frame.payload.clone()?;
    let sum: usize = value_of(&frame.headers, "sum")
        .and_then(|sum| sum.parse().ok())
        .unwrap_or(1);
    if sum <= 1 {
        return Some(payload);
    }
    let seq: usize = value_of(&frame.headers, "seq")
        .and_then(|seq| seq.parse().ok())
        .unwrap_or(0);
    let id = value_of(&frame.headers, "message_id")
        .unwrap_or_default()
        .to_string();
    let kept = pieces.entry(id.clone()).or_insert_with(|| vec![None; sum]);
    if let Some(slot) = kept.get_mut(seq) {
        *slot = Some(payload);
    }
    if kept.iter().any(Option::is_none) {
        return None;
    }
    pieces
        .remove(&id)
        .map(|kept| kept.into_iter().flatten().flatten().collect())
}

/// What somebody wrote to the app, passed on -- and nothing else is.
///
/// Only a person's words in a group: an event of another kind, a direct
/// message, a picture, are none of them somebody talking to Obelus. Which
/// group is Obelus's to judge, by the id that goes with them.
fn heard_event(
    payload: &[u8],
    seen: &mut std::collections::VecDeque<String>,
    sink: &Arc<dyn Sink<Event>>,
) {
    let Ok(event) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    if let Some(id) = event["header"]["event_id"].as_str() {
        if seen.iter().any(|kept| kept == id) {
            return;
        }
        seen.push_back(id.to_string());
        if seen.len() > 256 {
            seen.pop_front();
        }
    }
    if event["header"]["event_type"].as_str() != Some("im.message.receive_v1") {
        return;
    }
    let message = &event["event"]["message"];
    if message["chat_type"].as_str() != Some("group")
        || event["event"]["sender"]["sender_type"].as_str() != Some("user")
    {
        return;
    }
    let (Some(from), Some(room)) = (
        event["event"]["sender"]["sender_id"]["open_id"].as_str(),
        message["chat_id"].as_str(),
    ) else {
        return;
    };
    let Some(text) = message["content"]
        .as_str()
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .and_then(|content| words_of(message["message_type"].as_str()?, &content))
    else {
        return;
    };
    // A message with nothing above it is a topic just started, and the
    // topic is that message.
    let at = match message["root_id"].as_str().filter(|root| !root.is_empty()) {
        Some(root) => Where::Thread(root.to_string()),
        None => match message["message_id"].as_str() {
            Some(id) => Where::Fresh(id.to_string()),
            None => return,
        },
    };
    tracing::info!(
        ?at,
        message = message["message_id"].as_str(),
        "heard in the room"
    );
    let _ = sink.send(Event::Heard {
        from: from.to_string(),
        room: room.to_string(),
        at,
        text,
    });
}

/// A press on a question's card, passed on as the answer it makes: which
/// question, by the number it was asked with, the ids chosen -- one from a
/// button, one or several from the form's list -- and the words in its box.
fn heard_press(
    payload: &[u8],
    seen: &mut std::collections::VecDeque<String>,
    sink: &Arc<dyn Sink<Event>>,
) {
    let Ok(event) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    if let Some(id) = event["header"]["event_id"].as_str() {
        if seen.iter().any(|kept| kept == id) {
            return;
        }
        seen.push_back(id.to_string());
        if seen.len() > 256 {
            seen.pop_front();
        }
    }
    let event = &event["event"];
    let action = &event["action"];
    let (Some(from), Some(asked)) = (
        event["operator"]["open_id"].as_str(),
        action["value"]["asked"]
            .as_str()
            .and_then(|asked| asked.parse().ok()),
    ) else {
        return;
    };
    if action["value"]["by"].as_str() != Some(crate::this_process()) {
        tracing::info!(asked, "a press on a card another window put up");
        return;
    }
    let form = &action["form_value"];
    let chosen: Vec<String> = match (action["value"]["chosen"].as_str(), &form["chosen"]) {
        (Some(id), _) => vec![id.to_string()],
        (None, Value::String(id)) => vec![id.clone()],
        (None, Value::Array(ids)) => ids
            .iter()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect(),
        (None, _) => Vec::new(),
    };
    let words = form["words"]
        .as_str()
        .map(str::trim)
        .filter(|words| !words.is_empty())
        .map(str::to_string);
    tracing::info!(asked, ?chosen, "a question answered on its card");
    let _ = sink.send(Event::Answered {
        from: from.to_string(),
        asked,
        chosen,
        words,
    });
}

/// The words in a message: a text's text, or a rich message's title and
/// paragraphs -- which is what a topic group's box sends when the reader
/// gives a topic a title. Anything else has no words to hear.
fn words_of(kind: &str, content: &Value) -> Option<String> {
    match kind {
        "text" => content["text"].as_str().map(str::to_string),
        "post" => {
            // As sent it is under a language; as heard it is not.
            let post = match content.get("content") {
                Some(_) => content,
                None => content.as_object()?.values().next()?,
            };
            let mut lines: Vec<String> = Vec::new();
            if let Some(title) = post["title"].as_str().filter(|title| !title.is_empty()) {
                lines.push(title.to_string());
            }
            for paragraph in post["content"].as_array()? {
                let line: String = paragraph
                    .as_array()?
                    .iter()
                    .filter_map(|piece| match piece["tag"].as_str() {
                        Some("text" | "a" | "code_block") => piece["text"].as_str(),
                        _ => None,
                    })
                    .collect();
                lines.push(line);
            }
            Some(lines.join("\n").trim().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a frame carries arrives whole whether it came in one piece or
    /// several, in whatever order the pieces came.
    ///
    /// Broken deliberately by handing back the first piece: the event came
    /// back a third of itself.
    #[test]
    fn an_event_in_pieces_arrives_whole() {
        let piece = |seq: usize, bytes: &[u8]| Frame {
            seq_id: 0,
            log_id: 0,
            service: 1,
            method: 1,
            headers: vec![
                header("sum", "3"),
                header("seq", &seq.to_string()),
                header("message_id", "m1"),
            ],
            payload_encoding: None,
            payload_type: None,
            payload: Some(bytes.to_vec()),
            log_id_new: None,
        };
        let mut pieces = HashMap::new();
        assert_eq!(whole(&mut pieces, &piece(2, b"three")), None);
        assert_eq!(whole(&mut pieces, &piece(0, b"one")), None);
        assert_eq!(
            whole(&mut pieces, &piece(1, b"two")),
            Some(b"onetwothree".to_vec())
        );
        assert!(pieces.is_empty(), "the pieces were kept after the whole");
    }

    /// Somebody's words in a group are heard, with the group they were in
    /// -- in the topic its root names, or as a topic just started where it
    /// has none; a direct message is not heard at all.
    ///
    /// Broken deliberately three ways. Taking the thread's own id instead
    /// of the root: the reply in a topic arrived naming a thread nothing
    /// keeps. Hearing direct messages: the one sent to the bot alone was
    /// heard as a topic started. And reading only a text: the topic started
    /// with a title, which arrives as a rich message, was not heard at all.
    #[test]
    fn words_are_heard_where_they_were_written() {
        let (sender, heard) = std::sync::mpsc::channel::<Event>();
        let sink: Arc<dyn Sink<Event>> = Arc::new(sender);
        let event = |root: Option<&str>, chat: &str, kind: &str, content: &str| {
            json!({
                "schema": "2.0",
                "header": { "event_type": "im.message.receive_v1" },
                "event": {
                    "sender": { "sender_id": { "open_id": "ou_1" }, "sender_type": "user" },
                    "message": {
                        "message_id": "om_2",
                        "root_id": root,
                        "thread_id": "omt_3",
                        "chat_id": "oc_room",
                        "chat_type": chat,
                        "message_type": kind,
                        "content": content
                    }
                }
            })
            .to_string()
        };
        let text = "{\"text\":\"1\"}";
        let titled = json!({
            "title": "Fix the build",
            "content": [[{ "tag": "text", "text": "it fails on " }, { "tag": "text", "text": "arm" }]]
        })
        .to_string();
        let seen = &mut std::collections::VecDeque::new();
        heard_event(
            event(Some("om_root"), "group", "text", text).as_bytes(),
            seen,
            &sink,
        );
        heard_event(
            event(None, "group", "post", &titled).as_bytes(),
            seen,
            &sink,
        );
        heard_event(event(None, "p2p", "text", text).as_bytes(), seen, &sink);
        let heard: Vec<(String, Where, String)> = heard
            .try_iter()
            .filter_map(|event| match event {
                Event::Heard {
                    from,
                    room,
                    at,
                    text,
                } if from == "ou_1" => Some((room, at, text)),
                _ => None,
            })
            .collect();
        let room = || "oc_room".to_string();
        assert_eq!(
            heard,
            [
                (
                    room(),
                    Where::Thread("om_root".to_string()),
                    "1".to_string()
                ),
                (
                    room(),
                    Where::Fresh("om_2".to_string()),
                    "Fix the build\nit fails on arm".to_string()
                ),
            ],
            "something was heard that should not have been, or the other way about"
        );
    }

    /// An event Feishu sends again -- it does, when it thinks the answer
    /// did not arrive -- is heard once.
    ///
    /// Broken deliberately by not keeping the ids: the reply was heard
    /// twice, which is a question answered twice.
    #[test]
    fn an_event_sent_again_is_heard_once() {
        let (sender, heard) = std::sync::mpsc::channel::<Event>();
        let sink: Arc<dyn Sink<Event>> = Arc::new(sender);
        let event = json!({
            "schema": "2.0",
            "header": { "event_type": "im.message.receive_v1", "event_id": "e-1" },
            "event": {
                "sender": { "sender_id": { "open_id": "ou_1" }, "sender_type": "user" },
                "message": {
                    "message_id": "om_2",
                    "root_id": "om_root",
                    "chat_id": "oc_room",
                    "chat_type": "group",
                    "message_type": "text",
                    "content": "{\"text\":\"1\"}"
                }
            }
        })
        .to_string();
        let seen = &mut std::collections::VecDeque::new();
        heard_event(event.as_bytes(), seen, &sink);
        heard_event(event.as_bytes(), seen, &sink);
        assert_eq!(heard.try_iter().count(), 1, "the event was heard twice");
    }

    /// A rich message carries its markdown whole, with the reader called in
    /// front of it where they are to be.
    ///
    /// Broken deliberately by leaving the call out: the text came back
    /// without the reader in it, and their phone would not have rung.
    #[test]
    fn a_rich_message_is_markdown_and_calls_the_reader() {
        let said: Value = serde_json::from_str(&rich("**done**", Some("ou_1"))).expect("json");
        assert_eq!(
            said["zh_cn"]["content"][0][0]["text"],
            "<at user_id=\"ou_1\"></at> **done**"
        );
        assert_eq!(said["zh_cn"]["content"][0][0]["tag"], "md");
    }

    /// A question one press answers is a button per answer; one that takes
    /// several, or the reader's own words, is a form sent with one button
    /// -- and every press names the question and carries the agent's ids.
    ///
    /// Broken deliberately twice. Drawing a form whatever the question: the
    /// permission's answers were a list to choose from and a button to send.
    /// And putting the names where the ids go: the press carried "Allow
    /// once", which is no answer the agent offered.
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
        let drawn = |question: &Question| {
            serde_json::from_str::<Value>(&asking(7, question, "ou_1")).expect("a card")
        };

        let card = drawn(&question(false, None));
        let elements = card["body"]["elements"].as_array().expect("elements");
        assert_eq!(
            elements[0]["content"], "<at id=ou_1></at> Read the file?",
            "the reader is not called"
        );
        assert_eq!(elements[1]["tag"], "button", "{card:#}");
        assert_eq!(
            elements[1]["behaviors"][0]["value"],
            json!({ "asked": "7", "by": crate::this_process(), "chosen": "once" })
        );
        assert_eq!(elements.len(), 3, "{card:#}");

        let card = drawn(&question(true, Some(("Other".to_string(), false))));
        let form = &card["body"]["elements"][1];
        assert_eq!(form["tag"], "form", "{card:#}");
        assert_eq!(form["elements"][0]["tag"], "multi_select_static");
        assert_eq!(form["elements"][0]["options"][1]["value"], "never");
        assert_eq!(form["elements"][1]["tag"], "input");
        assert_eq!(form["elements"][2]["form_action_type"], "submit");
        assert_eq!(
            form["elements"][2]["behaviors"][0]["value"],
            json!({ "asked": "7", "by": crate::this_process() })
        );

        // Closed, a question with nothing said about it has no empty
        // element above what became of it.
        let closed: Value =
            serde_json::from_str(&settled("", "\u{2714} Allow once")).expect("a card");
        assert_eq!(
            closed["body"]["elements"].as_array().map(Vec::len),
            Some(1),
            "{closed:#}"
        );

        // A name past what a button takes is cut to fit.
        let mut long = question(false, None);
        long.choices[0].1 = "a".repeat(200);
        let card = drawn(&long);
        let shown = card["body"]["elements"][1]["text"]["content"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(shown.chars().count(), 75, "{card:#}");
    }

    /// A press is heard as the answer it makes -- arriving, as Feishu sends
    /// it over the long connection, as an event like any other: a button's
    /// one id, a form's list -- one or several -- and the words in its box,
    /// trimmed and only where there are any. A press Feishu sends twice is
    /// heard once, and one on a card another window put up not at all.
    ///
    /// Broken deliberately five ways. Telling a press by the frame rather
    /// than by the event: every press went to the messages and was dropped.
    /// Reading only the button's value: the form's choices arrived as nothing
    /// chosen. Reading a form's list only as several: the single one chosen
    /// from a list was dropped. Not keeping the ids of what was heard: the
    /// second sending answered twice. And not reading whose card it was:
    /// the press on the closed window's card answered this one's question.
    #[test]
    fn a_press_is_heard_as_its_answer() {
        let (sender, heard) = std::sync::mpsc::channel::<Event>();
        let sink: Arc<dyn Sink<Event>> = Arc::new(sender);
        let mut seen = std::collections::VecDeque::new();
        let by = crate::this_process();
        let press = |id: &str, action: Value| {
            json!({
                "schema": "2.0",
                "header": { "event_id": id, "event_type": "card.action.trigger" },
                "event": { "operator": { "open_id": "ou_1" }, "action": action },
            })
            .to_string()
        };
        let answers = |heard: &std::sync::mpsc::Receiver<Event>| {
            heard
                .try_iter()
                .filter_map(|event| match event {
                    Event::Answered {
                        from,
                        asked,
                        chosen,
                        words,
                    } => Some((from, asked, chosen, words)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        let button = press(
            "e1",
            json!({ "value": { "asked": "7", "by": by, "chosen": "once" }, "tag": "button" }),
        );
        let answered = heard_frame(button.as_bytes(), &mut seen, &sink);
        assert!(
            answered["data"].is_string(),
            "the press had no toast: {answered}"
        );
        heard_frame(button.as_bytes(), &mut seen, &sink);
        heard_frame(
            press(
                "e4",
                json!({ "value": { "asked": "7", "by": "another", "chosen": "never" } }),
            )
            .as_bytes(),
            &mut seen,
            &sink,
        );
        assert_eq!(
            answers(&heard),
            [("ou_1".to_string(), 7, vec!["once".to_string()], None)]
        );

        heard_frame(
            press(
                "e2",
                json!({
                    "value": { "asked": "8", "by": by },
                    "form_value": { "chosen": ["app", "ui"], "words": "  the docs too " },
                }),
            )
            .as_bytes(),
            &mut seen,
            &sink,
        );
        heard_frame(
            press(
                "e3",
                json!({ "value": { "asked": "9", "by": by }, "form_value": { "chosen": "app", "words": "" } }),
            )
            .as_bytes(),
            &mut seen,
            &sink,
        );
        assert_eq!(
            answers(&heard),
            [
                (
                    "ou_1".to_string(),
                    8,
                    vec!["app".to_string(), "ui".to_string()],
                    Some("the docs too".to_string())
                ),
                ("ou_1".to_string(), 9, vec!["app".to_string()], None),
            ]
        );
    }
}
