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

use std::io::{BufRead, Write};

use anyhow::{Context as _, Result, bail};
use serde_json::Value;

/// Reads one message, or `None` at a clean end of stream.
///
/// Headers other than `Content-Length` are skipped: the protocol allows
/// `Content-Type` and says to ignore what you do not know.
pub fn read_message<R>(reader: &mut R) -> Result<Option<Value>>
where
    R: BufRead,
{
    let mut length: Option<usize> = None;
    let mut header = String::new();

    loop {
        header.clear();
        let read = reader.read_line(&mut header).context("reading a header")?;
        if read == 0 {
            return if length.is_some() {
                // Headers arrived and then the stream ended, so a body was
                // promised and never came.
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
        .context("reading a message body")?;
    serde_json::from_slice(&body)
        .with_context(|| format!("parsing a {length}-byte message"))
        .map(Some)
}

/// Writes one message, framed.
pub fn write_message<W>(writer: &mut W, body: &str) -> Result<()>
where
    W: Write,
{
    // The length is in bytes, not characters. A message containing anything
    // outside ASCII — a path, a hover in Chinese — is longer than it looks.
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush().context("flushing a message")
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Read};

    use super::*;

    /// A reader that hands over one byte at a time, so every message is split
    /// at every possible place. Assuming a read stops on a message boundary is
    /// the mistake that works until a message is big enough not to.
    struct OneByteAtATime<'a> {
        remaining: &'a [u8],
    }

    impl Read for OneByteAtATime<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining.is_empty() || out.is_empty() {
                return Ok(0);
            }
            out[0] = self.remaining[0];
            self.remaining = &self.remaining[1..];
            Ok(1)
        }
    }

    fn framed(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    }

    #[test]
    fn a_message_comes_back_whole() {
        let stream = framed(r#"{"jsonrpc":"2.0","id":1,"result":null}"#);
        let mut reader = BufReader::new(stream.as_bytes());
        let message = read_message(&mut reader)
            .expect("reading")
            .expect("a message");
        assert_eq!(message["id"], 1);
    }

    #[test]
    fn several_messages_come_back_in_order() {
        let stream = framed(r#"{"id":1}"#) + &framed(r#"{"id":2}"#) + &framed(r#"{"id":3}"#);
        let mut reader = BufReader::new(stream.as_bytes());
        for expected in 1..=3 {
            let message = read_message(&mut reader)
                .expect("reading")
                .expect("a message");
            assert_eq!(message["id"], expected);
        }
        assert!(read_message(&mut reader).expect("reading").is_none());
    }

    #[test]
    fn a_message_split_at_every_byte_still_comes_back() {
        let stream = framed(r#"{"id":1,"method":"a"}"#) + &framed(r#"{"id":2,"method":"b"}"#);
        let mut reader = BufReader::new(OneByteAtATime {
            remaining: stream.as_bytes(),
        });
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("first")["method"],
            "a"
        );
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("second")["method"],
            "b"
        );
    }

    /// The length is bytes. A body with anything outside ASCII in it is longer
    /// than its character count, and using the wrong one desynchronises the
    /// stream permanently: every message after it starts mid-body.
    #[test]
    fn the_length_is_bytes_and_not_characters() {
        let body = "{\"hover\":\"\u{4f60}\u{597d}\"}";
        assert!(
            body.len() > body.chars().count(),
            "the sample has to be multi-byte or this proves nothing"
        );

        let stream = framed(body) + &framed(r#"{"after":true}"#);
        let mut reader = BufReader::new(stream.as_bytes());
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("first")["hover"],
            "\u{4f60}\u{597d}"
        );
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("second")["after"],
            true,
            "the stream desynchronised after a multi-byte body"
        );
    }

    #[test]
    fn other_headers_are_skipped() {
        let body = r#"{"id":7}"#;
        let stream = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut reader = BufReader::new(stream.as_bytes());
        assert_eq!(
            read_message(&mut reader)
                .expect("reading")
                .expect("a message")["id"],
            7
        );
    }

    #[test]
    fn a_clean_end_of_stream_is_not_an_error() {
        let mut reader = BufReader::new(&b""[..]);
        assert!(read_message(&mut reader).expect("reading").is_none());
    }

    /// A server killed mid-message. Reporting it as the end of the stream
    /// would look like a clean exit.
    #[test]
    fn a_truncated_body_is_an_error() {
        let stream = "Content-Length: 100\r\n\r\n{\"id\":1}";
        let mut reader = BufReader::new(stream.as_bytes());
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn a_message_with_no_length_is_an_error() {
        let mut reader = BufReader::new(&b"Content-Type: whatever\r\n\r\n{}"[..]);
        assert!(read_message(&mut reader).is_err());
    }

    /// Round-tripped through a multi-byte body, because that is the only kind
    /// that tells a byte count from a character count. Writing the character
    /// count desynchronises the stream permanently and the first message is
    /// still fine, so an ASCII round trip proves nothing.
    #[test]
    fn what_is_written_can_be_read_back() {
        let first = "{\"method\":\"hover\",\"text\":\"\u{4f60}\u{597d}\u{4e16}\u{754c}\"}";
        let second = r#"{"method":"initialized"}"#;

        let mut written = Vec::new();
        write_message(&mut written, first).expect("writing");
        write_message(&mut written, second).expect("writing");

        let mut reader = BufReader::new(written.as_slice());
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("first")["text"],
            "\u{4f60}\u{597d}\u{4e16}\u{754c}"
        );
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("second")["method"],
            "initialized",
            "the second message did not start where the first ended"
        );
    }

    /// Headers arrived and then the stream stopped, so a body was promised and
    /// never came. Reporting that as the end of the stream would look like the
    /// server shutting down cleanly, and obelus would stop asking it things
    /// rather than saying it had died.
    #[test]
    fn a_stream_that_stops_between_a_header_and_its_body_is_an_error() {
        let mut reader = BufReader::new(&b"Content-Length: 100\r\n"[..]);
        let outcome = read_message(&mut reader);
        assert!(outcome.is_err(), "got {outcome:?}");
    }
}
