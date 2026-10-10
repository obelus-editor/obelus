//! The transcript laid out into rows at a width, and what a row is made of.
//!
//! Here rather than in whoever draws it, because the keys, the caret, a
//! hold and the drawing all walk the same rows: see [`Chat::rows`].

use super::*;

/// Whether what this voice says arrives as the protocol's own text
/// content, which is the thing it asks clients to read as markdown.
///
/// The agent's answer and its thinking do. The reader's own message does
/// not -- Obelus has it as they typed it, and reflowing somebody's words
/// back at them is Obelus deciding what they meant. Nor do the rows Obelus
/// makes up itself: a note, a state, a step of a plan, a run's heading.
///
/// Nor a tool call, which is not one voice: what it carries is the
/// agent's text content and is read as markdown where the row is built,
/// and its title is a line the protocol calls human-readable rather than
/// markdown. Both are decided there, where which of the two is in hand is
/// known.
const fn reads_as_markdown(speaker: Speaker) -> bool {
    matches!(speaker, Speaker::Agent | Speaker::Thought)
}

/// Which characters of a row fall between two places in what it was laid
/// out from.
///
/// The runs of a row carry the bytes they were laid out from, so this is
/// where those bytes fall inside the window and what that is in characters
/// of the row. The runs that carry nothing -- what the reading drew rather
/// than read -- are counted past rather than held: whether they are inside
/// is a question about the rows around them, and [`Chat::mark_held`] is
/// where that is answered.
fn row_held(spans: &[Span], from: usize, to: usize) -> Option<std::ops::Range<usize>> {
    let mut held: Option<std::ops::Range<usize>> = None;
    let mut column = 0usize;
    for span in spans {
        let bytes = span.from.clone();
        for (offset, character) in span.text.char_indices() {
            if let Some(bytes) = bytes.as_ref() {
                let byte = bytes.start + offset;
                if byte >= from && byte < to {
                    let next = column..column + 1;
                    held = Some(match held {
                        Some(had) => had.start..next.end,
                        None => next,
                    });
                }
            }
            let _ = character;
            column += 1;
        }
    }
    held
}

/// One run of a row, in the colour the words themselves have nothing to
/// say about.
#[must_use]
fn plain(text: String) -> Vec<Span> {
    vec![Span::new(text, Ink::Plain)]
}

/// A thing said, laid out for a width, as the runs of each row.
///
/// Markdown where the protocol says the text is markdown, and plain
/// wrapping everywhere else. What an agent said, what it was thinking and
/// what a tool call carries all arrive as `ContentBlock::Text`, which is
/// the one the protocol writes "Clients SHOULD render this text as
/// Markdown" about. A tool call's *title* is not one of those, nor are the
/// lines of a change, nor the reader's own message -- Obelus has that as
/// they typed it and has no business reflowing it -- nor anything Obelus
/// says in its own voice.
///
/// The wrapping is markdown's own, because that is where the difficulty
/// lives: a fenced block does not wrap like a paragraph and a bullet's
/// second line is indented under its first.
///
/// Each row comes with the links on it: the ones markdown wrote as links,
/// and the addresses written out in the words.
fn laid_out(text: &str, width: u16, markdown: bool) -> Vec<Laid> {
    let written = addresses(text);
    if !markdown {
        return obelus_text::wrapped_from(text, width)
            .into_iter()
            .map(|(said, from)| {
                let spans = vec![Span::from_source(said, Ink::Plain, from)];
                let links = links_on(&spans, &written);
                (spans, None, links)
            })
            .collect();
    }
    obelus_markdown::render(text, width)
        .into_iter()
        .map(|row| match row.rule {
            // A rule has no words of its own, and the transcript has no
            // room for a view that draws one: it is a row like the others,
            // so it is drawn as what it is.
            true => (
                vec![Span::new(
                    "\u{2500}".repeat(usize::from(width.max(1))),
                    Ink::Mark,
                )],
                None,
                Vec::new(),
            ),
            // A block of code is copied, not followed: an address in it is
            // a line of the code.
            false if row.code.is_some() => (row.spans, row.code, Vec::new()),
            false => {
                let found: Vec<_> = row
                    .links
                    .into_iter()
                    .chain(written.iter().cloned())
                    .collect();
                let links = links_on(&row.spans, &found);
                (row.spans, row.code, links)
            }
        })
        .collect()
}

