//! What a server offers to type next, turned into values obelus can show.
//!
//! The protocol's completion item is a form with a dozen optional fields
//! whose defaults refer to one another: the text to put in is `textEdit`, or
//! `insertText`, or the label; what to sort by is `sortText` or the label;
//! what to filter by is `filterText` or the label. Every one of those
//! fallbacks is written out once, here, so that nothing downstream has to
//! know the protocol had a choice.
//!
//! The ranges are turned into the document's own coordinates on the way in,
//! for the same reason: a span in the encoding the server agreed to is a
//! thing only this layer should ever hold.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, Documentation,
    InsertTextFormat, MarkupContent, PositionEncodingKind, ServerCapabilities,
};
use serde_json::Value;

use crate::{coordinates::Span, text::Text, theme::SyntaxKind};

/// One thing that could be typed next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// What the list shows, and what a query is matched against.
    pub label: String,
    /// The signature or type, shown dimmed after the label.
    pub detail: Option<String>,
    /// What the server has to say about it, as markdown.
    ///
    /// Usually absent until [`resolve_params`] has been sent for the item:
    /// a server that put the documentation of a thousand candidates in one
    /// reply would be sending a manual to show four lines of.
    pub documentation: Option<String>,
    /// What sort of thing it is, as a colour.
    pub kind: Option<SyntaxKind>,
    /// And as a picture, where there is one that says more than the colour.
    pub icon: Option<char>,
    /// What goes into the document.
    pub insert: String,
    /// Whether [`Candidate::insert`] is a snippet rather than plain text.
    pub snippet: bool,
    /// What it replaces, where the server said so.
    ///
    /// `None` means the server left it to the client, and the client's
    /// answer is the word the reader is in the middle of.
    pub replace: Option<Span>,
    /// Edits elsewhere in the file that go in with it -- an import for the
    /// name being completed, which is the whole reason this is not just a
    /// string.
    pub extra: Vec<(Span, String)>,
    /// What the server wants it sorted by, when two candidates match a
    /// query equally well.
    pub sort: String,
    /// What the server wants it filtered by, which is not always the label.
    pub filter: String,
    /// The item as it arrived, to send back with `completionItem/resolve`.
    ///
    /// The protocol says to return the item unchanged, and servers put
    /// private state in `data` that they need to see again.
    pub item: Value,
    /// Whether the item has been resolved, so it is not asked about twice.
    pub resolved: bool,
}

/// Everything a server offered, and whether it was the whole of it.
#[derive(Clone, Debug, Default)]
pub struct Offer {
    /// The candidates, in the order the server sent them.
    pub candidates: Vec<Candidate>,
    /// Whether typing another letter needs a fresh question.
    ///
    /// A server that says its list is incomplete has filtered it down to
    /// what it could afford to send, and the next letter changes which
    /// thousand that is. One that says it is complete has sent everything,
    /// and narrowing it is the client's to do -- which is what makes a
    /// completion list survive the reader going on typing.
    pub incomplete: bool,
}

/// Whether the server answers `textDocument/completion` at all.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.completion_provider.is_some()
}

/// Whether typing this character is a question in itself.
///
/// A server says which characters mean "there is something to offer here
/// even though no word has been started": `.` and `::` in Rust, `<` in a
/// template, a quote inside a string. obelus asks about letters by itself
/// and has no idea which punctuation means anything in which language, so
/// this is the server's list or nothing.
///
/// The protocol's trigger characters are single characters, so a sequence
/// like `::` arrives as its last character -- which is why one `:` is
/// enough to ask, and why the first of a pair asks a question the server
/// answers with nothing.
#[must_use]
pub fn triggered_by(capabilities: &ServerCapabilities, character: char) -> bool {
    capabilities
        .completion_provider
        .as_ref()
        .and_then(|provider| provider.trigger_characters.as_ref())
        .is_some_and(|characters| {
            characters
                .iter()
                .any(|trigger| trigger.ends_with(character))
        })
}

/// Whether the server resolves an item's documentation.
#[must_use]
pub fn resolves(capabilities: &ServerCapabilities) -> bool {
    capabilities
        .completion_provider
        .as_ref()
        .and_then(|provider| provider.resolve_provider)
        .unwrap_or(false)
}

/// What a server's answer offers.
///
/// An error, a null, or a list of nothing all come back as nothing to show:
/// there is one thing to do about each of them, and it is to draw no panel.
#[must_use]
pub fn offer_in(
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) -> Offer {
    let Ok(value) = result else {
        return Offer::default();
    };
    let Ok(response) = serde_json::from_value::<Option<CompletionResponse>>(value.clone()) else {
        tracing::debug!("a completion answer in a shape obelus does not know");
        return Offer::default();
    };
    let (items, incomplete) = match response {
        None => return Offer::default(),
        Some(CompletionResponse::Array(items)) => (items, false),
        Some(CompletionResponse::List(list)) => (list.items, list.is_incomplete),
    };
    Offer {
        candidates: items
            .into_iter()
            .map(|item| candidate_of(item, text, encoding))
            .collect(),
        incomplete,
    }
}

