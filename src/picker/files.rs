//! Gathering the files the picker can offer.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};

use ignore::WalkBuilder;

use crate::event::Event;

/// How many paths to send at a time.
///
/// One message per file would wake the loop once per file and redraw a list
/// that is about to change again. One message for everything would leave the
/// picker empty for as long as the walk takes on a large tree.
const BATCH: usize = 512;

/// Walks a directory tree on its own thread, sending paths back in batches.
///
/// `generation` comes back with every batch. The picker can be closed and
/// reopened while a walk is still running, and the batches from the old one
/// have to be recognizable as stale rather than merged into the new list.
///
/// A thread because `WalkBuilder` is a blocking API, and the walk of a large
/// tree is long enough that the picker has to be usable while it runs.
pub fn spawn_walk(root: &Path, generation: u64, sender: Sender<Event>) {
    let root = root.to_path_buf();
    let outcome = std::thread::Builder::new()
        .name("obelus-walk".to_string())
        .spawn(move || {
            let mut batch: Vec<PathBuf> = Vec::with_capacity(BATCH);

            // `.gitignore` and friends are respected, and hidden files are
            // skipped: a reader looking for a file wants the ones under
            // version control, not `target` and `.git`.
            for entry in WalkBuilder::new(&root).build() {
                let entry = match entry {
                    Ok(entry) => entry,
                    // An unreadable directory is not worth abandoning the walk
                    // over.
                    Err(error) => {
                        tracing::debug!(%error, "skipping an entry");
                        continue;
                    }
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                // Relative to the root, which is what the picker shows and
                // what the reader typed to get here.
                let path = entry
                    .path()
                    .strip_prefix(&root)
                    .unwrap_or_else(|_| entry.path())
                    .to_path_buf();
                batch.push(path);

                if batch.len() >= BATCH {
                    let paths = std::mem::replace(&mut batch, Vec::with_capacity(BATCH));
                    // The receiver is gone, so the loop has ended.
                    if sender
                        .send(Event::FilesFound { generation, paths })
                        .is_err()
                    {
                        return;
                    }
                }
            }

            if !batch.is_empty() {
                let _ = sender.send(Event::FilesFound {
                    generation,
                    paths: batch,
                });
            }
        });

    if let Err(error) = outcome {
        tracing::warn!(%error, "not walking the tree");
    }
}
