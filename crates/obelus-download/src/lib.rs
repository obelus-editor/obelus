//! Fetching something Obelus did not write.
//!
//! What every fetch from somebody else's server shares, whatever it is
//! fetching: the agents' registry and their marks, an agent's archive, and
//! the themes' index and packages. One copy, because the mistakes in it
//! were made once and paid for -- a limit applied after the reading rather
//! than to it -- and a second copy is a second place for them to be made
//! again.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

/// How often to report progress.
///
/// Every chunk would wake the loop hundreds of times a second to redraw a
/// number that has not visibly changed.
const REPORT: Duration = Duration::from_millis(120);

/// How far a download has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes fetched so far.
    pub done: u64,
    /// How many there are, when the server says.
    pub total: Option<u64>,
    /// How long it has been running.
    pub elapsed: Duration,
}

impl Progress {
    /// How much longer, if that can be known.
    ///
    /// From the rate so far, which is the only honest estimate available:
    /// nothing here knows the shape of the rest of the download. `None`
    /// when there is no length to divide by, which is exactly when a
    /// package manager is doing the work.
    #[must_use]
    pub fn remaining(self) -> Option<Duration> {
        let total = self.total?;
        if self.done == 0 || total <= self.done {
            return None;
        }
        let rate = self.done as f64 / self.elapsed.as_secs_f64().max(0.001);
        let left = (total - self.done) as f64 / rate;
        left.is_finite().then(|| Duration::from_secs_f64(left))
    }

    /// How far through, from nothing to one.
    #[must_use]
    pub fn fraction(self) -> Option<f64> {
        let total = self.total?;
        (total > 0).then(|| (self.done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

/// Fetches the whole of what is at `url`, saying how far it has got at
/// most every [`REPORT`] -- counted from `started`, which is when the
/// reader asked, not when the first byte came.
///
/// The one part of an install that only waits, so it is the one part that
/// is awaited: the bytes arrive in chunks, and a chunk is a chance to say
/// how far along it is.
///
/// # Errors
///
/// When the server will not give it, or stops part way, in words that say
/// which.
pub async fn bytes(
    url: &str,
    started: Instant,
    mut report: impl FnMut(Progress),
) -> Result<Vec<u8>, String> {
    use futures::StreamExt as _;

    let http = reqwest::Client::builder()
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("fetching it: {error}"))?;
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("fetching it: {error}"))?;
    let total = response.content_length();

    let mut bytes: Vec<u8> = Vec::with_capacity(
        total
            .and_then(|total| usize::try_from(total).ok())
            .unwrap_or(0),
    );
    let mut body = response.bytes_stream();
    let mut said = Instant::now();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| format!("reading it: {error}"))?;
        bytes.extend_from_slice(&chunk);
        if said.elapsed() >= REPORT {
            said = Instant::now();
            report(Progress {
                done: bytes.len() as u64,
                total,
                elapsed: started.elapsed(),
            });
        }
    }
    Ok(bytes)
}

/// Whether `bytes` hash to `expected`, a SHA-256 written in hexadecimal --
/// which is what says they are the file the registry described, and may be
/// unpacked at all.
#[must_use]
pub fn hashes_to(bytes: &[u8], expected: &str) -> bool {
    let digest = Sha256::digest(bytes);
    let got: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    got.eq_ignore_ascii_case(expected)
}

/// Unpacks whichever shape the archive is, by what its name says it is.
///
/// # Errors
///
/// When it is not a shape this unpacks, or will not unpack.
pub fn unpack(bytes: &[u8], name: &str, into: &Path) -> Result<(), String> {
    let name = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let reader = flate2::read::GzDecoder::new(bytes);
        return tar::Archive::new(reader)
            .unpack(into)
            .map_err(|error| format!("unpacking it: {error}"));
    }
    if name.ends_with(".zip") {
        let reader = std::io::Cursor::new(bytes);
        let mut zip =
            zip::ZipArchive::new(reader).map_err(|error| format!("unpacking it: {error}"))?;
        return zip
            .extract(into)
            .map_err(|error| format!("unpacking it: {error}"));
    }
    // `.tar.bz2` is four of the agent registry's hundred entries, and a
    // shape Obelus has no unpacker for. Saying which shape beats saying
    // "failed".
    Err(format!("Obelus cannot unpack {name}"))
}

