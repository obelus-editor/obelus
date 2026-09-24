//! Markdown, laid out into rows, from the tree Obelus already parses.
//!
//! This borrowed a markdown renderer for a long time, and to use one at all
//! it had to cut the fenced blocks out of the source first: the renderer
//! threw the fence's language away before anything could ask what it was.
//! Two scanners grew up around that -- one to find the fences, one to decide
//! which lines were one paragraph -- and neither of them was a markdown
//! parser. They disagreed with each other about a fence inside a block
//! quote, and both disagreed with the buffer, where the same file is read by
//! a grammar that gets it right.
//!
//! So the reading reads that grammar instead. One answer to where a block
//! begins, what kind it is and what language is in it, and it is the answer
//! the reader is already looking at while they edit the file.
//!
//! What is borrowed now is nothing. The wrapping is
//! [`obelus_text::wrapped_from`] and the column widths are [`table`], both
//! Obelus's own.

/// How wide a table's columns are drawn.
mod table;

use std::ops::Range;

use obelus_row::{Ink, Row, Span};
use obelus_syntax::{LanguageId, highlight::Highlights, parse::SyntaxState, tree_sitter::Node};
use obelus_text::{Text, coordinates::ByteOffset, kind::SyntaxKind, text_width};

/// Lays a markdown source out for a width.
///
/// Every row is at most `width` cells, so the caller never has to wrap.
#[must_use]
pub fn render(source: &str, width: u16) -> Vec<Row> {
    let text = Text::from_string(source);
    let Some(state) = SyntaxState::new(LanguageId::Markdown, &text) else {
        // No grammar for markdown in this build. The words are still the
        // reader's, so they are wrapped and handed back unadorned.
        return plainly(source, width);
    };
    let mut highlights = Highlights::default();
    highlights.refresh(&state, &text, ByteOffset::new(0)..text.byte_length());
    // The trees for what is *inside* the paragraphs -- a word in italics, a
    // span of code, the words of a link. Already parsed, because the buffer
    // needs them too: this is the document's own injections, read rather
    // than asked for again. Parsing each paragraph a second time cost more
    // than laying the whole file out.
    // Every run of it, because every row is being laid out: a reading is
    // scrolled through, and the rows below the screen are how it knows how
    // far there is to scroll. What the *buffer* does instead -- ask only for
    // what is on the screen -- is not open to something that has to produce
    // the whole page at once.
    state.look_at(&text, 0..text.byte_length().get());
    let injected = state.injected();
    let inline: Vec<(Range<usize>, Node<'_>)> = injected
        .iter()
        .filter(|one| one.language() == LanguageId::MarkdownInline)
        .map(|one| {
            let root = one.tree().root_node();
            (root.byte_range(), root)
        })
        .collect();
    let mut laying = Laying {
        source,
        highlights,
        inline,
        rows: Vec::new(),
        owed: None,
    };
    laying.blocks(state.tree().root_node(), width, &Prefix::default());
    laying.rows
}

/// A source Obelus cannot parse, wrapped and no more.
fn plainly(source: &str, width: u16) -> Vec<Row> {
    obelus_text::wrapped_from(source, width)
        .into_iter()
        .map(|(said, from)| {
            Row::of(vec![Span::from_source(
                said,
                Ink::Plain,
                from.start..from.end,
            )])
        })
        .collect()
}

/// What goes down the left of a block, and how much room it takes.
///
/// A quote's mark and a bullet are not the author's words: they are what the
/// reading adds so that a nested thing looks nested. This is what every row
/// of the block carries; a bullet, which is carried once, is owed instead --
/// see [`Laying::owe`].
#[derive(Clone, Default)]
struct Prefix {
    /// What every row of the block carries.
    rest: Vec<Span>,
    /// How many cells it takes.
    width: u16,
}

impl Prefix {
    /// This prefix with another put after it, for a block inside a block.
    fn and(&self, rest: Vec<Span>, width: u16) -> Self {
        Self {
            rest: self.rest.iter().cloned().chain(rest).collect(),
            width: self.width + width,
        }
    }

    /// The room left for what the block actually says.
    fn room(&self, width: u16) -> u16 {
        width.saturating_sub(self.width).max(1)
    }
}

/// A mark the reading adds, of a given width.
///
/// Takes what it is given rather than a `&str`: the marks that never change
/// -- a quotation's bar, the sides of a box -- come in as themselves and are
/// not copied, and the ones that are worked out for a width come in owned.
fn mark(said: impl Into<std::borrow::Cow<'static, str>>) -> Span {
    Span::new(said, Ink::Mark)
}

/// The blank that stands where a mark stood, on the rows after the first.
fn room(cells: usize) -> Span {
    Span::new(" ".repeat(cells), Ink::Plain)
}

/// One document, becoming rows.
struct Laying<'a> {
    source: &'a str,
    highlights: Highlights,
    /// The parsed insides of every paragraph, by where each one sits.
    inline: Vec<(Range<usize>, Node<'a>)>,
    rows: Vec<Row>,
    /// The marks the next row owes, in place of its own left margin.
    ///
    /// A bullet is drawn once and then stood under, and what stands under it
    /// is every row of every block in the item -- the paragraph, the block
    /// of code below it, the list inside that. Which of those draws the
    /// first row is not the bullet's business, so the bullet is left here
    /// and taken by whichever row comes first.
    owed: Option<Vec<Span>>,
}

impl Laying<'_> {
    /// Every block under this node.
    ///
    /// With the blank lines between them kept, because a blank line is the
    /// one piece of punctuation markdown has for "these are two things".
    /// Blocks run together are a wall of text however well each of them is
    /// laid out.
    fn blocks(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let mut cursor = node.walk();
        let mut last: Option<usize> = None;
        for child in node.children(&mut cursor) {
            if child.kind() == "block_continuation" {
                continue;
            }
            let before = self.rows.len();
            if let Some(end) = last
                && self.parted(end, child.start_byte())
            {
                self.row(prefix, Vec::new());
            }
            self.block(child, width, prefix);
            // A block that drew nothing -- a marker, a run of whitespace --
            // leaves the blank line to whatever comes after it.
            if self.rows.len() > before {
                last = Some(child.end_byte());
            }
        }
    }

    /// Whether the source between two blocks holds a blank line.
    ///
    /// Asked of the source rather than of the tree, because a blank line is
    /// the one thing a markdown tree has no node for: it is what separates
    /// the nodes, and what is left over when they are taken out. A block
    /// ends at the end of its own last line, so anything at all between two
    /// of them is a line nobody wrote on -- whatever markers of the blocks
    /// they are inside run through it.
    fn blank_between(&self, from: usize, to: usize) -> bool {
        self.source
            .get(from..to)
            .is_some_and(|gap| gap.contains('\n'))
    }

    /// Whether two blocks are parted by a blank line, wherever it ended up.
    ///
    /// A block that holds others -- a list item, a quote -- takes the blank
    /// line after it into itself, so the gap the first of these looks in is
    /// empty and the line is at the end of the block instead. Both are the
    /// same blank line and mean the same thing.
    fn parted(&self, end: usize, next: usize) -> bool {
        if self.blank_between(end, next) {
            return true;
        }
        self.source
            .get(..end)
            .is_some_and(|said| said.len() - said.trim_end_matches('\n').len() > 1)
    }

    /// One block.
    fn block(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        // The mark a list item is introduced by, which the item has already
        // drawn: see [`Laying::list`].
        if node.kind().starts_with("list_marker") {
            return;
        }
        match node.kind() {
            // A section is the tree's way of hanging what follows a heading
            // under it. Nothing is drawn for it.
            "document" | "section" => self.blocks(node, width, prefix),
            "atx_heading" | "setext_heading" => self.heading(node, width, prefix),
            "paragraph" => self.paragraph(node, width, prefix),
            "block_quote" => self.quote(node, width, prefix),
            "list" => self.list(node, width, prefix),
            "fenced_code_block" | "indented_code_block" => self.code(node, width, prefix),
            "pipe_table" => self.table(node, width, prefix),
            "thematic_break" => self.rows.push(Row {
                spans: Vec::new(),
                rule: true,
            }),
            // The markers a block is made of rather than anything it says,
            // and the blank lines between blocks.
            "block_continuation" | "block_quote_marker" | "minus_metadata" | "plus_metadata" => {}
            // Anything else -- an HTML block, a link's definition -- is the
            // author's text and is laid out as text.
            _ => self.paragraph(node, width, prefix),
        }
    }

    /// A heading, in the ink of its level.
    fn heading(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let level = node
            .children(&mut node.walk())
            .find_map(|child| match child.kind() {
                "atx_h1_marker" | "setext_h1_underline" => Some(1),
                "atx_h2_marker" | "setext_h2_underline" => Some(2),
                "atx_h3_marker" => Some(3),
                "atx_h4_marker" => Some(4),
                "atx_h5_marker" => Some(5),
                "atx_h6_marker" => Some(6),
                _ => None,
            })
            .unwrap_or(1);
        let said = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "inline" || child.kind() == "paragraph");
        let Some(said) = said else {
            return;
        };
        self.said(said, width, prefix, Ink::Heading(level));
    }

    /// A paragraph, or anything else that is a run of words.
    fn paragraph(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let inline = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "inline");
        self.said(inline.unwrap_or(node), width, prefix, Ink::Plain);
    }

    /// A run of words: gathered, wrapped, and inked by what is in it.
    fn said(&mut self, node: Node<'_>, width: u16, prefix: &Prefix, ink: Ink) {
        let gathered = self.gather(node, Join::Soft);
        if gathered.text.trim().is_empty() {
            return;
        }
        let said = node.byte_range();
        let inside = self
            .inline
            .iter()
            .find(|(range, _)| range.start >= said.start && range.end <= said.end)
            .map(|(_, root)| *root);
        let (shown, looks) = stripped(&gathered, ink, inside);
        if shown.text.trim().is_empty() {
            return;
        }
        for (said, at) in obelus_text::wrapped_from(&shown.text, prefix.room(width)) {
            let spans = split(&said, at.start, &looks, &shown);
            self.row(prefix, spans);
        }
    }

    /// A quoted block: the mark down its left, and whatever it holds.
    fn quote(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        // The mark on every row, not only the first: a quote is quoted all
        // the way down, and a reader skimming the left edge is reading that
        // column rather than counting rows.
        let inside = prefix.and(vec![mark("\u{2503} ")], 2);
        self.blocks(node, width, &inside);
    }

    /// A list: every item, with the mark its kind wears.
    fn list(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let mut number = 0usize;
        let mut last: Option<usize> = None;
        let mut cursor = node.walk();
        for item in node.children(&mut cursor) {
            if item.kind() != "list_item" {
                continue;
            }
            number += 1;
            let marker = item
                .children(&mut item.walk())
                .find(|child| child.kind().starts_with("list_marker"));
            let said = match marker.map(|marker| marker.kind()) {
                Some("list_marker_dot" | "list_marker_parenthesis") => format!("{number}. "),
                _ => "\u{2022} ".to_string(),
            };
            let wide = u16::try_from(text_width(&said)).unwrap_or(2);
            let inside = prefix.and(vec![room(text_width(&said))], wide);
            // The mark itself, once, on whichever row the item draws first.
            let marks = prefix
                .rest
                .iter()
                .cloned()
                .chain([mark(said.clone())])
                .collect();
            self.owe(marks);
            if let Some(end) = last
                && self.parted(end, item.start_byte())
            {
                self.owed = None;
                self.row(prefix, Vec::new());
                self.owe(
                    prefix
                        .rest
                        .iter()
                        .cloned()
                        .chain([mark(said.clone())])
                        .collect(),
                );
            }
            self.blocks(item, width, &inside);
            last = Some(item.end_byte());
        }
    }

    /// A block of code, in the colours its own language would have.
    ///
    /// A box round it, the whole width the block has: a block of code set
    /// into prose is a thing on the page rather than part of it, and where
    /// it begins and ends is worth a line rather than a guess.
    ///
    /// Lines are broken at the width rather than at a space: a line of code
    /// has no words to respect, and breaking it anywhere else would put a
    /// space where the code has none.
    fn code(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let content = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "code_fence_content");
        // Whether Obelus has the grammar the fence names. Where it has not
        // -- a fence that says `ruby`, or says nothing -- the block keeps
        // the one colour that means "this is code" and nothing more, because
        // there is nobody to tell its parts apart.
        let named = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "info_string")
            .and_then(|info| self.source.get(info.byte_range()))
            .map(|said| said.trim().to_lowercase())
            .and_then(|said| LanguageId::for_name(&said))
            .is_some();
        let gathered = self.gather(content.unwrap_or(node), Join::Lines);
        let room = usize::from(prefix.room(width)).saturating_sub(2).max(1);

        self.row(prefix, vec![mark(across(room, true))]);
        for line in gathered.text.lines() {
            let at = offset_in(&gathered.text, line);
            let mut taken = 0usize;
            loop {
                let rest: String = line.chars().skip(taken).take(room).collect();
                let from = at + character_offset(line, taken);
                let mut spans = vec![mark("\u{2502}")];
                spans.extend(coloured(&rest, from, &gathered, &self.highlights, named));
                spans.push(room_for(room.saturating_sub(text_width(&rest))));
                spans.push(mark("\u{2502}"));
                self.row(prefix, spans);
                taken += room;
                if taken >= line.chars().count() {
                    break;
                }
            }
        }
        self.row(prefix, vec![mark(across(room, false))]);
    }

    /// A table, in columns that fit.
    fn table(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let mut rows: Vec<Vec<Gathered>> = Vec::new();
        let mut aligns: Vec<Align> = Vec::new();
        let mut cursor = node.walk();
        for line in node.children(&mut cursor) {
            match line.kind() {
                "pipe_table_header" | "pipe_table_row" => {
                    rows.push(
                        line.children(&mut line.walk())
                            .filter(|cell| cell.kind() == "pipe_table_cell")
                            .map(|cell| self.gather(cell, Join::Soft))
                            .collect(),
                    );
                }
                "pipe_table_delimiter_row" => {
                    aligns = line
                        .children(&mut line.walk())
                        .filter(|cell| cell.kind() == "pipe_table_delimiter_cell")
                        .map(|cell| {
                            let kinds: Vec<&str> = cell
                                .children(&mut cell.walk())
                                .map(|mark| mark.kind())
                                .collect();
                            match (
                                kinds.contains(&"pipe_table_align_left"),
                                kinds.contains(&"pipe_table_align_right"),
                            ) {
                                (true, true) => Align::Middle,
                                (false, true) => Align::Right,
                                _ => Align::Left,
                            }
                        })
                        .collect();
                }
                _ => {}
            }
        }
        let columns = rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(aligns.len());
        let wanted: Vec<Vec<usize>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| text_width(cell.text.trim()))
                    .collect()
            })
            .collect();
        let widths = table::widths(&wanted, columns, usize::from(prefix.room(width)));
        if widths.is_empty() {
            return;
        }
        let rule = |at: Rule| Row::of(vec![mark(rule_across(&widths, at))]);

        self.rows.push(rule(Rule::Top));
        for (index, row) in rows.iter().enumerate() {
            let mut spans = vec![mark("\u{2502}")];
            for (at, width) in widths.iter().enumerate() {
                let cell = row.get(at);
                let said = cell.map(|cell| cell.text.trim()).unwrap_or_default();
                let said: String = said.chars().take(*width).collect();
                let align = aligns.get(at).copied().unwrap_or(Align::Left);
                let (before, after) = padding(*width, text_width(&said), align);
                spans.push(room_for(before));
                if let Some(cell) = cell {
                    let at = offset_in(&cell.text, cell.text.trim());
                    spans.push(Span::from_source(
                        said.clone(),
                        Ink::Plain,
                        source_span(cell, at, said.len()),
                    ));
                } else {
                    spans.push(Span::new(said.clone(), Ink::Plain));
                }
                spans.push(room_for(after));
                spans.push(mark("\u{2502}"));
            }
            self.row(prefix, spans);
            if index == 0 {
                self.rows.push(rule(Rule::Middle));
            }
        }
        self.rows.push(rule(Rule::Bottom));
    }

    /// Puts one row down, with whatever goes down its left.
    ///
    /// A row with nothing on it carries only what would be *seen*: a
    /// quotation's bar, which is what says the quote goes on across the
    /// blank line inside it, and not a list's indent, which is blanks and
    /// would be a row of trailing spaces nobody asked for.
    fn row(&mut self, prefix: &Prefix, spans: Vec<Span>) {
        let mut row = self.owed.take().unwrap_or_else(|| prefix.rest.clone());
        if spans.is_empty() {
            row.retain(|span| !span.text.trim().is_empty());
        }
        row.extend(spans);
        self.rows.push(Row::of(row));
    }

    /// Leaves a mark for the next row to carry.
    fn owe(&mut self, marks: Vec<Span>) {
        self.owed = Some(marks);
    }

    /// The bytes a node is made of, with the markers of the blocks it is
    /// inside left out.
    ///
    /// A paragraph in a quote has the quote's marks running down the middle
    /// of it, and the paragraph has never heard of them: the tree keeps them
    /// as children of their own, so what is left when they are taken out is
    /// the author's text and nothing else.
    fn gather(&self, node: Node<'_>, join: Join) -> Gathered {
        let mut ranges = vec![node.byte_range()];
        take_out(node, &mut ranges);
        let mut text = String::new();
        let mut runs: Vec<(usize, usize, usize)> = Vec::new();
        for range in ranges {
            let Some(part) = self.source.get(range.clone()) else {
                continue;
            };
            let mut hard = false;
            for (index, line) in part.split('\n').enumerate() {
                if index > 0 {
                    text.push(match join {
                        // A newline inside a paragraph is a *soft* break: the
                        // text either side of it is one thing to lay out, and
                        // the break is a space. Unless the author asked for
                        // the break -- two spaces or a backslash at the end
                        // of a line -- in which case it is theirs and stays.
                        Join::Soft if !hard => ' ',
                        _ => '\n',
                    });
                }
                hard = join == Join::Soft && ends_hard(line);
                let line = match join {
                    // The marker is the asking, not the text.
                    Join::Soft => line.trim_end_matches([' ', '\\']).trim_start(),
                    Join::Lines => line,
                };
                if line.is_empty() {
                    continue;
                }
                runs.push((text.len(), range.start + offset_in(part, line), line.len()));
                text.push_str(line);
            }
        }
        Gathered { text, runs }
    }
}

