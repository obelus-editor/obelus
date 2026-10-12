//! Fetching something Obelus did not write.
//!
//! What every fetch from somebody else's server shares, whatever it is
//! fetching: the agents' registry and their marks, and the themes' index.
//! One copy, because the mistakes in it were made once and paid for -- a
//! limit applied after the reading rather than to it -- and a second copy
//! is a second place for them to be made again.

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
