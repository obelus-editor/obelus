//! Framing, and the two threads that carry it.
//!
//! The Agent Client Protocol is JSON-RPC 2.0 over the agent's own stdin and
//! stdout, one message per line -- no `Content-Length`, unlike the language
//! server protocol next door. That makes reading a `read_line`, with one
//! thing to be careful about: a line has no length limit, so a reader with a
//! fixed buffer works until an agent sends a large diff. `BufRead::read_line`
//! grows, so this is a matter of not reinventing it.
//!
//! Reading and writing get a thread each, for the same two reasons they do in
//! [`crate::lsp::transport`]: reading blocks by nature, and writing blocks
//! too -- an agent thinking about a prompt stops draining its stdin, and a
//! write from the main loop would hold the interface until it started again.

use std::io::{BufRead, Write};

use anyhow::{Context as _, Result};
use serde_json::Value;

/// Reads one message, or `None` at a clean end of stream.
///
/// Blank lines are skipped rather than reported: they are not messages, and
/// an agent that prints one should not look like an agent that has exited.
pub fn read_message<R>(reader: &mut R) -> Result<Option<Value>>
where
    R: BufRead,
{
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).context("reading a message")?;
        if read == 0 {
            return Ok(None);
        }
        let body = line.trim();
        if body.is_empty() {
            continue;
        }
        return serde_json::from_str(body)
            .with_context(|| {
                // Truncated, because the line that failed to parse can be a
                // whole file's contents.
                let sample: String = body.chars().take(200).collect();
                format!("parsing {sample:?}")
            })
            .map(Some);
    }
}

/// Writes one message, framed.
pub fn write_message<W>(writer: &mut W, body: &str) -> Result<()>
where
    W: Write,
{
    // One line, so anything with a newline in it would be two messages. What
    // `serde_json` produces has none -- newlines inside strings are escaped
    // -- and this is the assumption that says so out loud.
    debug_assert!(!body.contains('\n'), "a message with a newline in it");
    writeln!(writer, "{body}")?;
    writer.flush().context("flushing a message")
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Read};

    use super::*;

    /// A reader that hands over one byte at a time, so every message is split
    /// at every possible place.
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

    #[test]
    fn messages_come_back_in_order() {
        let stream = "{\"id\":1}\n{\"id\":2}\n";
        let mut reader = BufReader::new(stream.as_bytes());
        for expected in 1..=2 {
            let message = read_message(&mut reader)
                .expect("reading")
                .expect("a message");
            assert_eq!(message["id"], expected);
        }
        assert!(read_message(&mut reader).expect("reading").is_none());
    }

    #[test]
    fn a_message_split_at_every_byte_still_comes_back() {
        let stream = "{\"method\":\"a\"}\n{\"method\":\"b\"}\n";
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

    /// A newline inside a string is escaped, so a message carrying a file
    /// stays one message. An agent's answer is mostly this: prose with
    /// newlines in it.
    #[test]
    fn a_body_with_newlines_in_its_strings_is_one_message() {
        let body = serde_json::json!({ "text": "one\ntwo\nthree" }).to_string();
        assert!(!body.contains('\n'), "serde_json escaped nothing");

        let mut written = Vec::new();
        write_message(&mut written, &body).expect("writing");
        write_message(&mut written, r#"{"after":true}"#).expect("writing");

        let mut reader = BufReader::new(written.as_slice());
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("first")["text"],
            "one\ntwo\nthree"
        );
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("second")["after"],
            true,
            "the stream desynchronised after a body with newlines in it"
        );
    }

    /// Agents print blank lines. Reporting one as the end of the stream would
    /// have obelus decide the agent had exited.
    #[test]
    fn blank_lines_are_not_the_end_of_the_stream() {
        let mut reader = BufReader::new(&b"\n\n{\"id\":9}\n"[..]);
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("it")["id"],
            9
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_an_error() {
        let mut reader = BufReader::new(&b"listening on stdio\n"[..]);
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn a_clean_end_of_stream_is_not_an_error() {
        let mut reader = BufReader::new(&b""[..]);
        assert!(read_message(&mut reader).expect("reading").is_none());
    }

    /// A line much longer than any read buffer. An agent handing back a file
    /// it read is exactly this, and a fixed-size buffer would cut it in half
    /// and desynchronise the stream.
    #[test]
    fn a_very_long_line_comes_back_whole() {
        let text = "x".repeat(200_000);
        let body = serde_json::json!({ "text": text }).to_string();
        let stream = format!("{body}\n{{\"after\":true}}\n");
        let mut reader = BufReader::new(stream.as_bytes());
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("first")["text"]
                .as_str()
                .map(str::len),
            Some(200_000)
        );
        assert_eq!(
            read_message(&mut reader).expect("reading").expect("second")["after"],
            true
        );
    }
}
