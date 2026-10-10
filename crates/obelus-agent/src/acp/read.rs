//! What the agent sends, read into what Obelus says about it.
//!
//! The protocol's shapes go in and [`super::said`]'s come out: a
//! `session/update`, a permission asked for, a form for the reader to fill
//! in, a setting, a tool call. Reading only -- what goes back out is built
//! where it is sent, in [`super::link`] -- and nothing here waits on
//! anything, so all of it is plain functions over values.

use agent_client_protocol::schema::v1::{
    AvailableCommand, ContentBlock, ElicitationPropertySchema, ElicitationSchema, MultiSelectItems,
    NewSessionResponse, RequestPermissionRequest, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOption, SessionConfigSelectOptions,
    SessionModeState, SessionUpdate, ToolCallContent, ToolCallId, ToolCallLocation,
    ToolCallUpdateFields,
};

use super::said::{
    Call, Category, Change, Cost, Field, Kind, MODE, OFF, ON, Order, Place, Setting, Step, Takes,
    Update, Usage, Value,
};

/// The protocol's own word for one of its enums.
///
/// Through serde, which is the only thing that knows: these are
/// `snake_case` on the wire and `CamelCase` in Rust, and Obelus used to
/// bridge them with `{:?}` lowercased. That gives `inprogress` for
/// `InProgress` and `switchmode` for `SwitchMode` -- so every arm in
/// Obelus written against the protocol's spelling was an arm nothing could
/// reach: the glyph that says a call is running, the rule that keeps its
/// lines open while it runs, the picture on a plan being approved.
/// Somebody had already met it and papered over it by matching both
/// spellings of one word.
///
/// Derived rather than written out, so a variant Obelus has never seen
/// still comes out as whatever the wire calls it.
pub(crate) fn said_as(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What the agent is actually about to do, for the reader deciding whether
/// to let it.
///
/// The title is a line -- "run a command", "edit a file" -- and a line is
/// not enough to answer a question about permission: *which* command, on
/// *which* file. The protocol carries that as the tool call's content, so
/// this is the words of it, and the files it names when it has no words.
///
/// Not `raw_input`: that is the agent's own arguments in its own shape,
/// which Obelus would have to guess the meaning of. The typed fields are
/// what an agent fills in to be shown.
pub(crate) fn reason_of(request: &RequestPermissionRequest) -> Option<String> {
    let fields = &request.tool_call.fields;
    let said: Vec<String> = fields
        .content
        .iter()
        .flatten()
        .filter_map(|content| match content {
            ToolCallContent::Content(block) => words(&block.content),
            // A diff and a terminal are shown by the conversation itself
            // once the work is allowed; what the question needs is the
            // file it is about, which the locations carry.
            _ => None,
        })
        .collect();
    if !said.is_empty() {
        return Some(said.join("\n"));
    }
    let places: Vec<String> = fields
        .locations
        .iter()
        .flatten()
        .map(|place| place.path.display().to_string())
        .collect();
    (!places.is_empty()).then(|| places.join("\n"))
}

/// What a `session/update` means, if it is one Obelus shows.
///
/// The protocol has a dozen and a half kinds and this reads nine. The rest
/// -- usage, compaction -- are facts about the agent rather than about the
/// conversation, and a conversation with them in it is a log.
///
/// A plan was in that list once and does not belong in it: what an agent
/// means to do about what the reader just asked is the most conversation-
/// shaped thing the protocol carries. The objection was right about where
/// it goes, though -- a finished list of seven completed steps *is* a log,
/// so it is never written into the transcript. It is what is happening
/// now, and it is drawn where that is drawn.
pub(crate) fn read_update(update: SessionUpdate, dialect: super::tasks::Dialect) -> Vec<Update> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => words(&chunk.content)
            .map(Update::Said)
            .into_iter()
            .collect(),
        SessionUpdate::UserMessageChunk(chunk) => words(&chunk.content)
            .map(Update::Heard)
            .into_iter()
            .collect(),
        SessionUpdate::AgentThoughtChunk(chunk) => words(&chunk.content)
            .map(Update::Thought)
            .into_iter()
            .collect(),
        SessionUpdate::ToolCall(call) => vec![Update::Tool {
            call: Box::new(Call {
                id: call.tool_call_id.0.to_string(),
                title: call.title.clone(),
                kind: said_as(&call.kind),
                places: call.locations.iter().map(place_of).collect(),
                change: change_of(&call.content),
                ran: ran_in(&call.content),
                said: words_of(&call.content),
                backgrounded: dialect.backgrounded(call.meta.as_ref()),
            }),
            status: said_as(&call.status),
        }],
        // A later update carries only what changed, so what it leaves out
        // arrives here as nothing and is read as "the same as before".
        SessionUpdate::ToolCallUpdate(call) => vec![Update::Tool {
            call: Box::new(Call {
                backgrounded: dialect.backgrounded(call.meta.as_ref()),
                ..call_of(&call.tool_call_id, &call.fields)
            }),
            status: call
                .fields
                .status
                .map(|status| said_as(&status))
                .unwrap_or_default(),
        }],
        SessionUpdate::Plan(plan) => vec![Update::Plan(
            plan.entries
                .iter()
                .map(|entry| Step {
                    said: entry.content.clone(),
                    state: said_as(&entry.status),
                    priority: said_as(&entry.priority),
                })
                .collect(),
        )],
        SessionUpdate::CurrentModeUpdate(mode) => {
            vec![Update::Mode(mode.current_mode_id.0.to_string())]
        }
        SessionUpdate::AvailableCommandsUpdate(update) => vec![Update::Orders(
            update.available_commands.iter().map(order_of).collect(),
        )],
        SessionUpdate::ConfigOptionUpdate(update) => vec![Update::Settings(
            update
                .config_options
                .iter()
                .filter_map(setting_of)
                .collect(),
        )],
        // What the agent calls this conversation. A patch rather than a
        // value: absent means unchanged, null means cleared, and only a
        // string is a new name -- so the two that are not a string are
        // nothing to do, not a name of nothing.
        // How full it is, and what it has cost. Several of these arrive in
        // one turn -- the numbers only go up within a turn -- so this is
        // kept and shown rather than said.
        SessionUpdate::UsageUpdate(used) => vec![Update::Used(Usage {
            used: used.used,
            room: used.size,
            cost: used.cost.map(|cost| Cost {
                amount: cost.amount,
                currency: cost.currency,
            }),
        })],
        SessionUpdate::SessionInfoUpdate(info) => match info.title {
            agent_client_protocol::schema::MaybeUndefined::Value(title) => {
                vec![Update::Titled(title)]
            }
            agent_client_protocol::schema::MaybeUndefined::Undefined
            | agent_client_protocol::schema::MaybeUndefined::Null => Vec::new(),
        },
        other => {
            tracing::debug!(?other, "an update Obelus does not show");
            Vec::new()
        }
    }
}