/// Whether the breaks inside a gathered run are breaks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    /// A paragraph: they are soft, and become spaces.
    Soft,
    /// A block of code: they are the content, and stay.
    Lines,
}

/// A run of source, gathered, and where every part of it came from.
#[derive(Clone)]
struct Gathered {
    /// The text as it will be laid out.
    text: String,
    /// Where each run of it began in the source: its start here, its start
    /// there, and how long it is.
    runs: Vec<(usize, usize, usize)>,
}

impl Gathered {
    /// The runs this range is made of, as offsets into the range itself.
    ///
    /// A range of the gathered text can span several runs of the source --
    /// the lines of one paragraph -- and what is between them came from
    /// nobody's writing.
    fn runs_in(&self, range: Range<usize>) -> Vec<(usize, usize, usize)> {
        self.runs
            .iter()
            .filter_map(|(out, from, length)| {
                let start = (*out).max(range.start);
                let end = (out + length).min(range.end);
                (start < end).then(|| (start - range.start, from + (start - out), end - start))
            })
            .collect()
    }

    /// Where the source byte at `from` ended up in the gathered text.
    ///
    /// `None` for a byte that did not end up in it at all: the markers of
    /// the blocks it is inside were taken out on the way.
    fn text_of(&self, from: usize) -> Option<usize> {
        self.runs.iter().find_map(|(out, source, length)| {
            (from >= *source && from < source + length).then(|| out + (from - source))
        })
    }

