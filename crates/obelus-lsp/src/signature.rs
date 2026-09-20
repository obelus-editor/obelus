//! The call the cursor is inside, and which argument it is on.
//!
//! What a reader needs while they are typing arguments is one line: what
//! the thing takes, with the one they are on marked. The protocol sends a
//! list of signatures with a list of parameters each, and which of both is
//! active -- and the parameters are given either as a range into the label
//! or as a string to find in it, because the protocol has both shapes.

use lsp_types::{ParameterLabel, ServerCapabilities, SignatureHelp};
use serde_json::Value;

/// One call, as a line to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The whole signature, as the server wrote it.
    pub label: String,
    /// Which characters of it are the argument being typed.
    pub active: Option<(usize, usize)>,
}

/// Whether the server answers `textDocument/signatureHelp`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.signature_help_provider.is_some()
}

/// Whether typing this character asks for one.
///
/// The server's own list: `(` and `,` in most languages, and obelus has no
/// business guessing which punctuation opens a call in fourteen of them.
#[must_use]
pub fn triggered_by(capabilities: &ServerCapabilities, character: char) -> bool {
    capabilities
        .signature_help_provider
        .as_ref()
        .and_then(|provider| provider.trigger_characters.as_ref())
        .is_some_and(|characters| {
            characters
                .iter()
                .any(|trigger| trigger.ends_with(character))
        })
}

/// The signature a server's answer is about, if it is about one.
#[must_use]
pub fn in_reply(result: &Result<Value, String>) -> Option<Signature> {
    let Ok(value) = result else {
        return None;
    };
    let help = serde_json::from_value::<Option<SignatureHelp>>(value.clone()).ok()??;
    let active = help.active_signature.unwrap_or(0) as usize;
    let signature = help
        .signatures
        .get(active)
        .or_else(|| help.signatures.first())?;

    // Which parameter, by the signature's own answer where it gave one and
    // the whole call's otherwise: a server may say it per signature, which
    // is the shape that matters when it sent more than one.
    let at = signature
        .active_parameter
        .or(help.active_parameter)
        .unwrap_or(0) as usize;
    let active = signature
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.get(at))
        .and_then(|parameter| match &parameter.label {
            // Character offsets into the label, which is what obelus
            // wants: the panel draws characters.
            ParameterLabel::LabelOffsets([from, to]) => Some((*from as usize, *to as usize)),
            // A piece of the label, given as text. Found in it, because
            // the protocol says that is what it is.
            ParameterLabel::Simple(text) => {
                let at = signature.label.find(text.as_str())?;
                let from = signature.label[..at].chars().count();
                Some((from, from + text.chars().count()))
            }
        });
    Some(Signature {
        label: signature.label.clone(),
        active,
    })
}