/// A URL Obelus is willing to hand to the machine, or nothing.
///
/// `http` and `https` only, and it must name a host. Everything else is
/// refused, because what happens next is that Obelus asks the machine to
/// open this with whatever is registered for it: `file:` reaches the disk,
/// and an editor or a chat program registering a scheme of its own turns a
/// link into a way to start a program. The string came from the agent.
///
/// Parsed by hand rather than with a URL crate. What is being asked is
/// which scheme it is and whether anything follows -- not what the host
/// normalises to -- and a crate that answers the second brings a Unicode
/// database to do it.
pub(crate) fn somewhere_to_go(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    // A host is whatever comes before the path, and it has to be
    // something: `https:///whatever` names no machine.
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    // And nothing a shell or a launcher would read as more than one
    // argument. Every one of these is legal in a URL only when it is
    // written `%20`, `%0a` and so on, so refusing them refuses nothing a
    // well-formed URL needed.
    if url.chars().any(char::is_whitespace) || url.contains('\0') {
        return None;
    }
    Some(url.to_string())
}

/// The fields of a form, in the order Obelus will put them.
///
/// Or why it cannot put this one. A form Obelus half-fills in is worse than
/// one it declines: the agent gets an answer to a question it did not ask.
///
/// The order is the schema's map order, which is alphabetical by name --
/// the wire has an object and objects have no order, so there is nothing
/// else to go on.
pub(crate) fn fields_of(schema: &ElicitationSchema) -> Result<Vec<Field>, String> {
    let required: Vec<&str> = schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    let mut fields = Vec::new();
    for (name, property) in &schema.properties {
        let (title, about, takes) = match property {
            ElicitationPropertySchema::String(text) => (
                text.title.clone(),
                text.description.clone(),
                match (text.one_of.as_ref(), text.enum_values.as_ref()) {
                    // Named values: the agent gave each one a title, and
                    // that is what the row says.
                    (Some(named), _) => Takes::One(
                        named
                            .iter()
                            .map(|option| Value {
                                id: option.value.clone(),
                                name: option.title.clone(),
                                about: said_twice(option.description.as_deref(), &option.title),
                            })
                            .collect(),
                    ),
                    // Bare values, which are their own names.
                    (None, Some(values)) => Takes::One(
                        values
                            .iter()
                            .map(|value| Value {
                                id: value.clone(),
                                name: value.clone(),
                                about: None,
                            })
                            .collect(),
                    ),
                    (None, None) => Takes::Words(text.default.clone()),
                },
            ),
            ElicitationPropertySchema::Boolean(switch) => (
                switch.title.clone(),
                switch.description.clone(),
                Takes::Switch(switch.default.unwrap_or(false)),
            ),
            ElicitationPropertySchema::Number(number) => (
                number.title.clone(),
                number.description.clone(),
                Takes::Number {
                    whole: false,
                    least: number.minimum,
                    most: number.maximum,
                },
            ),
            ElicitationPropertySchema::Integer(number) => (
                number.title.clone(),
                number.description.clone(),
                Takes::Number {
                    whole: true,
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a bound a reader is expected to type by hand"
                    )]
                    least: number.minimum.map(|least| least as f64),
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a bound a reader is expected to type by hand"
                    )]
                    most: number.maximum.map(|most| most as f64),
                },
            ),
            // Several of a list. The items come either as bare strings or
            // as titled options, which is the same pair the single-select
            // kind comes in and is read the same way.
            ElicitationPropertySchema::Array(several) => (
                several.title.clone(),
                several.description.clone(),
                Takes::Some {
                    values: match &several.items {
                        MultiSelectItems::Titled(items) => items
                            .options
                            .iter()
                            .map(|option| Value {
                                id: option.value.clone(),
                                name: option.title.clone(),
                                about: said_twice(option.description.as_deref(), &option.title),
                            })
                            .collect(),
                        MultiSelectItems::String(items) => items
                            .values
                            .iter()
                            .map(|value| Value {
                                id: value.clone(),
                                name: value.clone(),
                                about: None,
                            })
                            .collect(),
                        other => return Err(format!("{name} is a list of {other:?}")),
                    },
                    least: several.min_items,
                    most: several.max_items,
                    chosen: several.default.clone().unwrap_or_default(),
                },
            ),
            other => return Err(format!("{name} is a {other:?}")),
        };
        fields.push(Field {
            name: name.clone(),
            title: title.unwrap_or_else(|| name.clone()),
            about,
            takes,
            required: required.contains(&name.as_str()),
        });
    }
    // The ones that have to be answered first, in the order the agent
    // listed them; the rest after, in the only order left.
    //
    // The schema's properties arrive as a sorted map -- JSON objects have
    // no order to keep -- so the order the agent wrote them in is gone by
    // the time Obelus sees it, and asking by the alphabet put "Other" in
    // front of the question it was an alternative to. What is left is what
    // the agent said had to be answered, which is the question itself.
    fields.sort_by_key(
        |field| match required.iter().position(|name| *name == field.name) {
            Some(at) => (0, at),
            None => (1, 0),
        },
    );
    Ok(fields)
}