    /// Where in the source the byte at `at` came from.
    fn source_of(&self, at: usize) -> Option<usize> {
        let next = self.runs.partition_point(|(out, _, _)| *out <= at);
        let (out, from, length) = *self.runs.get(next.checked_sub(1)?)?;
        (at < out + length).then_some(from + (at - out))
    }
}

/// Where a cell's text sits in the source, as far as it can be said.
fn source_span(cell: &Gathered, at: usize, length: usize) -> Range<usize> {
    let from = cell.source_of(at).unwrap_or(0);
    from..from + length
}

/// Every `block_continuation` under a node, taken out of its ranges.
fn take_out(node: Node<'_>, ranges: &mut Vec<Range<usize>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "block_continuation" {
            let gone = child.byte_range();
            *ranges = ranges
                .iter()
                .flat_map(|range| without(range.clone(), gone.clone()))
                .collect();
            continue;
        }
        take_out(child, ranges);
    }
}

/// One range with another taken out of it.
fn without(range: Range<usize>, gone: Range<usize>) -> Vec<Range<usize>> {
    if gone.end <= range.start || gone.start >= range.end {
        return vec![range];
    }
    let mut left = Vec::new();
    if range.start < gone.start {
        left.push(range.start..gone.start);
    }
    if gone.end < range.end {
        left.push(gone.end..range.end);
    }
    left
}

