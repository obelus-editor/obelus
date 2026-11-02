//! The messages, read and checked against each other and made into a
//! function each.
//!
//! The first language is the one the others are measured against: what it
//! has is what there is, and its variables are what a function takes. A
//! language with a message the first does not have, without one the first
//! has, or reaching for a variable nobody will pass, stops the build -- at
//! runtime each of those would be a sentence that quietly came out in the
//! wrong language or with a hole in it, and nothing would say so.
//!
//! Pure, so the tests can hand it text: the build script reads the files and
//! writes what this returns.

use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast;

/// One file of messages, as the build found it.
pub(crate) struct File {
    /// Its name without `.ftl`, which is the module its messages go in.
    pub(crate) stem: String,
    /// Where it is, from the crate's own directory, for `include_str!`.
    pub(crate) path: String,
    /// What it says.
    pub(crate) source: String,
}

/// One language's files.
pub(crate) struct Language {
    /// Its tag, which is also its directory: `en`, `zh-CN`.
    pub(crate) word: String,
    /// Its files, in any order.
    pub(crate) files: Vec<File>,
}

/// One message, as the first language has it.
struct Message {
    /// Its name in the files.
    id: String,
    /// The variables it is given, by name.
    variables: BTreeSet<String>,
    /// What it says, for the function's doc.
    text: String,
}

/// The Rust for every language's messages, or everything wrong with them.
pub(crate) fn generate(languages: &[Language]) -> Result<String, Vec<String>> {
    let mut wrong = Vec::new();
    let Some(first) = languages.first() else {
        return Err(vec!["no languages".to_string()]);
    };
    // Every language parsed, by file stem: the messages each file has, with
    // their variables.
    let mut read: Vec<BTreeMap<String, Vec<Message>>> = Vec::new();
    for language in languages {
        let mut files = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for file in &language.files {
            let place = format!("{}/{}.ftl", language.word, file.stem);
            if !is_a_name(&file.stem) {
                wrong.push(format!(
                    "{place}: a file's name has to be a word in lowercase and hyphens"
                ));
            }
            let messages = messages(&place, &file.source, &mut wrong);
            for message in &messages {
                if !seen.insert(message.id.clone()) {
                    wrong.push(format!(
                        "{place}: {} is said twice in {}",
                        message.id, language.word
                    ));
                }
            }
            files.insert(file.stem.clone(), messages);
        }
        read.push(files);
    }
    let Some((model, others)) = read.split_first() else {
        return Err(wrong);
    };
    for (language, files) in languages.iter().skip(1).zip(others) {
        let word = &language.word;
        for stem in model.keys().filter(|stem| !files.contains_key(*stem)) {
            wrong.push(format!(
                "{word}/{stem}.ftl: missing, and {} has it",
                first.word
            ));
        }
        for stem in files.keys().filter(|stem| !model.contains_key(*stem)) {
            wrong.push(format!(
                "{word}/{stem}.ftl: {} has no such file",
                first.word
            ));
        }
        for (stem, theirs) in files {
            let Some(ours) = model.get(stem) else {
                continue;
            };
            let place = format!("{word}/{stem}.ftl");
            for message in ours {
                if !theirs.iter().any(|their| their.id == message.id) {
                    wrong.push(format!("{place}: {} is missing", message.id));
                }
            }
            for their in theirs {
                let Some(message) = ours.iter().find(|message| message.id == their.id) else {
                    wrong.push(format!("{place}: {} is not in {}", their.id, first.word));
                    continue;
                };
                for variable in their.variables.difference(&message.variables) {
                    wrong.push(format!(
                        "{place}: {} uses ${variable}, which {} does not pass",
                        their.id, first.word
                    ));
                }
            }
        }
    }
    if !wrong.is_empty() {
        return Err(wrong);
    }
    Ok(written(languages, model))
}

/// A file's messages, and whatever will not parse in it put in `wrong`.
fn messages(place: &str, source: &str, wrong: &mut Vec<String>) -> Vec<Message> {
    let resource = match fluent_syntax::parser::parse(source) {
        Ok(resource) => resource,
        Err((resource, errors)) => {
            for error in errors {
                let line = source[..error.pos.start.min(source.len())]
                    .lines()
                    .count()
                    .max(1);
                wrong.push(format!("{place}:{line}: {}", error.kind));
            }
            resource
        }
    };
    let found: BTreeMap<&str, &ast::Message<&str>> = resource
        .body
        .iter()
        .filter_map(|entry| match entry {
            ast::Entry::Message(message) => Some((message.id.name, message)),
            _ => None,
        })
        .collect();
    let mut messages = Vec::new();
    for message in found.values() {
        let id = message.id.name;
        if !is_a_name(id) {
            wrong.push(format!(
                "{place}: {id} has to be a word in lowercase and hyphens"
            ));
        }
        let Some(value) = &message.value else {
            wrong.push(format!("{place}: {id} has no value of its own"));
            continue;
        };
        let mut variables = BTreeSet::new();
        variables_in(value, &found, &mut BTreeSet::from([id]), &mut variables);
        for variable in &variables {
            if !is_a_name(variable) {
                wrong.push(format!(
                    "{place}: ${variable} in {id} has to be a word in lowercase and hyphens"
                ));
            }
        }
        messages.push(Message {
            id: id.to_string(),
            variables,
            text: text_of(value),
        });
    }
    messages
}

/// Every variable a pattern reaches for, through the messages it names.
///
/// Not through terms: a term's variables are the arguments the reference
/// hands it, not anything the caller passes.
fn variables_in<'s>(
    pattern: &ast::Pattern<&'s str>,
    messages: &BTreeMap<&'s str, &ast::Message<&'s str>>,
    visiting: &mut BTreeSet<&'s str>,
    found: &mut BTreeSet<String>,
) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            in_expression(expression, messages, visiting, found);
        }
    }
}

