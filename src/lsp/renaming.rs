//! Telling a server that a file is about to be somewhere else.
//!
//! A file that moves takes its meaning with it. `src/lsp/hint.rs` is the
//! module `lsp::hint`, and moving it to `src/lsp/hints.rs` leaves every
//! `use crate::lsp::hint` in the project naming something that is not
//! there any more. The server is the only thing that knows which words
//! those are: obelus moves the file, and asks first what else has to
//! change so that the project still says what it said.
//!
//! Two messages, either side of the move. `workspace/willRenameFiles` is
//! a question asked before, and its answer is an edit; `didRenameFiles`
//! is a statement made after, so that a server which was not asked still
//! learns the file is gone from where it was.
//!
//! A server says which paths it wants to hear about, and means it.
//! rust-analyzer asks for `**/*.rs` as files and `**` as folders -- which
//! is to say every Rust file and every directory -- and a server told
//! about a `.png` would have to work out for itself that it has nothing
//! to say. The filters are globs, matched here with the same crate the
//! file walk already uses, rather than guessed at from the extension.

use std::path::Path;

use globset::GlobBuilder;
use lsp_types::{
    FileOperationFilter, FileOperationPatternKind, FileOperationRegistrationOptions,
    ServerCapabilities,
};
use serde_json::Value;

/// Whether the server wants to be asked before this path moves.
#[must_use]
pub fn asked_before(capabilities: &ServerCapabilities, path: &Path, directory: bool) -> bool {
    matches(
        registered(capabilities, |operations| operations.will_rename.as_ref()),
        path,
        directory,
    )
}

/// Whether the server wants to be told after it has.
#[must_use]
pub fn told_after(capabilities: &ServerCapabilities, path: &Path, directory: bool) -> bool {
    matches(
        registered(capabilities, |operations| operations.did_rename.as_ref()),
        path,
        directory,
    )
}

/// What both messages carry: where it was and where it is.
///
/// `None` for a path that cannot be a uri, which is a path obelus cannot
/// name to a server at all.
#[must_use]
pub fn params(from: &Path, to: &Path) -> Option<Value> {
    let from = super::client::uri_for(from).ok()?;
    let to = super::client::uri_for(to).ok()?;
    Some(serde_json::json!({
        "files": [{ "oldUri": from, "newUri": to }],
    }))
}

/// One of the two registrations, if the server made it.
fn registered<'a>(
    capabilities: &'a ServerCapabilities,
    which: impl Fn(
        &'a lsp_types::WorkspaceFileOperationsServerCapabilities,
    ) -> Option<&'a FileOperationRegistrationOptions>,
) -> &'a [FileOperationFilter] {
    capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.file_operations.as_ref())
        .and_then(which)
        .map_or(&[], |options| options.filters.as_slice())
}

/// Whether any filter covers this path.
///
/// The glob is matched against the path rather than against the uri the
/// message will carry. Both work for the filters servers actually
/// register -- `**/*.rs` matches either -- and the path is what a glob in
/// the protocol is about: a server that wrote `src/**` would mean the
/// directory, not a string that happens to start `file:///`.
fn matches(filters: &[FileOperationFilter], path: &Path, directory: bool) -> bool {
    let kind = match directory {
        true => FileOperationPatternKind::Folder,
        false => FileOperationPatternKind::File,
    };
    filters.iter().any(|filter| {
        // A scheme the server named and did not name `file` is about
        // documents obelus has no path for.
        if filter
            .scheme
            .as_deref()
            .is_some_and(|scheme| scheme != "file")
        {
            return false;
        }
        // Said nothing about which: both.
        if filter
            .pattern
            .matches
            .as_ref()
            .is_some_and(|wanted| *wanted != kind)
        {
            return false;
        }
        let ignoring_case = filter
            .pattern
            .options
            .as_ref()
            .and_then(|options| options.ignore_case)
            .unwrap_or(false);
        GlobBuilder::new(&filter.pattern.glob)
            .case_insensitive(ignoring_case)
            .build()
            .is_ok_and(|glob| glob.compile_matcher().is_match(path))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What rust-analyzer actually registers, taken off the wire.
    fn rust_analyzer() -> ServerCapabilities {
        serde_json::from_value(serde_json::json!({
            "workspace": {
                "fileOperations": {
                    "willRename": {
                        "filters": [
                            { "scheme": "file", "pattern": { "glob": "**/*.rs", "matches": "file" } },
                            { "scheme": "file", "pattern": { "glob": "**", "matches": "folder" } },
                        ]
                    }
                }
            }
        }))
        .expect("the capabilities a real server sent")
    }

    #[test]
    fn a_server_is_asked_about_what_it_registered_for() {
        let capabilities = rust_analyzer();
        let rust = Path::new("/home/reader/project/src/lsp/hint.rs");
        assert!(asked_before(&capabilities, rust, false));
        // A file that is not Rust: the filter says files must be `*.rs`,
        // and the folder filter does not save it.
        assert!(!asked_before(
            &capabilities,
            Path::new("/home/reader/project/README.md"),
            false
        ));
        // A directory, which the second filter is entirely about.
        assert!(asked_before(
            &capabilities,
            Path::new("/home/reader/project/src/lsp"),
            true
        ));
        // The same path as a file is the first filter's business, and it
        // has no extension.
        assert!(!asked_before(
            &capabilities,
            Path::new("/home/reader/project/src/lsp"),
            false
        ));
    }

    /// rust-analyzer registers `willRename` and not `didRename`, so the
    /// notification it never asked for is never sent.
    #[test]
    fn a_server_is_not_told_what_it_did_not_ask_about() {
        let capabilities = rust_analyzer();
        assert!(!told_after(
            &capabilities,
            Path::new("/home/reader/project/src/lsp/hint.rs"),
            false
        ));
    }

    #[test]
    fn a_server_that_registered_nothing_is_left_alone() {
        let capabilities = ServerCapabilities::default();
        let path = Path::new("/home/reader/project/src/main.rs");
        assert!(!asked_before(&capabilities, path, false));
        assert!(!told_after(&capabilities, path, false));
    }

    /// A filter with no `matches` covers both, which is what the protocol
    /// says undefined means and not what a copy of the file branch would
    /// do.
    #[test]
    fn a_filter_that_says_nothing_about_kind_covers_both() {
        let capabilities: ServerCapabilities = serde_json::from_value(serde_json::json!({
            "workspace": { "fileOperations": {
                "willRename": { "filters": [{ "pattern": { "glob": "**/*.ts" } }] }
            }}
        }))
        .expect("capabilities");
        assert!(asked_before(&capabilities, Path::new("/p/src/a.ts"), false));
        assert!(asked_before(&capabilities, Path::new("/p/src/a.ts"), true));
    }

    #[test]
    fn the_message_names_both_places() {
        let params = params(Path::new("/p/a.rs"), Path::new("/p/b.rs")).expect("two uris");
        let files = params["files"].as_array().expect("a list of one");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["oldUri"], "file:///p/a.rs");
        assert_eq!(files[0]["newUri"], "file:///p/b.rs");
    }
}