/// How a run of a paragraph is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Look {
    ink: Option<Ink>,
    bold: bool,
    italic: bool,
    strikeout: bool,
}

/// A paragraph with its markers taken out, and what each byte that is left
/// looks like.
///
/// The stars round a word in italics are the author asking for italics, not
/// words they wrote: a reading that drew them would be showing the request
/// beside the thing requested. So they go, and what they asked for is
/// carried by the run instead.
///
/// From the inline grammar rather than by looking for stars -- and from the
/// tree the document already has, rather than one parsed here. What a
/// paragraph is made of is a second language inside it, and Obelus parses
/// that for the buffer whether or not anybody is reading.
fn stripped(gathered: &Gathered, ink: Ink, inside: Option<Node<'_>>) -> (Gathered, Vec<Look>) {
    let plain = Look {
        ink: Some(ink),
        ..Look::default()
    };
    let mut looks = vec![plain; gathered.text.len()];
    let Some(inside) = inside else {
        return (gathered.clone(), looks);
    };
    let mut gone: Vec<Range<usize>> = Vec::new();
    marks(inside, gathered, &mut looks, &mut gone);
    gone.sort_by_key(|range| range.start);

    // What is left, in the order it was written, with where each run of it
    // came from carried through both hands it has passed by now.
    let mut text = String::new();
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut left: Vec<Look> = Vec::new();
    let mut at = 0usize;
    for range in gone
        .iter()
        .chain(std::iter::once(&(gathered.text.len()..0)))
    {
        let end = range.start.max(at).min(gathered.text.len());
        if at < end
            && let Some(part) = gathered.text.get(at..end)
        {
            // One run for each run it was gathered from, rather than one for
            // the whole of what is kept: two characters beside each other
            // here may be a line apart in the source, and a run that claimed
            // both would be claiming the break between them as well.
            runs.extend(
                gathered
                    .runs_in(at..end)
                    .into_iter()
                    .map(|(out, from, length)| (text.len() + out, from, length)),
            );
            text.push_str(part);
            left.extend(looks.get(at..end).unwrap_or_default().iter().copied());
        }
        at = at.max(range.end);
    }
    (Gathered { text, runs }, left)
}