/// What to send to `completionItem/resolve` for a candidate.
#[must_use]
pub fn resolve_params(candidate: &Candidate) -> Value {
    candidate.item.clone()
}

/// Takes what a resolve added to an item.
///
/// Only the three fields a resolve is asked for: what it is, what it says,
/// and what else has to be edited for it. The rest of the item is what the
/// original said, because a server is entitled to send back a form with the
/// fields it did not fill in left out.
pub fn resolved_into(
    candidate: &mut Candidate,
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) {
    candidate.resolved = true;
    let Ok(value) = result else {
        return;
    };
    let Ok(item) = serde_json::from_value::<CompletionItem>(value.clone()) else {
        return;
    };
    if let Some(documentation) = documentation_of(item.documentation.as_ref()) {
        candidate.documentation = Some(documentation);
    }
    if let Some(detail) = detail_of(&item) {
        candidate.detail = Some(detail);
    }
    if let Some(edits) = item.additional_text_edits {
        candidate.extra = edits
            .into_iter()
            .map(|edit| (span_of(edit.range, text, encoding), edit.new_text))
            .collect();
    }
}

/// One item, in obelus's own terms.
fn candidate_of(item: CompletionItem, text: &Text, encoding: &PositionEncodingKind) -> Candidate {
    let raw = serde_json::to_value(&item).unwrap_or(Value::Null);
    // What goes in, in the order the protocol says to look: the edit the
    // server wrote, then the text it suggested, then the label itself.
    let (insert, replace) = match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => (
            edit.new_text.clone(),
            Some(span_of(edit.range, text, encoding)),
        ),
        // Two ranges, one of which replaces what is in front of the cursor
        // as well. obelus takes the replacing one: a reader completing in
        // the middle of a word means to replace the word, which is what
        // every editor that offers the choice defaults to.
        Some(CompletionTextEdit::InsertAndReplace(edit)) => (
            edit.new_text.clone(),
            Some(span_of(edit.replace, text, encoding)),
        ),
        None => (
            item.insert_text
                .clone()
                .unwrap_or_else(|| item.label.clone()),
            None,
        ),
    };
    Candidate {
        detail: detail_of(&item),
        documentation: documentation_of(item.documentation.as_ref()),
        kind: item.kind.and_then(kind_of),
        icon: item.kind.and_then(icon_of),
        insert,
        snippet: item.insert_text_format == Some(InsertTextFormat::SNIPPET),
        replace,
        extra: item
            .additional_text_edits
            .unwrap_or_default()
            .into_iter()
            .map(|edit| (span_of(edit.range, text, encoding), edit.new_text))
            .collect(),
        sort: item.sort_text.unwrap_or_else(|| item.label.clone()),
        filter: item.filter_text.unwrap_or_else(|| item.label.clone()),
        label: item.label,
        item: raw,
        resolved: false,
    }
}

/// The signature or type shown after the label.
///
/// `labelDetails.detail` before `detail`: rust-analyzer puts the function's
/// arguments in the first and the module it came from in the second, and the
/// arguments are what a reader is choosing between.
fn detail_of(item: &CompletionItem) -> Option<String> {
    item.label_details
        .as_ref()
        .and_then(|details| details.detail.clone())
        .or_else(|| item.detail.clone())
        .map(|detail| detail.trim().to_string())
        .filter(|detail| !detail.is_empty())
}

/// What the server wrote about an item, as markdown either way.
///
/// Plain text is markdown that happens to have no marks in it, which is
/// true of every plain-text documentation any server sends: they are
/// paragraphs and indented code.
fn documentation_of(documentation: Option<&Documentation>) -> Option<String> {
    let text = match documentation? {
        Documentation::String(text) => text.clone(),
        Documentation::MarkupContent(MarkupContent { value, .. }) => value.clone(),
    };
    (!text.trim().is_empty()).then_some(text)
}

/// The colour a kind of candidate is drawn in.
///
/// The same table the outline uses, for the same reason: a name is coloured
/// by what it names, in the colours the code itself uses, so `push_str` in
/// the list is the colour `push_str` is in the file. Anything with no
/// answer -- a word from the file, a snippet, a file name -- takes the
/// ordinary foreground rather than a colour invented for it.
fn kind_of(kind: CompletionItemKind) -> Option<SyntaxKind> {
    match kind {
        CompletionItemKind::FUNCTION | CompletionItemKind::METHOD => Some(SyntaxKind::Function),
        CompletionItemKind::CONSTRUCTOR => Some(SyntaxKind::Constructor),
        CompletionItemKind::CLASS
        | CompletionItemKind::STRUCT
        | CompletionItemKind::INTERFACE
        | CompletionItemKind::ENUM
        | CompletionItemKind::TYPE_PARAMETER => Some(SyntaxKind::Type),
        CompletionItemKind::FIELD | CompletionItemKind::PROPERTY => Some(SyntaxKind::Property),
        CompletionItemKind::CONSTANT | CompletionItemKind::ENUM_MEMBER => {
            Some(SyntaxKind::Constant)
        }
        CompletionItemKind::VARIABLE => Some(SyntaxKind::Variable),
        CompletionItemKind::KEYWORD => Some(SyntaxKind::Keyword),
        CompletionItemKind::MODULE => Some(SyntaxKind::Keyword),
        _ => None,
    }
}

