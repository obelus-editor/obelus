//! The call the cursor is inside, and which argument it is on.
//!
//! What a reader needs while they are typing arguments is one line: what
//! the thing takes, with the one they are on marked. The protocol sends a
//! list of signatures with a list of parameters each, and which of both is
//! active -- and the parameters are given either as a range into the label
//! or as a string to find in it, because the protocol has both shapes.
//!
//! Every signature it sent is kept, the active one first. A name with
//! several is a name the reader has to choose between, and a panel showing
//! one of them says there is one; the active one goes first because what is
//! drawn is capped, and the call they are in is the one that has to be on
//! screen.
//!
//! **A parameter nothing is on is not the first parameter.** The protocol
//! lets both `activeParameter` fields be `null`, and says in as many words
//! that a `null` is "no parameter is active" while an absent one falls
//! through to the other -- two things `Option<u32>` cannot tell apart, so
//! that one question is asked of the answer as it arrived. Marking the
//! first argument of a call whose cursor is past the last one is the panel
//! saying something false about the one thing it is for.

use lsp_types::{
    Documentation, MarkupContent, ParameterLabel, ServerCapabilities, SignatureHelp,
    SignatureInformation,
};
use serde_json::Value;

/// One of the calls a name could be, as a line to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The whole signature, as the server wrote it.
    pub label: String,
    /// Which characters of it are the argument being typed.
    ///
    /// Only the active signature has one: a parameter of a signature the
    /// cursor is not in is not being typed into.
    pub active: Option<(usize, usize)>,
}

/// What a server said the call the cursor is inside takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// Every signature it sent, the one the cursor is in first.
    pub signatures: Vec<Signature>,
    /// What the active parameter says about itself, as markdown.
    ///
    /// The parameter's own where it has one and the signature's otherwise:
    /// the reader is filling in an argument, so what that argument is for
    /// is the nearer answer, and the signature's is what is left to say
    /// when the parameter says nothing.
    pub documentation: Option<String>,
    /// The answer as it arrived.
    ///
    /// Handed back as `activeSignatureHelp` when the next question is a
    /// retrigger, which is what the protocol asks a client to do: a server
    /// told which signature the reader is looking at can keep them on it
    /// rather than choosing again.
    pub said: Value,
}

/// Whether the server answers `textDocument/signatureHelp`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.signature_help_provider.is_some()
}

/// Whether typing this character asks for one.
///
/// The server's own list: `(` and `,` in most languages, and Obelus has no
/// business guessing which punctuation opens a call in fourteen of them.
#[must_use]
pub fn triggered_by(capabilities: &ServerCapabilities, character: char) -> bool {
    listed(
        capabilities
            .signature_help_provider
            .as_ref()
            .and_then(|provider| provider.trigger_characters.as_ref()),
        character,
    )
}

/// Whether typing this character asks again, while one is showing.
///
/// A second list, and it means something else. rust-analyzer's is `)`,
/// which is not "the call is over": the call it closes may be an argument
/// of another one, so the answer is a fresh question about whichever call
/// the cursor is in now. A client that treated it as an ending would close
/// the panel on `f(g(), |` -- where the reader is still inside `f`.
#[must_use]
pub fn retriggered_by(capabilities: &ServerCapabilities, character: char) -> bool {
    listed(
        capabilities
            .signature_help_provider
            .as_ref()
            .and_then(|provider| provider.retrigger_characters.as_ref()),
        character,
    )
}

/// Whether a character is in one of those lists.
///
/// `ends_with`, because a trigger may be several characters (`::`) and what
/// arrived is the last of them.
fn listed(characters: Option<&Vec<String>>, character: char) -> bool {
    characters.is_some_and(|characters| {
        characters
            .iter()
            .any(|trigger| trigger.ends_with(character))
    })
}

/// Why the question is being asked.
///
/// The protocol's trigger kinds, named. They are not the same thing to a
/// server: a character it asked to hear about is a call being written, and
/// the document changing under the caret is a client re-asking because what
/// is under the caret is not what it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// One of the server's own trigger characters was typed.
    Typed(char),
    /// The document moved under the caret -- a candidate that put a call in.
    Changed,
}

/// Why the question is being asked, for the server to read.
///
/// The protocol's `SignatureHelpContext`, which a client sends once it has
/// said it can (`contextSupport`). Without it a server cannot tell a fresh
/// call from the reader typing the next comma of one, which is the
/// difference between choosing a signature and keeping the one they are
/// looking at.
#[must_use]
pub fn context(asked: Asked, showing: Option<&Answer>) -> Value {
    let mut context = match asked {
        Asked::Typed(trigger) => serde_json::json!({
            "triggerKind": 2,
            "triggerCharacter": trigger.to_string(),
            "isRetrigger": showing.is_some(),
        }),
        Asked::Changed => serde_json::json!({
            "triggerKind": 3,
            "isRetrigger": showing.is_some(),
        }),
    };
    if let Some(showing) = showing {
        context["activeSignatureHelp"] = showing.said.clone();
    }
    context
}

