//! What a server says after its client has gone.
//!
//! Against a server that is a line of `sh`, because what is being tested
//! is the pipe and not the protocol: it says one thing as it starts, waits
//! for its input to close -- which is its client going -- and says one
//! thing more, the way a server answers a shutdown on its way out.

#![cfg(unix)]

use std::time::Duration;

use obelus_lsp::{Message, Server, client::Client};
use obelus_syntax::LanguageId;

/// One message as the protocol frames it: an empty object, which is two
/// bytes.
const SPEAKS: &str = "printf 'Content-Length: 2\\r\\n\\r\\n{}'; cat > /dev/null; \
                      printf 'Content-Length: 2\\r\\n\\r\\n{}'; sleep 5";

/// Nothing a server says after its client has gone reaches the loop: it
/// would be taken for the next server's of that language.
///
/// Broken deliberately by sending whether or not the client is still
/// listening: the word the server says on its way out arrives after.
#[test]
fn a_server_is_not_heard_after_its_client_goes() {
    let (sender, events) = std::sync::mpsc::channel::<Message>();
    let server = Server {
        command: "sh",
        arguments: &["-c", SPEAKS],
    };
    let root = std::env::temp_dir();
    let client = Client::start(LanguageId::Rust, server, &root, sender).expect("starting sh");
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("the server said nothing as it started");

    drop(client);
    assert!(
        events.recv_timeout(Duration::from_secs(1)).is_err(),
        "the server was heard after its client went"
    );
}