/// The picture for a kind of candidate.
///
/// Mostly [`crate::icons::for_kind`], because a candidate and an outline
/// row name the same sorts of thing. The exceptions are the kinds the
/// protocol tells apart and a colour cannot: a module and a keyword are
/// both keyword-coloured, and drawing them alike would put the same
/// picture on `mod` and on `pub`. A snippet, a file and a folder have no
/// colour of their own at all, and a picture is the only thing that says
/// what they are.
fn icon_of(kind: CompletionItemKind) -> Option<char> {
    match kind {
        // `md-key`, for the word that is one.
        CompletionItemKind::KEYWORD => Some('\u{f0306}'),
        // `md-code_braces`: a snippet is a piece of code rather than a
        // name.
        CompletionItemKind::SNIPPET => Some('\u{f0169}'),
        // `md-file_document` and `md-folder`, which is what they are.
        CompletionItemKind::FILE => Some('\u{f0219}'),
        CompletionItemKind::FOLDER => Some('\u{f024b}'),
        other => kind_of(other).map(crate::icons::for_kind),
    }
}

/// A protocol range as a span of the document.
fn span_of(range: lsp_types::Range, text: &Text, encoding: &PositionEncodingKind) -> Span {
    let (line, column) = super::position::from_lsp(text, range.start, encoding);
    let (end_line, end_column) = super::position::from_lsp(text, range.end, encoding);
    Span {
        line,
        column,
        end_line,
        end_column,
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::CompletionOptions;

    use super::*;

    /// A module and a keyword are both keyword-coloured, and a snippet is
    /// not coloured at all: what tells them apart in a list is the
    /// picture, so it has to be a different one.
    #[test]
    fn the_kinds_a_colour_cannot_tell_apart_have_their_own_pictures() {
        let module = icon_of(CompletionItemKind::MODULE).expect("a module has a picture");
        let keyword = icon_of(CompletionItemKind::KEYWORD).expect("a keyword has one");
        let snippet = icon_of(CompletionItemKind::SNIPPET).expect("a snippet has one");
        assert_eq!(
            kind_of(CompletionItemKind::MODULE),
            kind_of(CompletionItemKind::KEYWORD),
            "the two this test is about no longer share a colour"
        );
        assert_ne!(module, keyword, "`mod` and `pub` are drawn alike");
        assert_ne!(snippet, keyword);
        assert_ne!(snippet, module);

        // A method is a function wherever it appears, so it wears what the
        // outline gives one rather than a second picture for the same thing.
        assert_eq!(
            icon_of(CompletionItemKind::METHOD),
            Some(crate::icons::for_kind(SyntaxKind::Function))
        );
        // And a kind obelus has nothing to say about wears nothing.
        assert_eq!(icon_of(CompletionItemKind::TEXT), None);
    }

    /// The punctuation that asks a question is the server's to name. A
    /// server that names none is a server obelus only asks about words.
    #[test]
    fn only_the_characters_a_server_named_ask_anything() {
        let mut capabilities = ServerCapabilities::default();
        assert!(!triggered_by(&capabilities, ':'));

        capabilities.completion_provider = Some(CompletionOptions {
            trigger_characters: Some(vec![":".to_string(), ".".to_string()]),
            ..Default::default()
        });
        assert!(triggered_by(&capabilities, ':'));
        assert!(triggered_by(&capabilities, '.'));
        assert!(
            !triggered_by(&capabilities, ';'),
            "a semicolon is the end of a statement, not a question"
        );

        // A trigger the protocol lets a server write as a sequence: what
        // arrives from the keyboard is its last character, one at a time.
        capabilities.completion_provider = Some(CompletionOptions {
            trigger_characters: Some(vec!["->".to_string()]),
            ..Default::default()
        });
        assert!(triggered_by(&capabilities, '>'));
        assert!(
            !triggered_by(&capabilities, '-'),
            "the first half of a sequence is not the question"
        );

        // A server that offers completion and names no triggers still
        // answers about words, and asks nothing of punctuation.
        capabilities.completion_provider = Some(CompletionOptions::default());
        assert!(!triggered_by(&capabilities, '.'));
    }
}