/// What one inline node asks for, and what of it is not the asking.
///
/// The tree is the document's, so what it says is said in the source's own
/// bytes; the text being laid out has had the markers of whatever blocks it
/// is inside taken out of it already, so every range has to come back
/// through [`Gathered::text_of`]. A range that lands in one of those markers
/// is a range about something that is not on the page.
fn marks(node: Node<'_>, gathered: &Gathered, looks: &mut [Look], gone: &mut Vec<Range<usize>>) {
    let here = |range: Range<usize>| -> Option<Range<usize>> {
        let start = gathered.text_of(range.start)?;
        Some(start..start + (range.end - range.start))
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let Some(range) = here(child.byte_range()) else {
            marks(child, gathered, looks, gone);
            continue;
        };
        match child.kind() {
            "emphasis" => paint(looks, &range, |look| look.italic = true),
            "strong_emphasis" => paint(looks, &range, |look| look.bold = true),
            "strikethrough" => paint(looks, &range, |look| look.strikeout = true),
            "code_span" => paint(looks, &range, |look| look.ink = Some(Ink::Code)),
            // The marks themselves, whatever they asked for.
            "emphasis_delimiter" | "code_span_delimiter" => {
                gone.push(range);
                continue;
            }
            // A link is its words. Where it points is not on the page: a
            // reading is read, and a URL in the middle of a sentence is a
            // line the eye has to climb over.
            "inline_link" | "image" => {
                let words = child
                    .children(&mut child.walk())
                    .find(|part| part.kind() == "link_text")
                    .and_then(|part| here(part.byte_range()));
                match words {
                    Some(words) => {
                        gone.push(range.start..words.start);
                        gone.push(words.end..range.end);
                    }
                    None => gone.push(range),
                }
            }
            _ => {}
        }
        marks(child, gathered, looks, gone);
    }
}