/// One setting, as the view offers it -- if it is one Obelus can show.
///
/// Nothing but a kind it has never heard of is dropped: a setting whose
/// values Obelus cannot list is a row that would do nothing when chosen,
/// and the agent's own dialog for it is not Obelus's to open.
pub(crate) fn setting_of(option: &SessionConfigOption) -> Option<Setting> {
    let (values, current, kind) = match &option.kind {
        SessionConfigKind::Select(select) => (
            values_of(&select.options),
            select.current_value.0.to_string(),
            Kind::Select,
        ),
        SessionConfigKind::Boolean(boolean) => (
            vec![
                Value {
                    id: ON.to_string(),
                    name: ON.to_string(),
                    about: None,
                },
                Value {
                    id: OFF.to_string(),
                    name: OFF.to_string(),
                    about: None,
                },
            ],
            match boolean.current_value {
                true => ON.to_string(),
                false => OFF.to_string(),
            },
            Kind::Switch,
        ),
        other => {
            tracing::debug!(?other, "a setting Obelus cannot show");
            return None;
        }
    };
    // What each one is, once, where it arrives. How an agent declares a
    // setting decides how Obelus draws it and what pressing enter on it
    // does -- a switch is flipped and a list is opened -- so "why is this
    // one drawn like that" is a question about this line, and it was
    // unanswerable without it.
    let setting = Setting {
        id: option.id.0.to_string(),
        name: option.name.clone(),
        about: said_twice(option.description.as_deref(), &option.name),
        values,
        current,
        kind,
        category: category_of(option.category.as_ref()),
        legacy: false,
    };
    tracing::debug!(
        id = setting.id,
        name = setting.name,
        kind = ?setting.kind,
        category = ?setting.category,
        values = setting.values.len(),
        current = setting.current,
        "a setting the agent offers"
    );
    Some(setting)
}

