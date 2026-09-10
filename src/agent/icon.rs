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

use std::{path::PathBuf, sync::mpsc::Sender, time::Duration};

use crate::event::Event;

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
pub fn spawn_fetch(wanted: Vec<(String, String)>, sender: Sender<Event>) {
    if wanted.is_empty() {
        return;
    }
    let outcome = std::thread::Builder::new()
        .name("obelus-icons".to_string())
        .spawn(move || {
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
                match fetch(&agent, &url) {
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
    if let Err(error) = outcome {
        tracing::warn!(%error, "not fetching agent icons");
    }
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
fn http() -> Result<ureq::Agent, ureq::Error> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(PATIENCE))
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent())
}

/// One icon, over the network.
fn fetch(agent: &ureq::Agent, url: &str) -> Result<String, ureq::Error> {
    let mut response = agent.get(url).call()?;
    response
        .body_mut()
        .with_config()
        .limit(MOST)
        .read_to_string()
}

#[cfg(test)]
mod tests {
    use super::path_for;

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