/// Says something about every byte of a run.
fn paint(looks: &mut [Look], range: &Range<usize>, what: impl Fn(&mut Look)) {
    for look in looks.iter_mut().take(range.end).skip(range.start) {
        what(look);
    }
}

/// One row of a paragraph, split into runs that are drawn the same way.
fn split(said: &str, at: usize, looks: &[Look], gathered: &Gathered) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut run = String::new();
    let mut look: Option<Look> = None;
    let mut from = at;
    let mut byte = at;
    // Where the run would have to carry on from for it to be one run. A row
    // is laid out from text that was gathered out of the source, so two
    // characters beside each other here may be pages apart there -- and a
    // span says where it came from, which it cannot do for two places.
    let mut carries: Option<usize> = None;
    for character in said.chars() {
        let here = looks.get(byte).copied().unwrap_or_default();
        let source = gathered.source_of(byte);
        let broken = carries.is_some_and(|next| source != Some(next));
        if (look.is_some_and(|had| had != here) || broken) && !run.is_empty() {
            spans.push(spanned(&run, from, look.unwrap_or_default(), gathered));
            run = String::new();
            from = byte;
        }
        look = Some(here);
        run.push(character);
        byte += character.len_utf8();
        carries = source.map(|source| source + character.len_utf8());
    }
    if !run.is_empty() {
        spans.push(spanned(&run, from, look.unwrap_or_default(), gathered));
    }
    spans
}