fn in_expression<'s>(
    expression: &ast::Expression<&'s str>,
    messages: &BTreeMap<&'s str, &ast::Message<&'s str>>,
    visiting: &mut BTreeSet<&'s str>,
    found: &mut BTreeSet<String>,
) {
    match expression {
        ast::Expression::Select { selector, variants } => {
            in_inline(selector, messages, visiting, found);
            for variant in variants {
                variables_in(&variant.value, messages, visiting, found);
            }
        }
        ast::Expression::Inline(inline) => in_inline(inline, messages, visiting, found),
    }
}

fn in_inline<'s>(
    inline: &ast::InlineExpression<&'s str>,
    messages: &BTreeMap<&'s str, &ast::Message<&'s str>>,
    visiting: &mut BTreeSet<&'s str>,
    found: &mut BTreeSet<String>,
) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            found.insert(id.name.to_string());
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => {
            for argument in &arguments.positional {
                in_inline(argument, messages, visiting, found);
            }
            for argument in &arguments.named {
                in_inline(&argument.value, messages, visiting, found);
            }
        }
        ast::InlineExpression::MessageReference { id, .. } => {
            // A message naming itself, however far round, is Fluent's to
            // refuse when it is formatted; here it only has to end.
            if let Some(message) = messages.get(id.name)
                && let Some(value) = &message.value
                && visiting.insert(id.name)
            {
                variables_in(value, messages, visiting, found);
                visiting.remove(id.name);
            }
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for argument in &arguments.named {
                in_inline(&argument.value, messages, visiting, found);
            }
        }
        ast::InlineExpression::Placeable { expression } => {
            in_expression(expression, messages, visiting, found);
        }
        ast::InlineExpression::TermReference {
            arguments: None, ..
        }
        | ast::InlineExpression::StringLiteral { .. }
        | ast::InlineExpression::NumberLiteral { .. } => {}
    }
}

/// What a pattern says, roughly, for a doc comment: its text, with each
/// placeable as `{…}`.
fn text_of(pattern: &ast::Pattern<&str>) -> String {
    let mut text = String::new();
    for element in &pattern.elements {
        match element {
            ast::PatternElement::TextElement { value } => text.push_str(value),
            ast::PatternElement::Placeable { expression } => match expression {
                ast::Expression::Inline(ast::InlineExpression::VariableReference { id }) => {
                    text.push_str(&format!("{{${}}}", id.name));
                }
                _ => text.push_str("{\u{2026}}"),
            },
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a name can become a Rust one by swapping its hyphens.
fn is_a_name(name: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else",
        "enum", "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match",
        "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
        "true", "try", "type", "unsafe", "use", "where", "while", "yield",
    ];
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !name.ends_with('-')
        && !name.contains("--")
        && !KEYWORDS.contains(&snake(name).as_str())
}

/// A Fluent name as a Rust one.
fn snake(name: &str) -> String {
    name.replace('-', "_")
}

/// The Rust, once everything has been checked.
fn written(languages: &[Language], model: &BTreeMap<String, Vec<Message>>) -> String {
    let mut out = String::new();
    out.push_str("// Written by build.rs from locales/. Not to be edited.\n\n");
    out.push_str(&format!(
        "/// Every language the messages are written in, in the order the catalogue keeps them.\n\
         pub(crate) const WORDS: [&str; {}] = [{}];\n\n",
        languages.len(),
        languages
            .iter()
            .map(|language| format!("{:?}", language.word))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str(&format!(
        "/// Each language's files, in the order of [`WORDS`].\n\
         pub(crate) const SOURCES: [&[&str]; {}] = [\n",
        languages.len()
    ));
    for language in languages {
        out.push_str("    &[\n");
        let mut files: Vec<&File> = language.files.iter().collect();
        files.sort_by(|one, other| one.stem.cmp(&other.stem));
        for file in files {
            out.push_str(&format!(
                "        include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?})),\n",
                format!("/{}", file.path)
            ));
        }
        out.push_str("    ],\n");
    }
    out.push_str("];\n\n");
    let plain: Vec<&Message> = model
        .values()
        .flatten()
        .filter(|message| message.variables.is_empty())
        .collect();
    out.push_str(&format!(
        "/// The messages that take nothing, which are said once per language and kept.\n\
         pub(crate) const PLAIN: [&str; {}] = [{}];\n",
        plain.len(),
        plain
            .iter()
            .map(|message| format!("{:?}", message.id))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for (stem, messages) in model {
        out.push_str(&format!(
            "\n/// What `{stem}.ftl` says.\npub mod {} {{\n",
            snake(stem)
        ));
        for message in messages {
            let doc = format!("`{}`: {}", message.id, message.text);
            out.push_str(&format!("    #[doc = {doc:?}]\n    #[must_use]\n"));
            let name = snake(&message.id);
            if message.variables.is_empty() {
                let index = plain
                    .iter()
                    .position(|plain| plain.id == message.id)
                    .unwrap_or_default();
                out.push_str(&format!(
                    "    pub fn {name}() -> &'static str {{\n        crate::plain({index})\n    }}\n"
                ));
            } else {
                let parameters = message
                    .variables
                    .iter()
                    .map(|variable| format!("{}: impl Into<crate::Arg>", snake(variable)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let arguments = message
                    .variables
                    .iter()
                    .map(|variable| format!("({variable:?}, {}.into())", snake(variable)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "    pub fn {name}({parameters}) -> String {{\n        crate::formatted({:?}, [{arguments}])\n    }}\n",
                    message.id
                ));
            }
        }
        out.push_str("}\n");
    }
    out
}