/// Reads a response body, up to a limit, as text.
///
/// The limit is on the *reading*: a body larger than this is not held in
/// memory and then cut down, it is simply not read past. Cutting it down
/// afterwards was worse in both directions -- the whole of a hostile
/// response went into memory first, and the cut landed wherever the byte
/// count fell, which inside a multi-byte character is a panic.
///
/// What arrives is whatever whole characters fit. A mark or a registry that
/// has outgrown the limit is still worth the part of it that is readable.
///
/// # Errors
///
/// When the body stops arriving part way.
pub async fn text_within(response: reqwest::Response, most: u64) -> Result<String, reqwest::Error> {
    use futures::StreamExt as _;

    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let mut bytes: Vec<u8> = Vec::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        let room = most.saturating_sub(bytes.len());
        if room == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    // Lossy rather than refused, and for the same reason the limit is: what
    // a truncation leaves in the middle of a character is a replacement
    // mark at the end of a string, not an error about the whole fetch.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    /// A hash is the file's whatever case it is written in, and a file
    /// that is not the one described is not.
    ///
    /// Deliberate break: comparing the two as they are written, which takes
    /// a registry's capitals for a different file.
    #[test]
    fn a_file_is_the_one_its_hash_says() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(super::hashes_to(b"abc", abc));
        assert!(super::hashes_to(b"abc", &abc.to_uppercase()));
        assert!(!super::hashes_to(b"abd", abc));
    }

    /// An archive is unpacked into where it is put, and a shape there is no
    /// unpacker for is said by its name.
    ///
    /// Deliberate break: only `.tgz` taken for a gzipped tar, which leaves
    /// `.tar.gz` -- what nearly everything ships -- refused.
    #[test]
    fn an_archive_is_unpacked_or_said_to_be_one_that_cannot_be() {
        let mut packed = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let words = b"base = \"dark\"\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(words.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        packed
            .append_data(&mut header, "rain/theme.toml", &words[..])
            .expect("the entry");
        let bytes = packed
            .into_inner()
            .expect("the tar")
            .finish()
            .expect("the gzip");

        let into = std::env::temp_dir().join(format!("obelus-download-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&into);
        std::fs::create_dir_all(&into).expect("the directory");
        super::unpack(&bytes, "packages/rain-3f9c.tar.gz", &into).expect("unpacked");
        assert_eq!(
            std::fs::read(into.join("rain/theme.toml")).expect("the file"),
            words
        );
        let refused = super::unpack(&bytes, "rain.tar.bz2", &into).expect_err("refused");
        assert!(refused.contains("rain.tar.bz2"), "{refused}");
        let _ = std::fs::remove_dir_all(&into);
    }
    /// A body cut at the limit comes back, and comes back readable.
    ///
    /// The limit is a byte count and a mark is not ASCII, so the cut lands
    /// inside a character about as often as not. Slicing a `String` there
    /// panics, which is what this replaced: the fetch took the whole body
    /// into memory and then cut it, and a 64 KiB mark whose 65536th byte
    /// was mid-character took the icon task down with it.
    #[test]
    fn a_cut_lands_wherever_it_lands_and_is_still_a_string() {
        // A string of three-byte characters, cut at a byte that cannot be
        // a boundary.
        let whole: String = "\u{4f60}".repeat(64);
        let most = 100;
        assert!(
            !whole.is_char_boundary(most),
            "this test is about nothing: the cut is on a boundary"
        );
        let cut = String::from_utf8_lossy(&whole.as_bytes()[..most]).into_owned();
        assert!(
            cut.ends_with('\u{fffd}'),
            "a character cut in half is not marked: {cut:?}"
        );
        assert_eq!(
            cut.chars().filter(|c| *c == '\u{4f60}').count(),
            most / 3,
            "the characters that did fit did not survive"
        );
    }
}