/// A row of a thing said, laid out: its runs, the block of code it is part
/// of, and the links on it.
type Laid = (Vec<Span>, Option<obelus_row::Code>, Vec<Link>);

/// The links among `found` whose words are on a row, by the row's own
/// characters.
fn links_on(spans: &[Span], found: &[(std::ops::Range<usize>, String)]) -> Vec<Link> {
    let mut links: Vec<Link> = found
        .iter()
        .filter_map(|(words, to)| {
            Some(Link {
                characters: row_held(spans, words.start, words.end)?,
                to: to.clone(),
            })
        })
        .collect();
    // An address written out as a link's own words is found twice, and is
    // one link.
    links.sort_by_key(|link| link.characters.start);
    links.dedup_by(|later, earlier| later.characters.start < earlier.characters.end);
    links
}

/// Every web address written out in a text, and where it is.
///
/// Written out rather than linked: an address an agent puts in a sentence
/// is one the reader would otherwise have to hold and copy out. Where it
/// ends is a guess, and the guess is the one a reader makes -- at a space
/// or a character no address is written with, before the full stop of the
/// sentence it ends, and before a bracket it did not open.
fn addresses(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| text.get(from..)?.find(scheme))
        .min()
        .map(|at| from + at)
    {
        let rest = &text[start..];
        let mut end = rest
            .find(|c: char| !c.is_ascii_graphic() || matches!(c, '<' | '>' | '"' | '`' | '|'))
            .unwrap_or(rest.len());
        while let Some(last) = rest[..end].chars().last() {
            let said = &rest[..end];
            let unopened = |open: char, close: char| {
                last == close && said.matches(close).count() > said.matches(open).count()
            };
            match matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_')
                || unopened('(', ')')
                || unopened('[', ']')
            {
                true => end -= 1,
                false => break,
            }
        }
        let said = &rest[..end];
        if said
            .split_once("://")
            .is_some_and(|(_, host)| !host.is_empty())
        {
            found.push((start..start + end, said.to_string()));
        }
        from = start + end.max(1);
    }
    found
}