/// What the agent said a setting is about, as one of the few Obelus can do
/// something with.
///
/// A category Obelus has never heard of is [`Category::Other`], which is
/// also what nothing said means: the spec reserves the unprefixed names for
/// itself and tells clients to handle the rest gracefully, and the graceful
/// thing is to show the setting and claim nothing about it.
fn category_of(category: Option<&SessionConfigOptionCategory>) -> Category {
    match category {
        Some(SessionConfigOptionCategory::Mode) => Category::Mode,
        Some(SessionConfigOptionCategory::Model) => Category::Model,
        Some(SessionConfigOptionCategory::ModelConfig) => Category::ModelConfig,
        Some(SessionConfigOptionCategory::ThoughtLevel) => Category::ThoughtLevel,
        Some(SessionConfigOptionCategory::Other(name)) => {
            tracing::debug!(name, "a category Obelus has never heard of");
            Category::Other
        }
        None | Some(_) => Category::Other,
    }
}

/// Everything a new conversation says it can be set to, as one list.
///
/// The same two halves a session keeps apart and merges -- the mode from
/// the older dedicated methods, and the config options -- put together by
/// the same rule: the options win where they carry a mode themselves, and
/// the old one goes in front of them where they do not. Written here as
/// well as there because this list has no session behind it to do it.
pub(crate) fn offers_in(opened: &NewSessionResponse) -> Vec<Setting> {
    let options: Vec<Setting> = opened
        .config_options
        .as_ref()
        .map(|options| options.iter().filter_map(setting_of).collect())
        .unwrap_or_default();
    let carried = options
        .iter()
        .any(|option| option.category == Category::Mode);
    let mut offers = Vec::with_capacity(options.len() + 1);
    offers.extend(opened.modes.as_ref().map(mode_setting).filter(|_| !carried));
    offers.extend(options);
    offers
}

/// The mode an agent offers through the dedicated methods, as a setting
/// like any other.
///
/// The protocol is dropping those methods: "Dedicated session mode methods
/// will be removed in a future version of the protocol", and the option
/// with `category: "mode"` is what replaces them -- so an agent in the
/// middle of that change offers both, to be understood by clients on either
/// side of it. Obelus reads the old shape into the new one here, at the
/// edge, so that everything above this has one kind of thing to draw, walk
/// and set. What is left of the old way is [`Setting::legacy`] and the one
/// branch that reads it.
pub(crate) fn mode_setting(state: &SessionModeState) -> Setting {
    Setting {
        id: MODE.to_string(),
        name: "Mode".to_string(),
        about: None,
        values: state
            .available_modes
            .iter()
            .map(|mode| Value {
                id: mode.id.0.to_string(),
                name: mode.name.clone(),
                about: said_twice(mode.description.as_deref(), &mode.name),
            })
            .collect(),
        current: state.current_mode_id.0.to_string(),
        kind: Kind::Select,
        category: Category::Mode,
        legacy: true,
    }
}

/// The values of a selector, as one list.
fn values_of(options: &SessionConfigSelectOptions) -> Vec<Value> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values.iter().map(value_of).collect(),
        // Flattened, with each group's name kept on its rows. A list of
        // rows that can be chosen and headers that cannot would be a list
        // where the arrows sometimes land on nothing.
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|option| {
                    let value = value_of(option);
                    Value {
                        about: Some(match value.about {
                            Some(about) => format!("{} \u{b7} {about}", group.name),
                            None => group.name.clone(),
                        }),
                        ..value
                    }
                })
            })
            .collect(),
        other => {
            tracing::debug!(?other, "values Obelus cannot list");
            Vec::new()
        }
    }
}

/// One value of a selector.
fn value_of(option: &SessionConfigSelectOption) -> Value {
    Value {
        id: option.value.0.to_string(),
        name: option.name.clone(),
        about: said_twice(option.description.as_deref(), &option.name),
    }
}

/// A description, unless it is the name again.
///
/// Agents fill both in for every row whether they have anything to add or
/// not -- Copilot's model list describes "GPT-5.4" as "GPT-5.4" -- and a row
/// that says the same thing twice reads as a mistake in Obelus.
fn said_twice(about: Option<&str>, name: &str) -> Option<String> {
    about
        .filter(|about| about.trim() != name.trim())
        .map(str::to_string)
}

