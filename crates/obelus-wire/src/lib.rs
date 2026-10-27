//! The words between a window and the Obelus it draws, as bytes.
//!
//! **A frame is a length and a message.** Four bytes, little-endian, saying
//! how many follow, and then that many of postcard: compact, and the same
//! bytes on every platform, which is the whole of what is asked of it. What a
//! message *is* belongs to whoever sends it -- `obelus_front::link` -- and this
//! knows only how one is carried.
//!
//! **No field may be left out of the bytes.** Postcard writes every field in
//! order and reads them back by position, so a type that skips one when it
//! is empty (`skip_serializing_if`), flattens another into itself, or is one
//! of several shapes told apart by looking (`untagged`) writes bytes it
//! cannot read. Those attributes are for a format with names in it; a type
//! that crosses here goes without them.
//!
//! **Both ends say which version of the words they speak, first.** Two
//! builds of Obelus are not guaranteed to agree about a single message, so
//! the first frame either way is a [`Hello`], and a connection whose two
//! ends disagree about [`WIRE_VERSION`] says so in words and goes no further
//! -- rather than reading the next message as something it is not.

use std::io::{Read, Write};

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Which version of the words this build speaks.
///
/// Moved whenever what a message is moves: a field, a variant, an order.
/// The golden bytes beside the messages are what notices one moving without
/// this.
pub const WIRE_VERSION: u32 = 1;

/// The longest a frame may be.
///
/// A length is read before what it measures, so a length that is nonsense
/// -- a stream that is not this one, half a frame from before a reconnect
/// -- would otherwise be a gigabyte allocated on its say-so. Larger than
/// anything Obelus sends: a file dropped on a window larger than half of
/// this is refused before it is sent.
pub const MOST: usize = 64 << 20;

/// The first thing either end says.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hello {
    /// [`WIRE_VERSION`], as the end that says it has it.
    pub version: u32,
    /// Which Obelus that end is, for a sentence about the two not agreeing.
    pub build: String,
}

impl Hello {
    /// What this build says.
    #[must_use]
    pub fn here(build: &str) -> Self {
        Self {
            version: WIRE_VERSION,
            build: build.to_string(),
        }
    }

    /// Why the other end's hello is not one this end can go on from, in
    /// words for the reader -- or nothing, where it is.
    #[must_use]
    pub fn refused(&self, theirs: &Self) -> Option<String> {
        (self.version != theirs.version).then(|| {
            format!(
                "This Obelus is {} and the other is {}, and they do not speak alike",
                self.build, theirs.build
            )
        })
    }
}

/// A message as a frame: its length and its bytes.
///
/// # Errors
///
/// Where the message will not go into postcard, or goes into more than
/// [`MOST`].
pub fn framed<T: Serialize>(message: &T) -> std::io::Result<Vec<u8>> {
    let body = postcard::to_stdvec(message).map_err(std::io::Error::other)?;
    if body.len() > MOST {
        return Err(std::io::Error::other(format!(
            "a message of {} bytes, which is more than a frame holds",
            body.len()
        )));
    }
    let length = u32::try_from(body.len()).map_err(std::io::Error::other)?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// A frame's body as the message it is.
///
/// # Errors
///
/// Where the bytes are not one.
pub fn unframed<T: DeserializeOwned>(body: &[u8]) -> std::io::Result<T> {
    postcard::from_bytes(body)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// How long the frame a length says is, where it is one a frame may be.
fn length_of(length: [u8; 4], most: usize) -> std::io::Result<usize> {
    let length = usize::try_from(u32::from_le_bytes(length)).map_err(std::io::Error::other)?;
    if length > most {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("a frame says it is {length} bytes, which no frame is"),
        ));
    }
    Ok(length)
}

/// Writes one message.
///
/// # Errors
///
/// Where it will not be framed, or will not be written.
pub fn write<W: Write, T: Serialize>(writer: &mut W, message: &T) -> std::io::Result<()> {
    writer.write_all(&framed(message)?)?;
    writer.flush()
}

/// Reads one message, or nothing where the other end has finished -- which
/// is a stream ending between two frames, and not in the middle of one.
///
/// # Errors
///
/// Where the stream ends inside a frame, or what is in one is not a message.
pub fn read<R: Read, T: DeserializeOwned>(reader: &mut R) -> std::io::Result<Option<T>> {
    read_at_most(reader, MOST)
}

/// [`read`], of a frame no longer than `most`.
///
/// For what is said before the other end has shown who it is: a few dozen
/// bytes, from anybody at all on the network, who could otherwise have
/// [`MOST`] made for each connection on their say-so.
///
/// # Errors
///
/// The same as [`read`]'s, and a frame that says it is longer than `most`.
pub fn read_at_most<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    most: usize,
) -> std::io::Result<Option<T>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut body = vec![0_u8; length_of(length, most)?];
    reader.read_exact(&mut body)?;
    unframed(&body).map(Some)
}

/// [`write`], on a connection the runtime is waiting on.
///
/// # Errors
///
/// The same as [`write`]'s.
pub async fn send<W, T>(writer: &mut W, message: &T) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    writer.write_all(&framed(message)?).await?;
    writer.flush().await
}