/// One run of a row, with where it came from.
fn spanned(said: &str, at: usize, look: Look, gathered: &Gathered) -> Span {
    let ink = look.ink.unwrap_or(Ink::Plain);
    let mut span = match gathered.source_of(at) {
        Some(from) => Span::from_source(said.to_string(), ink, from..from + said.len()),
        None => Span::new(said.to_string(), ink),
    };
    span.bold = look.bold;
    span.italic = look.italic;
    span.strikeout = look.strikeout;
    span
}

/// One row of a block of code, split into runs of a single kind.
///
/// `named` is whether Obelus has the grammar the fence named. Where it has
/// not -- a fence that says `ruby`, or says nothing at all -- the whole row
/// keeps the one colour that means "this is code", because there is nobody
/// to tell its parts apart.
fn coloured(
    said: &str,
    at: usize,
    gathered: &Gathered,
    highlights: &Highlights,
    named: bool,
) -> Vec<Span> {
    if !named {
        return vec![match gathered.source_of(at) {
            Some(from) => Span::from_source(said.to_string(), Ink::Code, from..from + said.len()),
            None => Span::new(said.to_string(), Ink::Code),
        }];
    }
    let kind_at = |byte: usize| -> Option<SyntaxKind> {
        highlights.kind_at(ByteOffset::new(gathered.source_of(byte)?))
    };
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut kind: Option<Option<SyntaxKind>> = None;
    let mut from = at;
    let mut byte = at;
    for character in said.chars() {
        let here = kind_at(byte);
        if kind.is_some_and(|had| had != here) && !run.is_empty() {
            spans.push(inked(&run, from, kind.flatten(), gathered));
            run = String::new();
            from = byte;
        }
        kind = Some(here);
        run.push(character);
        byte += character.len_utf8();
    }
    if !run.is_empty() {
        spans.push(inked(&run, from, kind.flatten(), gathered));
    }
    spans
}

