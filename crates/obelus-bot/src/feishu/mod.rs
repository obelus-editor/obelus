//! Feishu, and Lark, which is the same platform on another domain: what it
//! has to be told, and the connection to it.
//!
//! Written here rather than through a community SDK, which was tried: the
//! one there is cannot reply in a topic -- the whole of how a conversation
//! is told apart in a group -- connects only to Feishu's domain and
//! never Lark's, and reads a message as failed unless the app holds a
//! permission it has no other use for. What is needed is the long
//! connection, four calls and one event, and those are the next file.

pub mod connection;

use crate::platform::{Description, Field, FieldKind, Setup};

/// The two domains, the word for each as the settings file writes it.
pub const DOMAINS: &[&str] = &["feishu", "lark"];

/// Feishu, as the settings page and the relay know it.
pub static DESCRIPTION: Description = Description {
    key: "feishu",
    name: "Feishu",
    fields: &[
        Field {
            key: "app_id",
            name: obelus_i18n::bot::feishu_app_id,
            about: obelus_i18n::bot::feishu_app_id_about,
            kind: FieldKind::Text,
        },
        Field {
            key: "app_secret",
            name: obelus_i18n::bot::feishu_app_secret,
            about: obelus_i18n::bot::feishu_app_secret_about,
            kind: FieldKind::Secret { looks_like: "" },
        },
        Field {
            key: "domain",
            name: obelus_i18n::bot::feishu_domain,
            about: obelus_i18n::bot::feishu_domain_about,
            kind: FieldKind::Choice(DOMAINS),
        },
    ],
    setup: Setup::Steps {
        name: obelus_i18n::bot::feishu_console,
        // One app per machine, for the reason Slack's row gives: the long
        // connection hands each event to one of an app's connections at
        // random.
        // And the card callback by the same connection, or a question's
        // card draws and refuses every press with 200340.
        about: obelus_i18n::bot::feishu_console_about,
        url: "https://open.feishu.cn/app",
    },
    begin: "Start a topic here to talk to an agent.",
    connect: |told, sink| {
        let said = |key| told.get(key).cloned().unwrap_or_default();
        connection::start(said("app_id"), said("app_secret"), &said("domain"), sink)
    },
};