/// [`read`], on a connection the runtime is waiting on.
///
/// # Errors
///
/// The same as [`read`]'s.
pub async fn receive<R, T>(reader: &mut R) -> std::io::Result<Option<T>>
where
    R: tokio::io::AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut body = vec![0_u8; length_of(length, MOST)?];
    reader.read_exact(&mut body).await?;
    unframed(&body).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message of the shapes that cross: an enum, a string, a list, a
    /// number that is not small.
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    enum Said {
        Nothing,
        Words(String),
        Many(Vec<u64>),
        Both { name: String, at: Option<u64> },
    }

    /// What goes in comes out, one frame after another on one stream.
    ///
    /// Broken deliberately by writing the length big-endian in `framed`:
    /// the first length read was a frame of no possible size.
    #[test]
    fn what_is_written_is_what_is_read() {
        let said = [
            Said::Nothing,
            Said::Words("über".to_string()),
            Said::Many(vec![0, 1, u64::MAX]),
            Said::Both {
                name: "a.rs".to_string(),
                at: Some(300),
            },
        ];
        let mut stream = Vec::new();
        for one in &said {
            write(&mut stream, one).expect("written");
        }
        let mut reading = stream.as_slice();
        for one in &said {
            assert_eq!(
                read::<_, Said>(&mut reading).expect("read").as_ref(),
                Some(one)
            );
        }
        assert_eq!(read::<_, Said>(&mut reading).expect("the end"), None);
    }

    /// A stream that stops inside a frame is an error, and one that stops
    /// between two is the other end finishing.
    ///
    /// Broken deliberately by answering `None` for any end in `read`: a
    /// connection cut half way through a message read as one that had
    /// said goodbye.
    #[test]
    fn a_stream_cut_inside_a_frame_is_not_one_that_finished() {
        let frame = framed(&Said::Words("cut".to_string())).expect("framed");
        let mut cut = &frame[..frame.len() - 1];
        assert!(read::<_, Said>(&mut cut).is_err());
        let mut nothing: &[u8] = &[];
        assert_eq!(read::<_, Said>(&mut nothing).expect("the end"), None);
    }

    /// A length no frame could have is refused before anything is made for
    /// it.
    ///
    /// Broken deliberately by taking the check out of `length_of`: the
    /// test asked for four gigabytes.
    #[test]
    fn a_length_no_frame_has_is_refused() {
        let mut stream: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0];
        let error = read::<_, Said>(&mut stream).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    /// A frame longer than a reader asked for at most is refused, and one
    /// as long as that is read.
    ///
    /// Broken deliberately by measuring every frame against [`MOST`] in
    /// `read_at_most`: a frame of a kilobyte got past a reader that would
    /// take sixteen bytes.
    #[test]
    fn a_frame_longer_than_was_asked_for_is_refused() {
        let said = Said::Words("x".repeat(1024));
        let mut long: &[u8] = &framed(&said).expect("framed");
        let error = read_at_most::<_, Said>(&mut long, 16).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let mut taken: &[u8] = &framed(&said).expect("framed");
        assert_eq!(
            read_at_most::<_, Said>(&mut taken, 2048).expect("read"),
            Some(said)
        );
    }

    /// The same, the other way: on a connection the runtime waits on.
    ///
    /// Broken deliberately by leaving the flush out of `send`: nothing
    /// reached the other end of the pipe, and the wait for it ran out.
    #[test]
    fn a_connection_carries_frames_both_ways() {
        let said = Said::Both {
            name: "here".to_string(),
            at: None,
        };
        let got = obelus_runtime::handle().block_on(async {
            let (near, far) = tokio::io::duplex(64);
            let (mut reading, _) = tokio::io::split(far);
            let (_, mut writing) = tokio::io::split(near);
            send(&mut tokio::io::BufWriter::new(&mut writing), &said)
                .await
                .expect("sent");
            // Bounded, so that a frame that never arrives fails here rather
            // than waiting for ever.
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                receive::<_, Said>(&mut reading),
            )
            .await
            .expect("it arrived")
            .expect("received")
        });
        assert_eq!(got, Some(said));
    }

    /// Two ends that do not speak the same version say so, and two that do
    /// say nothing.
    ///
    /// Broken deliberately by comparing the builds instead of the versions:
    /// two builds of one version refused each other.
    #[test]
    fn a_version_apart_is_said_in_words() {
        let here = Hello::here("0.18.0");
        assert_eq!(here.refused(&Hello::here("0.18.1")), None);
        let older = Hello {
            version: WIRE_VERSION + 1,
            build: "0.19.0".to_string(),
        };
        let said = here.refused(&older).expect("refused");
        assert!(said.contains("0.18.0") && said.contains("0.19.0"), "{said}");
    }

    /// The hello's own bytes, which are the one frame every build must
    /// read: it is what says whether to go on.
    ///
    /// Broken deliberately by putting `build` before `version`: these bytes
    /// moved, which is what an older build would have read wrongly.
    #[test]
    fn a_hello_is_these_bytes() {
        let frame = framed(&Hello {
            version: 1,
            build: "ob".to_string(),
        })
        .expect("framed");
        assert_eq!(frame, [4, 0, 0, 0, 1, 2, b'o', b'b']);
    }
}
