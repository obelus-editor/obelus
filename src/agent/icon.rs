//! The registry's marks: fetching them, and keeping them on disk.
//!
//! Every entry in the registry carries an icon -- a monochrome SVG of a few
//! hundred bytes, drawn in `currentColor`, sixteen pixels square. That last
//! detail is what makes them worth having in a terminal: sixteen pixels is
//! one text row, so an agent's own mark fits exactly where a glyph would
//! have gone.
//!
//! This module only moves the file. What turns it into pixels is
//! [`crate::ui::image`], because the size to draw it at and the colour to
//! ink it in are the view's business and are not known here.
//!
//! Fetched one at a time on one thread, and cached by id. Forty files of
//! four kilobytes is a directory obelus can keep for good: these change
//! when an agent changes its logo, which is not on the scale a reader
//! notices.

use std::{path::PathBuf, time::Duration};

use crate::{agent::Event, sink::Sink};

/// How long to wait for one icon.
///
/// Short: a mark is decoration, and a reader who is looking at the page now
/// is not helped by one that arrives after they have left.
const PATIENCE: Duration = Duration::from_secs(10);

/// The most one icon may weigh.
///
/// The largest in the registry today is four kilobytes. Sixty-four is room
/// for a much more detailed drawing and a limit on what a wrong URL can do
/// to memory.
const MOST: u64 = 64 * 1024;

/// Where the icons are kept between sessions.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("obelus").join("icons"))
}

/// Where one agent's icon is kept.
///
/// Named by the registry's id, which is the only name obelus has for an
/// agent -- and, since it is also a path segment here, one that has to be
/// checked: a registry entry is somebody else's string, and `../` in it
/// would name a file outside the cache.
#[must_use]
pub fn path_for(id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || !id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
        })
        || id.starts_with('.')
    {
        return None;
    }
    Some(directory()?.join(format!("{id}.svg")))
}

/// One icon, from a previous session.
#[must_use]
pub fn cached(id: &str) -> Option<String> {
    std::fs::read_to_string(path_for(id)?).ok()
}

/// Fetches the icons obelus does not already have.
///
/// The cached ones are sent first and from the same thread, so the page
/// draws whatever it had while the rest arrive. Each one is its own event:
/// forty small drawings landing one by one is forty cheap frames, and the
/// alternative is a page with no marks on it until the last one lands.
pub fn spawn_fetch(wanted: Vec<(String, String)>, sender: impl Sink<Event>) {
    if wanted.is_empty() {
        return;
    }
    crate::runtime::handle().spawn(async move {
        let mut missing = Vec::new();
        for (id, url) in wanted {
            match cached(&id) {
                Some(svg) => {
                    if sender.send(Event::Icon { id, svg }).is_err() {
                        return;
                    }
                }
                None => missing.push((id, url)),
            }
        }
        if missing.is_empty() {
            return;
        }
        let agent = match http() {
            Ok(agent) => agent,
            Err(error) => {
                tracing::warn!(%error, "not fetching agent icons");
                return;
            }
        };
        for (id, url) in missing {
            match fetch(&agent, &url).await {
                Ok(svg) => {
                    store(&id, &svg);
                    if sender.send(Event::Icon { id, svg }).is_err() {
                        return;
                    }
                }
                // Nothing is sent for one that did not arrive. A missing
                // mark is a card that looks the way it looked before
                // obelus fetched marks at all, which is why this whole
                // path is allowed to fail quietly.
                Err(error) => tracing::debug!(id, %error, "no icon for this agent"),
            }
        }
    });
}

/// Keeps one icon for the next session.
fn store(id: &str, svg: &str) {
    let Some(path) = path_for(id) else { return };
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    if let Err(error) = std::fs::write(&path, svg) {
        tracing::debug!(%error, "not caching an agent icon");
    }
}

/// The thing that does the asking, made once for the whole batch so that
/// forty icons off one host cost one connection.
fn http() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(PATIENCE)
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// One icon, over the network.
async fn fetch(agent: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    let response = agent.get(url).send().await?.error_for_status()?;
    text_within(response, MOST).await
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
pub(crate) async fn text_within(
    response: reqwest::Response,
    most: u64,
) -> Result<String, reqwest::Error> {
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
    use super::path_for;

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

    #[test]
    fn an_id_cannot_name_a_file_outside_the_cache() {
        // The ids the registry actually uses.
        for id in ["claude-acp", "gemini", "py_agent", "a.b"] {
            let path = path_for(id).expect("a path for a plain id");
            assert!(path.ends_with(format!("{id}.svg")));
        }
        // And what obelus refuses to take from somebody else's file.
        for id in ["", "../../evil", "a/b", ".ssh", "x\0y"] {
            assert!(path_for(id).is_none(), "{id} named a file");
        }
    }
}