impl Chat {
    /// Everything said, as rows wrapped to a width.
    ///
    /// A blank row between one speaker and the next: a transcript with no
    /// space in it reads as one voice.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<Row> {
        let mut rows = self.laid_out(width);
        // And what the reader has hold of, once there are rows to hold:
        // the two ends of a selection are places in what was said, and
        // which characters of which rows that is depends on the width
        // these were just laid out at.
        //
        // Here rather than in what is kept, so that a drag across a long
        // conversation does not lay the whole of it out again for every
        // report the terminal sends.
        self.mark_held(&mut rows);
        rows
    }

    /// The rows, laid out or remembered from the last time they were.
    fn laid_out(&self, width: u16) -> Vec<Row> {
        // And against which characters are pictures, which is a question
        // about the window's fonts and changes when the reader's do: a
        // heart laid out one cell wide and drawn two runs into the word
        // after it.
        let at = (width, obelus_text::pictures_version());
        if let Some(rows) = self
            .laid
            .borrow()
            .as_ref()
            .filter(|(laid, _)| *laid == at)
            .map(|(_, rows)| rows.clone())
        {
            return rows;
        }
        let rows = self.lay_out(width);
        *self.laid.borrow_mut() = Some((at, rows.clone()));
        rows
    }

    /// Every row of the conversation, at a width.
    fn lay_out(&self, width: u16) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut at = 0;
        let mut first = true;
        while at < self.said.len() {
            let run = self.run_from(at);
            if !first {
                rows.push(self.blank(at));
            }
            first = false;
            match run.len() >= LEAST_TO_FOLD {
                // A run of one kind, under a heading of its own: thirty
                // tool calls in a turn is a log, and a reader looking for
                // what the agent *did* should not have to scroll past the
                // machine to find it.
                true => rows.extend(self.run_rows(run.clone(), width)),
                false => {
                    for index in run.clone() {
                        rows.extend(self.said_rows(index, 0, width));
                    }
                }
            }
            at = run.end;
        }
        // And what is happening now, under the last of it: the foot of the
        // transcript is where the next thing will appear, which is where a
        // reader is already looking.
        if let Some(doing) = self.doing.as_deref() {
            if !first {
                rows.push(self.blank(self.said.len()));
            }
            // The agent's list for this turn, where it has one: one row
            // saying which step it is on, and the whole of it under that
            // for a reader who wants to see whether it understood the job
            // -- which is the moment they would interrupt it.
            //
            // Folded to the one row by default. While the agent works the
            // transcript is filling with the thing the reader is actually
            // watching, and seven rows of checklist is a third of the
            // screen held for the whole turn.
            let planning = !self.plan.is_empty();
            rows.push(Row {
                speaker: Speaker::Doing,
                spans: plain(match planning {
                    true => self.step_now(),
                    false => doing.to_string(),
                }),
                first: true,
                state: None,
                kind: String::new(),
                place: None,
                away: None,
                links: Vec::new(),
                from: None,
                held: None,
                // Anchored one past the end of what was said, which is the
                // one index that can never name a [`Said`]: a plan is not a
                // thing that was said, and giving it an index into the
                // transcript would be filing it as one.
                folds: planning.then_some(Folds::Plan),
                open: self.plan_open,
                unsent: None,
                again: None,
                marker: None,
                changed: None,
                code: None,
                depth: 0,
            });
            if planning && self.plan_open {
                rows.extend(self.plan.iter().flat_map(|step| {
                    obelus_text::wrapped(&step.said, width.saturating_sub(DEEPER))
                        .into_iter()
                        .enumerate()
                        .map(|(line, text)| Row {
                            speaker: Speaker::Step,
                            spans: plain(text),
                            from: None,
                            held: None,
                            first: false,
                            // On the first row of a step only, so a step
                            // that wraps is one step with one mark.
                            state: (line == 0).then(|| step.state.clone()),
                            kind: String::new(),
                            away: None,
                            links: Vec::new(),
                            place: None,
                            folds: None,
                            open: false,
                            unsent: None,
                            again: None,
                            marker: None,
                            changed: None,
                            code: None,
                            depth: 1,
                        })
                }));
            }
        }
        rows
    }

    /// The run of things said that begins at `at`: as many tool calls of
    /// one kind as follow one another, or the one thing that is not.
    fn run_from(&self, at: usize) -> std::ops::Range<usize> {
        let Some(said) = self.said.get(at) else {
            // Never empty: the caller walks by the end of what this
            // returns, and an empty range would leave it where it was.
            return at..at + 1;
        };
        if said.speaker != Speaker::Tool {
            return at..at + 1;
        }
        let mut end = at + 1;
        while self
            .said
            .get(end)
            .is_some_and(|next| next.speaker == Speaker::Tool && next.kind == said.kind)
        {
            end += 1;
        }
        at..end
    }

    /// A run of tool calls: its heading, and its members when it is open.
    fn run_rows(&self, run: std::ops::Range<usize>, width: u16) -> Vec<Row> {
        let Some(said) = self.said.get(run.start) else {
            return Vec::new();
        };
        let members: Vec<&Said> = self.said[run.clone()].iter().collect();
        let count = run.len();
        // What they have in common, where they have it: a run of reads is a
        // run of files, and a run of commands is a run of calls.
        let what = match members.iter().all(|said| !said.places.is_empty()) {
            true => "Files",
            false => "Calls",
        };
        // The state of the run is the state of the worst of it: one that
        // failed is the news, and one still running is why the row moves.
        let state = members
            .iter()
            .filter_map(|said| said.state.as_deref())
            .fold(None, |worst: Option<&str>, state| match (worst, state) {
                (Some("failed"), _) | (_, "failed") => Some("failed"),
                (Some("in_progress"), _) | (_, "in_progress") => Some("in_progress"),
                (Some("pending"), _) | (_, "pending") => Some("pending"),
                (_, state) => Some(state),
            });
        let open = self.is_run_open(run.start);
        let mut rows = vec![Row {
            speaker: Speaker::Tool,
            spans: plain(format!("{count} {what}")),
            from: None,
            held: None,
            first: true,
            state: state.map(str::to_string),
            kind: said.kind.clone(),
            place: None,
            away: None,
            links: Vec::new(),
            folds: Some(Folds::Run(run.start)),
            open,
            unsent: None,
            again: None,
            marker: None,
            changed: None,
            code: None,
            depth: 0,
        }];
        if open {
            for index in run {
                rows.extend(self.said_rows(index, 1, width));
            }
        }
        rows
    }

    /// One thing said, as the rows it takes.
    fn said_rows(&self, at: usize, depth: u8, width: u16) -> Vec<Row> {
        let Some(said) = self.said.get(at) else {
            return Vec::new();
        };
        let room = width.saturating_sub(u16::from(depth) * DEEPER);
        let inside = room.saturating_sub(DEEPER);

        // What a call carries, under the call's own row -- which is what
        // folds it, because a diff is the one thing an agent sends that is
        // longer than the screen and a plan is the other. Neither is prose:
        // the row says what the call is and this is what it is about.
        //
        // One shape for the words and the lines, and both of them where a
        // call has both. They were two arms once, the change first and
        // returning, so a call that said why it was changing something had
        // the why dropped -- and the protocol puts them in one list.
        //
        // The words first: they are the account of the change, and an
        // account after the thing it accounts for is a footnote.
        // What the call carries that is not its own title over again --
        // dropped here rather than where the rows are drawn, because what
        // is wrong is the row: a fold is a promise of something behind it,
        // and behind a copy of the heading there is nothing. Nothing is
        // assumed about what an agent means by it: a call whose words say
        // something of their own keeps every row it had, and one that
        // starts saying something gets them back in the update that does.
        // Kept with which of the words each is, because a place in the
        // transcript has to say which text of a thing said it is in and
        // the filtering above loses the count.
        let carried: Vec<(usize, &String)> = said
            .words
            .iter()
            .enumerate()
            .filter(|(_, words)| words.trim() != said.text.trim())
            .collect();
        if said.speaker == Speaker::Tool && (!carried.is_empty() || !said.change.is_empty()) {
            // Wrapped, like the title of a call that carries nothing --
            // which is the same title and was the only one being wrapped.
            // Put down as one run however long it was, the calls whose
            // headings ran off the side were exactly the ones with
            // something behind them to read, and a command Obelus had run
            // is a title as long as the command.
            //
            // The first row is what opens and what folds; the rest are the
            // rest of the same words, at the same depth and from the same
            // text, so a place in the title is still a place in the title.
            //
            // A few rows of it are shown whatever the fold says, and the
            // rest goes behind it.
            //
            // All of it was drawn once, which nobody noticed while a title
            // was a handful of words: it wrapped to one row and there was
            // no rest of it. A command is a title as long as the command,
            // and an agent that writes a script into a heredoc sends the
            // whole script as the title -- so a closed call sat there with
            // twenty rows of shell under a mark saying it was shut, and
            // the key that was supposed to put it away moved one row of
            // output.
            //
            // Then all of it went behind the fold, and a command of two
            // rows -- which is most of them -- could not be read without
            // opening the call and reading it past its own output. The
            // reader is looking at a transcript of commands: the command
            // is the thing on the row.
            //
            // So a cap, which is what a card's own prose gets and for the
            // same reason: somebody else's text may be as long as it likes
            // and may not push what it belongs to off the screen. Under it
            // the whole command is on the page and opening the call is
            // about the output. Over it the arrow on the first row says
            // there is more, which is what that arrow says about
            // everything else behind it.
            let mut title = laid_out(&said.text, room, reads_as_markdown(said.speaker)).into_iter();
            let (spans, links) = title.next().map_or_else(
                || (plain(String::new()), Vec::new()),
                |(spans, _, links)| (spans, links),
            );
            let mut rows = vec![Row {
                changed: (!said.change.is_empty())
                    .then(|| obelus_git::change::counted(&said.change)),
                links,
                ..self.opening(
                    said,
                    spans,
                    depth,
                    Some(Folds::Said(at)),
                    Some((at, Source::Text)),
                )
            }];
            // The rows of it that are shown closed as well as open.
            let open = self.is_open(at);
            let shown = match open {
                true => usize::MAX,
                false => MOST_TITLE_ROWS.saturating_sub(1),
            };
            rows.extend(title.by_ref().take(shown).map(|(spans, _, links)| Row {
                links,
                ..Self::under(said, spans, depth, Some((at, Source::Text)))
            }));
            if open {
                // Markdown, unless Obelus is running a command for this
                // call: then these words are the command and what it has
                // printed, put here by [`Chat::running`], and a terminal's
                // bytes are not prose. Read as markdown they lose the line
                // between the command and its output -- one newline is a
                // soft break -- which is a call saying it ran something
                // that it never ran.
                rows.extend(carried.iter().flat_map(|(which, words)| {
                    laid_out(words, inside, said.ran.is_none()).into_iter().map(
                        |(spans, code, links)| Row {
                            code,
                            links,
                            ..Self::under(said, spans, depth + 1, Some((at, Source::Words(*which))))
                        },
                    )
                }));
                // A diff's own markers, which words do not get: "it is
                // changing this" and "it is saying this" are different
                // news, and the markers are where a reader takes that in.
                rows.extend(said.change.iter().enumerate().flat_map(|(which, line)| {
                    laid_out(&line.text, inside, false)
                        .into_iter()
                        .map(move |(spans, ..)| Row {
                            marker: line.marker,
                            ..Self::under(said, spans, depth + 1, Some((at, Source::Change(which))))
                        })
                }));
            }
            return rows;
        }

        let words = laid_out(&said.text, room, reads_as_markdown(said.speaker));
        // Thinking long enough to be worth putting away gets a heading of
        // its own, which is what folds it. Obelus does not fold it away by
        // itself -- an agent's reasoning about the code is often the most
        // of what a turn is worth -- but a reader who has read it should be
        // able to close it. Short thinking is just the words: a heading
        // over three words is two rows saying one thing.
        if said.speaker != Speaker::Thought || words.len() < LEAST_TO_FOLD {
            return words
                .into_iter()
                .enumerate()
                .map(|(row, (spans, code, links))| Row {
                    code,
                    links,
                    ..match row {
                        0 => self.opening(said, spans, depth, None, Some((at, Source::Text))),
                        _ => Self::under(said, spans, depth, Some((at, Source::Text))),
                    }
                })
                .collect();
        }
        // The heading is Obelus's word for what is behind it, not the
        // agent's: there is nothing in it to take a copy of.
        let mut rows = vec![self.opening(
            said,
            plain("thought".to_string()),
            depth,
            Some(Folds::Said(at)),
            None,
        )];
        if self.is_open(at) {
            rows.extend(laid_out(&said.text, inside, true).into_iter().map(
                |(spans, code, links)| Row {
                    code,
                    links,
                    ..Self::under(said, spans, depth + 1, Some((at, Source::Text)))
                },
            ));
        }
        rows
    }

    /// The row a thing said begins with.
    ///
    /// It carries the glyph and everything that is true of the whole of it:
    /// where it has got to, the file it names, and whether there is more
    /// behind it than it is showing.
    fn opening(
        &self,
        said: &Said,
        spans: Vec<Span>,
        depth: u8,
        folds: Option<Folds>,
        from: Option<(usize, Source)>,
    ) -> Row {
        Row {
            speaker: said.speaker,
            spans,
            from,
            held: None,
            first: true,
            state: said.state.clone(),
            kind: said.kind.clone(),
            place: said
                .places
                .first()
                .map(|place| (place.clone(), said.places.len() - 1)),
            // Where it points on the web, which only one voice has: a row
            // the reader was sent away by keeps the address it sent them
            // to, so pressing the key on it sends them again.
            away: match said.speaker {
                Speaker::Away => said.words.first().cloned(),
                _ => None,
            },
            links: Vec::new(),
            folds,
            open: folds.is_some_and(|what| self.is_open_now(what)),
            unsent: match said.unsent {
                true => from.map(|(at, _)| at),
                false => None,
            },
            again: match (said.speaker, said.unsent) {
                (Speaker::Reader, false) => from.map(|(at, _)| at),
                _ => None,
            },
            marker: None,
            changed: None,
            code: None,
            depth,
        }
    }

    /// A row that continues something already begun.
    fn under(said: &Said, spans: Vec<Span>, depth: u8, from: Option<(usize, Source)>) -> Row {
        Row {
            speaker: said.speaker,
            spans,
            from,
            held: None,
            first: false,
            state: None,
            kind: said.kind.clone(),
            place: None,
            away: None,
            links: Vec::new(),
            folds: None,
            open: false,
            unsent: None,
            again: None,
            marker: None,
            changed: None,
            code: None,
            depth,
        }
    }

    /// The row that separates one thing said from the next.
    fn blank(&self, at: usize) -> Row {
        Row {
            speaker: self.said.get(at).map_or(Speaker::Note, |said| said.speaker),
            spans: Vec::new(),
            // A blank is not a word of anybody's.
            from: None,
            held: None,
            first: false,
            state: None,
            kind: String::new(),
            place: None,
            away: None,
            links: Vec::new(),
            folds: None,
            open: false,
            unsent: None,
            again: None,
            marker: None,
            changed: None,
            code: None,
            depth: 0,
        }
    }

    /// Whether the thing a mark folds is open.
    ///
    /// The one answer for all three, because the row that draws the mark
    /// has to agree with the key that presses it: which of them a row is
    /// about is now on the row, so neither has to guess from an index.
    pub(super) fn is_open_now(&self, what: Folds) -> bool {
        match what {
            Folds::Said(at) => self.is_open(at),
            Folds::Run(at) => self.is_run_open(at),
            Folds::Plan => self.plan_open,
        }
    }

    /// Whether what one thing said begins is open.
    fn is_open(&self, at: usize) -> bool {
        let Some(said) = self.said.get(at) else {
            return false;
        };
        if let Some(open) = said.opened {
            return open;
        }
        if said.speaker == Speaker::Thought {
            return true;
        }
        // A change is open while it is the question: the agent is asking to
        // make it, and what it is asking about is the lines. Once it is
        // made the file itself has them, and Obelus draws a file's changes
        // in the margin beside them -- so the block folds away and the row
        // that opens it stays. A command still running is the same case:
        // what it is printing is the thing being waited on.
        //
        // Having failed is not on this list, and was. The reasoning was
        // that a reader whose tests have just failed is looking for the
        // failure -- true, and not Obelus's to act on, because "failed" is
        // a word from the agent and a great many commands say it without
        // anything being wrong. `grep` exits 1 with nothing to report,
        // `diff` exits 1 on a difference, `test` exits 1 for false: an
        // agent asking a question with a command gets a non-zero answer
        // and marks the call failed, and Obelus was throwing the output of
        // every one of those open and holding it open. What is left is the
        // mark, which says where to look, and the key, which is one press.
        matches!(said.state.as_deref(), Some("pending" | "in_progress"))
    }

    /// Whether the run of calls beginning at `at` is open.
    ///
    /// What the reader said about it, and otherwise shut. A run is a log:
    /// Obelus makes one out of a stretch of calls of a kind exactly
    /// because nobody reads a log line by line, and a run that decides for
    /// itself when to be a log again is a run the reader cannot keep shut.
    ///
    /// It used to open itself when any of its calls had failed, and that
    /// is what this is about. One call in eight says failed -- which an
    /// agent says of a `grep` that matched nothing as readily as of a
    /// build that fell over -- and eight calls came open, with a cross on
    /// the heading over them. Seven of them had nothing to say. The
    /// heading still carries the worst state of what is inside it, so
    /// nothing is hidden: what has gone is a shut thing opening itself.
    ///
    /// Asked of the run rather than of the call it begins at, which is what
    /// it used to be. A run is named by that call and so they shared an
    /// answer: opening the run set the call's `opened`, the call's own mark
    /// read it back, and enter on the first call of a run shut the run.
    fn is_run_open(&self, at: usize) -> bool {
        self.said
            .get(at)
            .and_then(|said| said.run_opened)
            .unwrap_or(false)
    }

    /// Marks the rows the reader has hold of.
    ///
    /// In two passes, because what a middle row holds cannot be worked out
    /// from the row alone. The ends of the selection are places in what was
    /// said; everything between them is held whole -- including the rows
    /// that came from nothing anybody wrote, the blank between two things
    /// said and the bullet in front of a list item. Those are on the screen
    /// between the words that are held, so they are part of what the reader
    /// is pointing at, and leaving them out would copy something other than
    /// what is in front of them.
    fn mark_held(&self, rows: &mut [Row]) {
        let Some((anchor, other)) = self.held else {
            return;
        };
        let (first, last) = match anchor <= other {
            true => (anchor, other),
            false => (other, anchor),
        };
        let mut ends: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let Some((said, source)) = row.from else {
                continue;
            };
            let here = (said, source);
            if here < (first.said, first.source) || here > (last.said, last.source) {
                continue;
            }
            let from = match here == (first.said, first.source) {
                true => first.at,
                false => 0,
            };
            let to = match here == (last.said, last.source) {
                true => last.at,
                false => usize::MAX,
            };
            if let Some(held) = row_held(&row.spans, from, to) {
                ends.push((index, held));
            }
        }
        let (Some((top, first_held)), Some((foot, last_held))) = (ends.first(), ends.last()) else {
            return;
        };
        let (top, foot) = (*top, *foot);
        for (index, row) in rows.iter_mut().enumerate() {
            if index < top || index > foot {
                continue;
            }
            let whole = row.text().chars().count();
            row.held = Some(match (index == top, index == foot) {
                (true, true) => first_held.start..last_held.end,
                (true, false) => first_held.start..whole,
                (false, true) => 0..last_held.end,
                (false, false) => 0..whole,
            });
        }
    }
}