/// One run of code, in the ink its kind asks for.
fn inked(said: &str, at: usize, kind: Option<SyntaxKind>, gathered: &Gathered) -> Span {
    // A run its own grammar said nothing about -- the spaces between the
    // words of a language Obelus does parse -- is the plain colour. The
    // colour that means "code" is for a block nobody could read at all.
    let ink = kind.map_or(Ink::Plain, Ink::Syntax);
    match gathered.source_of(at) {
        Some(from) => Span::from_source(said.to_string(), ink, from..from + said.len()),
        None => Span::new(said.to_string(), ink),
    }
}

/// The top or the bottom of the box a block of code is drawn in.
fn across(inside: usize, top: bool) -> String {
    let (left, right) = match top {
        true => ('\u{250c}', '\u{2510}'),
        false => ('\u{2514}', '\u{2518}'),
    };
    let mut line = String::new();
    line.push(left);
    line.extend(std::iter::repeat_n('\u{2500}', inside));
    line.push(right);
    line
}

/// The cells a row does not reach.
fn room_for(cells: usize) -> Span {
    Span::new(" ".repeat(cells), Ink::Plain)
}

/// Which rule of a table this is, for its corners.
#[derive(Clone, Copy)]
enum Rule {
    Top,
    Middle,
    Bottom,
}

/// A table's horizontal rule, with the corners and crossings for where it
/// sits.
fn rule_across(widths: &[usize], at: Rule) -> String {
    let (left, join, right) = match at {
        Rule::Top => ('\u{250c}', '\u{252c}', '\u{2510}'),
        Rule::Middle => ('\u{251c}', '\u{253c}', '\u{2524}'),
        Rule::Bottom => ('\u{2514}', '\u{2534}', '\u{2518}'),
    };
    let mut line = String::new();
    line.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            line.push(join);
        }
        line.extend(std::iter::repeat_n('\u{2500}', *width));
    }
    line.push(right);
    line
}

/// Which way a column's cells are pushed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Middle,
    Right,
}

/// The blanks either side of a cell, for how it is aligned.
fn padding(width: usize, said: usize, align: Align) -> (usize, usize) {
    let spare = width.saturating_sub(said);
    match align {
        Align::Left => (0, spare),
        Align::Right => (spare, 0),
        Align::Middle => (spare / 2, spare - spare / 2),
    }
}

/// Where a part of a string begins in it, in bytes.
fn offset_in(whole: &str, part: &str) -> usize {
    (part.as_ptr() as usize).saturating_sub(whole.as_ptr() as usize)
}

/// Whether a line ends the way an author ends one on purpose.
///
/// Two spaces or a backslash: markdown's two ways of saying "the break at
/// the end of this line is mine, keep it".
fn ends_hard(line: &str) -> bool {
    line.ends_with("  ") || line.ends_with('\\')
}

/// Where a character offset into a line is, in bytes.
fn character_offset(line: &str, characters: usize) -> usize {
    line.char_indices()
        .nth(characters)
        .map_or(line.len(), |(byte, _)| byte)
}
