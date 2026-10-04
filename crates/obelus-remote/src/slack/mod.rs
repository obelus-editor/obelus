//! Slack: what it has to be told, the app a reader makes on its side, and
//! the connection to it.

pub mod connection;

use crate::platform::{Description, Field, FieldKind, Setup};

/// Slack, as the settings page and the relay know it.
pub static DESCRIPTION: Description = Description {
    key: "slack",
    name: "Slack",
    fields: &[
        Field {
            key: "app_token",
            name: "App token",
            about: "Socket Mode's token, from Basic Information",
            kind: FieldKind::Secret {
                looks_like: "xapp-",
            },
        },
        Field {
            key: "bot_token",
            name: "Bot token",
            about: "From OAuth & Permissions, once the app is installed",
            kind: FieldKind::Secret {
                looks_like: "xoxb-",
            },
        },
    ],
    setup: Setup::Copy {
        name: "Manifest",
        // One app per machine, said where the app is made: Slack hands each
        // event of an app to one of its connections at random, so two
        // machines on one app each hear half of what is said and neither can
        // tell. The keyring keeps the tokens from following a reader's
        // settings to another machine; this keeps the reader from putting
        // them there.
        about: "One app per machine: paste it into Create New App → From a manifest",
        what: "the manifest",
        text: manifest,
    },
    begin: "Send a message here to talk to an agent; it answers in the thread under it.",
    cards: false,
    connect: |told, sink| {
        let said = |key| told.get(key).cloned().unwrap_or_default();
        connection::start(said("app_token"), said("bot_token"), sink)
    },
};

/// The app Obelus needs, as Slack's "from a manifest" takes it.
///
/// Socket Mode, because the machine Obelus runs on has no address the
/// internet can reach. No Home tab and no Messages tab: everything happens
/// in a private channel the reader makes and invites the app to, where every
/// message begins a conversation in the thread under it, and a page or a
/// direct message beside it would be a second place for it to happen. No
/// interactivity: everything Obelus says is words and everything it is told
/// is words, which is what every chat can carry. And the scopes for reading
/// and writing the channels it is invited to, private or not -- nothing
/// where it has not been invited.
#[must_use]
pub fn manifest() -> String {
    let manifest = serde_json::json!({
        "display_information": {
            "name": "Obelus",
            "description": "Work on Obelus's notes from Slack",
            "background_color": "#18181b",
        },
        "features": {
            "app_home": {
                "home_tab_enabled": false,
                "messages_tab_enabled": false,
            },
            "bot_user": {
                "display_name": "Obelus",
                "always_online": true,
            },
        },
        "oauth_config": {
            "scopes": {
                "bot": ["chat:write", "channels:history", "groups:history", "users:read"],
            },
        },
        "settings": {
            "event_subscriptions": {
                "bot_events": ["message.channels", "message.groups"],
            },
            "interactivity": { "is_enabled": false },
            "org_deploy_enabled": false,
            "socket_mode_enabled": true,
            "token_rotation_enabled": false,
        },
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}
