//! Framing, and the two threads that carry it.
//!
//! A language server speaks JSON-RPC over its own stdin and stdout, framed the
//! way HTTP frames a body: a `Content-Length` header, a blank line, then
//! exactly that many bytes. Nothing about it is line-oriented, so a reader
//! that assumes messages arrive whole, or that a read stops on a boundary,
//! works until the day a message is large enough to be split.
//!
//! Reading and writing get a thread each. Reading blocks by nature. Writing
//! blocks too, and that is the less obvious one: a server busy indexing stops
//! draining its stdin, the pipe fills, and a write from the main loop would
//! hold the whole interface until it drained.

use anyhow::{Context as _, Result, bail};

/// Reads one message's bytes, or `None` at a clean end of stream.
///
/// The body, unparsed. Framing is cheap and parsing is not -- semantic
/// tokens for a two-thousand-line file is 473 KiB of JSON that takes about
/// twenty milliseconds -- so the two are separated: the framing happens on
/// the runtime, where it costs no thread because it is only waiting, and
/// the parsing goes somewhere a twenty-millisecond pause harms nobody.
pub async fn read_body<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _};

    let mut length: Option<usize> = None;
    let mut header = String::new();

    loop {
        header.clear();
        let read = reader
            .read_line(&mut header)
            .await
            .context("reading a header")?;
        if read == 0 {
            return if length.is_some() {
                bail!("the stream ended between a header and its body")
            } else {
                Ok(None)
            };
        }

        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(
                value
                    .trim()
                    .parse()
                    .with_context(|| format!("a Content-Length of {value:?}"))?,
            );
        }
    }

    let Some(length) = length else {
        bail!("a message arrived with no Content-Length");
    };

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("reading a message body")?;
    Ok(Some(body))
}

/// One message, framed, ready to write.
///
/// The one place that knows the shape. The length is in bytes, not
/// characters: a message containing anything outside ASCII -- a path, a
/// hover in Chinese -- is longer than it looks.
#[must_use]
pub fn framed(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One message off the reader the client uses.
    ///
    /// The framing is what this file does; the parsing that follows it is
    /// the client's. So a test frames some text, reads the bytes back and
    /// reads them as JSON, which is the same two steps in the same order.
    async fn read(stream: &str) -> Option<serde_json::Value> {
        let mut reader = tokio::io::BufReader::new(stream.as_bytes());
        let body = read_body(&mut reader).await.expect("reading")?;
        Some(serde_json::from_slice(&body).expect("parsing"))
    }

    /// The same, for the streams that are supposed to fail.
    async fn refused(stream: &str) -> bool {
        let mut reader = tokio::io::BufReader::new(stream.as_bytes());
        read_body(&mut reader).await.is_err()
    }

    #[tokio::test]
    async fn a_message_comes_back_whole() {
        let message = read(&framed(r#"{"jsonrpc":"2.0","id":1,"result":null}"#))
            .await
            .expect("a message");
        assert_eq!(message["id"], 1);
    }

    #[tokio::test]
    async fn several_messages_come_back_in_order() {
        let stream = framed(r#"{"id":1}"#) + &framed(r#"{"id":2}"#) + &framed(r#"{"id":3}"#);
        let mut reader = tokio::io::BufReader::new(stream.as_bytes());
        for expected in 1..=3 {
            let body = read_body(&mut reader)
                .await
                .expect("reading")
                .expect("a message");
            let message: serde_json::Value = serde_json::from_slice(&body).expect("parsing");
            assert_eq!(message["id"], expected);
        }
        assert!(
            read_body(&mut reader).await.expect("reading").is_none(),
            "the stream did not end cleanly"
        );
    }

    /// Headers obelus does not know are skipped, which the protocol says to
    /// do and which real servers rely on.
    #[tokio::test]
    async fn an_unknown_header_is_stepped_over() {
        let body = r#"{"hover":true}"#;
        let stream = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(read(&stream).await.expect("a message")["hover"], true);
    }

    /// The length is in bytes, not characters.
    ///
    /// The body here holds real multi-byte characters rather than their
    /// JSON escapes -- which is the whole test: `"\u4f60"` in the source is
    /// six ASCII bytes and counting either way gives six. A reader or a
    /// writer that counted characters would cut every message after the
    /// first one short, and this is what says so.
    #[tokio::test]
    async fn the_length_is_bytes_and_not_characters() {
        let body = "{\"text\":\"\u{4f60}\u{597d}\"}";
        assert!(
            body.len() > body.chars().count(),
            "this test is about nothing: the body is all ASCII"
        );
        let stream = framed(body) + &framed(r#"{"after":true}"#);
        let mut reader = tokio::io::BufReader::new(stream.as_bytes());

        let first = read_body(&mut reader)
            .await
            .expect("reading")
            .expect("first");
        let first: serde_json::Value = serde_json::from_slice(&first).expect("parsing");
        assert_eq!(first["text"], "\u{4f60}\u{597d}");
        // The one that follows is what a wrong length actually breaks: the
        // reader would start it in the middle of the one before.
        let second = read_body(&mut reader)
            .await
            .expect("reading")
            .expect("second");
        let second: serde_json::Value = serde_json::from_slice(&second).expect("parsing");
        assert_eq!(second["after"], true);
    }

    /// A stream that stops between a header and its body is a broken
    /// server, not a clean end -- and the two must not look alike, because
    /// one is worth reporting and the other is how every session ends.
    #[tokio::test]
    async fn a_body_that_never_came_is_an_error() {
        assert!(refused("Content-Length: 100\r\n\r\n{\"id\":1}").await);
        assert!(refused("Content-Length: 100\r\n").await);
    }

    #[tokio::test]
    async fn a_header_with_no_length_is_an_error() {
        assert!(refused("Content-Type: text/plain\r\n\r\n{}").await);
    }

    #[tokio::test]
    async fn a_clean_end_of_stream_is_not_an_error() {
        let mut reader = tokio::io::BufReader::new(&b""[..]);
        assert!(read_body(&mut reader).await.expect("reading").is_none());
    }
}