/// A call, from the fields an update carries.
pub(crate) fn call_of(id: &ToolCallId, fields: &ToolCallUpdateFields) -> Call {
    Call {
        id: id.0.to_string(),
        title: fields.title.clone().unwrap_or_default(),
        kind: fields.kind.map(|kind| said_as(&kind)).unwrap_or_default(),
        places: fields
            .locations
            .clone()
            .map(|places| places.iter().map(place_of).collect())
            .unwrap_or_default(),
        change: fields
            .content
            .clone()
            .and_then(|content| change_of(&content)),
        ran: fields.content.clone().and_then(|content| ran_in(&content)),
        said: fields.content.as_deref().map(words_of).unwrap_or_default(),
        // Asked of the update's `_meta` by whoever has one: a permission
        // request carries the call's fields without it.
        backgrounded: false,
    }
}

/// The words a call carries, in the order it gave them.
///
/// Its own list rather than one string: a call says things at different
/// moments -- the plan first and what became of it after -- and joining
/// them at the edge would leave whoever draws them unable to tell one from
/// the next.
fn words_of(content: &[ToolCallContent]) -> Vec<String> {
    content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(block) => words(&block.content),
            // A diff and a terminal are not words: they have their own
            // shapes and their own rows, and reading them as prose would
            // draw a file twice in two different ways.
            _ => None,
        })
        .collect()
}

/// The command a call is running, if it is running one.
fn ran_in(content: &[ToolCallContent]) -> Option<String> {
    content.iter().find_map(|content| match content {
        ToolCallContent::Terminal(terminal) => Some(terminal.terminal_id.0.to_string()),
        _ => None,
    })
}

/// The change a call carries, if it carries one.
fn change_of(content: &[ToolCallContent]) -> Option<Change> {
    content.iter().find_map(|content| match content {
        ToolCallContent::Diff(diff) => Some(Change {
            path: diff.path.clone(),
            before: diff.old_text.clone(),
            after: diff.new_text.clone(),
        }),
        _ => None,
    })
}

/// Where a tool call said it was working.
fn place_of(location: &ToolCallLocation) -> Place {
    Place {
        path: location.path.clone(),
        line: location.line,
    }
}

/// One command, as the view offers it.
fn order_of(order: &AvailableCommand) -> Order {
    Order {
        name: order.name.clone(),
        description: order.description.clone(),
        hint: order.input.as_ref().and_then(|input| match input {
            agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(hint) => {
                Some(hint.hint.clone())
            }
            // A kind of input Obelus has not heard of. The name is still
            // the command; what it takes after it is between the reader and
            // the agent.
            _ => None,
        }),
    }
}

/// The words in a content block.
///
/// A block is text, an image, audio, or a link to something in the
/// workspace. Only the first is words; the others are named rather than
/// dropped, because a turn that silently loses a block reads as an agent
/// that said nothing.
fn words(content: &ContentBlock) -> Option<String> {
    match content {
        ContentBlock::Text(text) => Some(text.text.clone()),
        ContentBlock::Image(_) => Some("(an image)".to_string()),
        ContentBlock::Audio(_) => Some("(audio)".to_string()),
        ContentBlock::ResourceLink(link) => Some(format!("({})", link.uri)),
        ContentBlock::Resource(_) => Some("(a resource)".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    /// Every shape Obelus refuses to hand to the machine's own launcher.
    ///
    /// The end-to-end test drives one of these through a real agent; this
    /// is the rest of them, because a predicate with five arms wants five
    /// cases and not five conversations.
    ///
    /// Broken deliberately by returning the string whatever it says.
    #[test]
    fn only_a_web_address_is_somewhere_to_go() {
        for said in [
            // A scheme some program on this machine has registered, which
            // is the shape that turns a link into a way to start it.
            "vscode://file/etc/passwd",
            "ms-msdt:/id",
            // The disk, by either spelling.
            "file:///etc/passwd",
            "file://localhost/etc/passwd",
            // No machine named.
            "https:///nowhere",
            "http://?query",
            // Not a URL at all.
            "console.example.com",
            "javascript:alert(1)",
            // Whitespace, which is only ever `%20` in a well-formed URL and
            // is how a string becomes two arguments.
            "https://example.com/ --a-flag",
            "https://example.com/\nhttps://elsewhere.com",
        ] {
            assert_eq!(
                super::somewhere_to_go(said),
                None,
                "Obelus would have opened {said:?}"
            );
        }

        for said in [
            "https://console.example.com/oauth/authorize?code=1&state=2",
            "http://localhost:8080/callback",
            // The scheme as the agent happens to spell it.
            "HTTPS://example.com/",
        ] {
            assert_eq!(
                super::somewhere_to_go(said).as_deref(),
                Some(said),
                "Obelus would not have opened {said:?}"
            );
        }
    }
}