/// The call a server's answer is about, if it is about one.
#[must_use]
pub fn in_reply(result: &Result<Value, String>) -> Option<Answer> {
    let Ok(said) = result else {
        return None;
    };
    let help = serde_json::from_value::<Option<SignatureHelp>>(said.clone()).ok()??;

    // Which signature, clamped: an index past the end is a server's
    // mistake, and the protocol says the first one is what it means.
    let which = help.active_signature.unwrap_or(0) as usize;
    let which = match which < help.signatures.len() {
        true => which,
        false => 0,
    };
    let signature = help.signatures.get(which)?;

    let at = active_parameter(said, which, &help);
    let signatures = std::iter::once(Signature {
        label: signature.label.clone(),
        active: at.and_then(|at| marked(signature, at)),
    })
    // The rest in the order they arrived, less the one already at the
    // front.
    .chain(
        help.signatures
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != which)
            .map(|(_, other)| Signature {
                label: other.label.clone(),
                active: None,
            }),
    )
    .collect();

    Some(Answer {
        signatures,
        documentation: documentation(signature, at),
        said: said.clone(),
    })
}

/// Which parameter is being typed, where there is one.
///
/// Read off the answer as it arrived, because this is the one question the
/// typed shape cannot answer: `activeParameter` may be a number, may be
/// `null`, and may be absent, and the protocol gives those three different
/// meanings while `Option<u32>` has two.
fn active_parameter(said: &Value, which: usize, help: &SignatureHelp) -> Option<usize> {
    let theirs = said
        .get("signatures")
        .and_then(|signatures| signatures.get(which))
        .and_then(|signature| signature.get("activeParameter"));
    // The signature's own answer, where it gave one: a number is that
    // parameter and `null` is none of them.
    if let Some(value) = theirs {
        return match value.is_null() {
            true => None,
            false => help
                .signatures
                .get(which)?
                .active_parameter
                .map(|at| at as usize),
        };
    }
    match said.get("activeParameter") {
        Some(value) if value.is_null() => None,
        // Absent means nought, which the protocol says in as many words.
        _ => Some(help.active_parameter.unwrap_or(0) as usize),
    }
}

/// Which characters of the label a parameter is.
fn marked(signature: &SignatureInformation, at: usize) -> Option<(usize, usize)> {
    match &signature.parameters.as_ref()?.get(at)?.label {
        // Offsets into the label -- counted in UTF-16, which the protocol
        // says in as many words and which `as usize` quietly reads as a
        // count of characters. The two agree on everything in the basic
        // multilingual plane and part company on the first emoji, and what
        // they would mark then is the wrong half of the label. The panel
        // draws characters, so they are converted to characters here.
        ParameterLabel::LabelOffsets([from, to]) => Some((
            obelus_text::characters_at_utf16(&signature.label, *from as usize),
            obelus_text::characters_at_utf16(&signature.label, *to as usize),
        )),
        // A piece of the label, given as text. Found in it, because
        // the protocol says that is what it is.
        ParameterLabel::Simple(text) => {
            let at = signature.label.find(text.as_str())?;
            let from = signature.label[..at].chars().count();
            Some((from, from + text.chars().count()))
        }
    }
}

/// What the call says about itself, if it says anything.
///
/// The parameter being typed first: a reader in the middle of an argument
/// list is asking what *this* argument is, and the signature's own
/// documentation is the answer to a question they have already had
/// answered by the label above it.
fn documentation(signature: &SignatureInformation, at: Option<usize>) -> Option<String> {
    at.and_then(|at| {
        signature
            .parameters
            .as_ref()?
            .get(at)?
            .documentation
            .as_ref()
    })
    .and_then(prose)
    .or_else(|| signature.documentation.as_ref().and_then(prose))
}

/// One of those, as the markdown it is.
///
/// Nothing for one that is blank: a rule with nothing under it is a panel
/// saying there was something to say.
fn prose(documentation: &Documentation) -> Option<String> {
    let said = match documentation {
        Documentation::String(text) => text,
        Documentation::MarkupContent(MarkupContent { value, .. }) => value,
    };
    match said.trim().is_empty() {
        true => None,
        false => Some(said.clone()),
    }
}
