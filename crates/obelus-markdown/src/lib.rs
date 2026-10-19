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

/// What GitHub draws and the source only spells: HTML and shortcodes.
mod html;
/// How wide a table's columns are drawn.
mod table;

use std::ops::Range;

use obelus_row::{Code, Ink, Row, Span};
use obelus_syntax::{LanguageId, highlight::Highlights, parse::SyntaxState, tree_sitter::Node};
use obelus_text::{Text, coordinates::ByteOffset, kind::SyntaxKind, text_width};

/// Lays a markdown source out for a width.
///
/// Every row is at most `width` cells, so the caller never has to wrap.
/// Every `<details>` is open: this is for whatever has nowhere to open one,
/// and a heading that can never be opened would be words nobody can read.
#[must_use]
pub fn render(source: &str, width: u16) -> Vec<Row> {
    render_folding(source, width, &|_, _| true)
}

/// Lays a markdown source out for a width, asking `open` whether each
/// `<details>` is open: where it begins in the source, and whether it asks
/// to start open -- which a reader who has never touched it is owed.
///
/// A shut one is its heading and nothing else; the heading's row says which
/// it is ([`Row::details`]), for whoever turns it.
#[must_use]
pub fn render_folding(source: &str, width: u16, open: &dyn Fn(usize, bool) -> bool) -> Vec<Row> {
    // A closing fence is only a closing fence to the grammar with a line
    // break after it, and what an agent says ends where its last fence
    // does: the fence was drawn inside the box as a line of the code, and
    // copied with it. Put at the end, the break moves nothing before it, so
    // every place a row says it came from is still a place in what was
    // said.
    let ended;
    let source = match source.is_empty() || source.ends_with('\n') {
        true => source,
        false => {
            ended = format!("{source}\n");
            ended.as_str()
        }
    };
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
    let trees = |language: LanguageId| -> Vec<(Range<usize>, Node<'_>)> {
        injected
            .iter()
            .filter(|one| one.language() == language)
            .map(|one| {
                let root = one.tree().root_node();
                (root.byte_range(), root)
            })
            .collect()
    };
    let inline = trees(LanguageId::MarkdownInline);
    let mut laying = Laying {
        source,
        highlights,
        inline,
        quoted: 0,
        rows: Vec::new(),
        owed: None,
        open,
        details: Vec::new(),
    };
    laying.blocks(state.tree().root_node(), width, &Prefix::default());
    // One never closed holds the rest of the document, as GitHub has it.
    while !laying.details.is_empty() {
        laying.close_details();
    }
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
    /// How many quotes deep the block being laid out is.
    quoted: usize,
    rows: Vec<Row>,
    /// The marks the next row owes, in place of its own left margin.
    ///
    /// A bullet is drawn once and then stood under, and what stands under it
    /// is every row of every block in the item -- the paragraph, the block
    /// of code below it, the list inside that. Which of those draws the
    /// first row is not the bullet's business, so the bullet is left here
    /// and taken by whichever row comes first.
    owed: Option<Vec<Span>>,
    /// Whether each `<details>` is open, as whoever draws the rows says.
    open: &'a dyn Fn(usize, bool) -> bool,
    /// The `<details>` the rows being laid out are inside, outermost first.
    ///
    /// The document's, not a block's: what one holds is as often blocks of
    /// their own -- a fence, a list -- between the block of HTML that opens
    /// it and the one that closes it.
    details: Vec<Opened>,
}

/// A `<details>` being laid out.
struct Opened {
    /// Where it begins in the source.
    at: usize,
    /// Whether it is open.
    open: bool,
    /// Whether it asks to start open.
    asked_open: bool,
    /// How many rows there were once its heading was down: a shut one is
    /// laid out like an open one, and goes back to here when it closes.
    from: usize,
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
            let parted = last.is_some_and(|end| self.parted(end, child.start_byte()));
            if parted {
                self.row(prefix, Vec::new());
            }
            let drawn = self.rows.len();
            self.block(child, width, prefix);
            // Nor the line in front of it: a `</details>` alone in its block
            // left a blank line, and the next block another.
            if parted && self.rows.len() == drawn {
                self.rows.pop();
            }
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
        // The mark a list item is introduced by, and the box a task is,
        // which the item has already drawn: see [`Laying::list`].
        if node.kind().starts_with("list_marker") || node.kind().starts_with("task_list_marker") {
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
            "html_block" => self.html(node, width, prefix),
            "pipe_table" => self.table(node, width, prefix),
            "thematic_break" => self.rows.push(Row {
                spans: Vec::new(),
                rule: true,
                code: None,
                links: Vec::new(),
                details: None,
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
        self.said(inline.unwrap_or(node), width, prefix, self.words());
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
        let links = inside.map_or_else(Vec::new, |inside| links(inside, self.source));
        self.lay(
            &Shaped {
                shown,
                looks,
                links,
            },
            width,
            prefix,
        );
    }

    /// A block of HTML: what is left of it once its tags have gone, laid out
    /// as the words it is -- see [`html`].
    ///
    /// Its own tags rather than the inline grammar's, which is not asked to
    /// look inside a block it has said is HTML. A soft join, so a line break
    /// in the source is a space the way it is in a paragraph and in a page
    /// GitHub draws; the breaks are the tags'.
    fn html(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let gathered = self.gather(node, Join::Soft);
        let tags = html::tags(&gathered.text);
        // Cut where a `<details>` opens or closes and where its heading is,
        // and each piece between laid out as HTML is.
        let mut at = 0;
        let mut next = 0;
        while let Some(found) = tags
            .iter()
            .skip(next)
            .position(|tag| tag.name == "details" || tag.name == "summary")
        {
            let here = next + found;
            let tag = &tags[here];
            self.html_part(&gathered, &tags, at..tag.range.start, width, prefix);
            at = tag.range.end;
            next = here + 1;
            match (tag.name.as_str(), tag.closing) {
                ("details", false) => {
                    let source = gathered.source_of(tag.range.start).unwrap_or(0);
                    let default = tag.open;
                    let open = (self.open)(source, default);
                    // Its heading follows at once where it has one, and is
                    // the word GitHub puts there where it has none.
                    let summarised = tags.get(here + 1).is_some_and(|summary| {
                        summary.name == "summary"
                            && !summary.closing
                            && gathered
                                .text
                                .get(tag.range.end..summary.range.start)
                                .is_some_and(|between| between.trim().is_empty())
                    });
                    self.details.push(Opened {
                        at: source,
                        open,
                        asked_open: default,
                        from: self.rows.len(),
                    });
                    if !summarised {
                        let heading = Shaped {
                            shown: Gathered {
                                text: "Details".to_string(),
                                runs: Vec::new(),
                            },
                            looks: vec![Look::default(); "Details".len()],
                            links: Vec::new(),
                        };
                        self.summary(&heading, width, prefix);
                    }
                }
                ("details", true) => self.close_details(),
                ("summary", false) => {
                    let end = tags
                        .iter()
                        .enumerate()
                        .skip(here + 1)
                        .find(|(_, end)| end.name == "summary" && end.closing);
                    let words_end = end.map_or(gathered.text.len(), |(_, end)| end.range.start);
                    let heading = shaped(&gathered, at..words_end, &tags, self.words(), false);
                    self.summary(&heading, width, prefix);
                    if let Some((end_at, end)) = end {
                        at = end.range.end;
                        next = end_at + 1;
                    } else {
                        at = gathered.text.len();
                        next = tags.len();
                    }
                }
                // A `</summary>` with no heading open: nothing to end.
                _ => {}
            }
        }
        self.html_part(&gathered, &tags, at..gathered.text.len(), width, prefix);
    }

    /// The heading of the `<details>` just opened: its words, emphasised,
    /// and on its first row which one it is and whether it is open.
    fn summary(&mut self, heading: &Shaped, width: u16, prefix: &Prefix) {
        let Some(details) = self.details.last() else {
            return;
        };
        let (at, open, asked_open) = (details.at, details.open, details.asked_open);
        let first = self.rows.len();
        let heading = Shaped {
            shown: heading.shown.clone(),
            looks: heading
                .looks
                .iter()
                .map(|look| Look {
                    bold: true,
                    ..*look
                })
                .collect(),
            links: heading.links.clone(),
        };
        self.lay(&heading, width, prefix);
        if self.rows.len() == first {
            // A heading with no words is still the one thing to open it by.
            self.row(prefix, vec![Span::new("Details", self.words())]);
        }
        if let Some(row) = self.rows.get_mut(first) {
            row.details = Some(obelus_row::Details {
                at,
                open,
                asked_open,
            });
        }
        if let Some(details) = self.details.last_mut() {
            details.from = self.rows.len();
        }
    }

    /// Ends the innermost `<details>`: what a shut one holds goes.
    fn close_details(&mut self) {
        if let Some(details) = self.details.pop()
            && !details.open
        {
            self.rows.truncate(details.from);
        }
    }

    /// A piece of a block of HTML between the places a `<details>` cuts it.
    fn html_part(
        &mut self,
        gathered: &Gathered,
        tags: &[html::Tag],
        part: Range<usize>,
        width: u16,
        prefix: &Prefix,
    ) {
        if part.is_empty() {
            return;
        }
        let tags: Vec<html::Tag> = tags
            .iter()
            .filter(|tag| tag.range.start >= part.start && tag.range.end <= part.end)
            .cloned()
            .collect();
        // A table the block holds whole is drawn as a table, and the words
        // either side of it as words.
        let mut at = part.start;
        for (open, close) in html::tables(&tags) {
            let (Some(first), Some(last)) = (tags.get(open), tags.get(close)) else {
                continue;
            };
            let before = shaped(gathered, at..first.range.start, &tags, self.words(), true);
            self.lay(&before, width, prefix);
            let rows: Vec<Vec<Shaped>> = html::rows(&tags[open..=close])
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|cell| shaped(gathered, cell.words, &tags, self.words(), false))
                        .collect()
                })
                .collect();
            // Its first row is a heading's where every cell of it is one,
            // which is what a `<thead>` holds and what markdown's own first
            // row always is.
            let heading = html::rows(&tags[open..=close])
                .first()
                .is_some_and(|row| row.iter().all(|cell| cell.heading));
            self.grid(&rows, &[], heading, width, prefix);
            at = last.range.end;
        }
        let after = shaped(gathered, at..part.end, &tags, self.words(), true);
        self.lay(&after, width, prefix);
    }

    /// Words with what they look like, wrapped and put on rows, each row
    /// carrying the links whose words are on it.
    fn lay(&mut self, shaped: &Shaped, width: u16, prefix: &Prefix) {
        let Shaped {
            shown,
            looks,
            links,
        } = shaped;
        if shown.text.trim().is_empty() {
            return;
        }
        for (said, at) in obelus_text::wrapped_from(&shown.text, prefix.room(width)) {
            if said == RULE {
                self.rows.push(Row {
                    spans: Vec::new(),
                    rule: true,
                    code: None,
                    links: Vec::new(),
                    details: None,
                });
                continue;
            }
            let spans = split(&said, at.start, looks, shown);
            // A link's words may wrap, and every row of them goes where
            // the link does.
            let here: Vec<(Range<usize>, String)> = links
                .iter()
                .filter(|(words, _)| {
                    spans.iter().any(|span| {
                        span.from
                            .as_ref()
                            .is_some_and(|from| from.start < words.end && words.start < from.end)
                    })
                })
                .cloned()
                .collect();
            self.row(prefix, spans);
            if let Some(row) = self.rows.last_mut() {
                row.links = here;
            }
        }
    }

    /// A quoted block: the mark down its left, and whatever it holds.
    fn quote(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        // The mark on every row, not only the first: a quote is quoted all
        // the way down, and a reader skimming the left edge is reading that
        // column rather than counting rows.
        let inside = prefix.and(vec![mark("\u{2503} ")], 2);
        self.quoted += 1;
        self.blocks(node, width, &inside);
        self.quoted -= 1;
    }

    /// The ink words that ask for nothing are drawn in here.
    ///
    /// The page's own, and in a quote the one Obelus says the lesser things
    /// in: a comment that quotes the one before it is mostly the one before
    /// it, and with both in one colour nothing but the bar down the edge
    /// said which words were whose -- GitHub greys a quote for the same
    /// reason. What the words ask for is still worn over it: code is code
    /// and bold is bold in a quote too.
    const fn words(&self) -> Ink {
        match self.quoted {
            0 => Ink::Plain,
            _ => Ink::Aside,
        }
    }

    /// A list: every item, with the mark its kind wears.
    fn list(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let mut number: Option<u64> = None;
        let mut last: Option<usize> = None;
        let mut cursor = node.walk();
        for item in node.children(&mut cursor) {
            if item.kind() != "list_item" {
                continue;
            }
            let marker = item
                .children(&mut item.walk())
                .find(|child| child.kind().starts_with("list_marker"));
            // The first item's number is where the list starts, and the rest
            // are counted on from it whatever they were written as: an agent
            // that numbers one list on from the last, and then asks about
            // "7", means the row that says 7.
            let number = *number.insert(match number {
                Some(before) => before + 1,
                None => marker
                    .and_then(|marker| self.source.get(marker.byte_range()))
                    .and_then(|said| said.trim().trim_end_matches(['.', ')']).parse().ok())
                    .unwrap_or(1),
            });
            // A task is its box, where the bullet would be: GitHub's
            // `- [x]`, which every pull request's template is full of. Laid
            // out as a block of its own, the marker stood on a row by itself
            // and the words went on to the one under it.
            //
            // The box's plain characters and not the Nerd Font's, which a
            // switch elsewhere wears: a glyph of the font is two cells in a
            // terminal and wants a blank after it there and not in a window,
            // and rows are laid out here before anybody knows which.
            let task = item
                .children(&mut item.walk())
                .find_map(|child| match child.kind() {
                    "task_list_marker_checked" => Some(true),
                    "task_list_marker_unchecked" => Some(false),
                    _ => None,
                });
            let said = match (task, marker.map(|marker| marker.kind())) {
                (Some(true), _) => "\u{2611} ".to_string(),
                (Some(false), _) => "\u{25a1} ".to_string(),
                (None, Some("list_marker_dot" | "list_marker_parenthesis")) => {
                    format!("{number}. ")
                }
                (None, _) => "\u{2022} ".to_string(),
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
        // The newline before the closing fence ends the last line rather
        // than beginning another, and a copy that kept it would run a
        // command pasted into a shell before the reader had read it there.
        let code = Code {
            at: node.start_byte(),
            text: gathered
                .text
                .strip_suffix('\n')
                .unwrap_or(&gathered.text)
                .into(),
        };
        let first = self.rows.len();

        self.row(prefix, vec![mark(across(room, true))]);
        for line in gathered.text.lines() {
            let at = offset_in(&gathered.text, line);
            for piece in obelus_text::broken_at(line, room) {
                let rest = &line[piece.clone()];
                let mut spans = vec![mark("\u{2502}")];
                spans.extend(coloured(
                    rest,
                    at + piece.start,
                    &gathered,
                    &self.highlights,
                    named,
                ));
                spans.push(room_for(room.saturating_sub(text_width(rest))));
                spans.push(mark("\u{2502}"));
                self.row(prefix, spans);
            }
        }
        self.row(prefix, vec![mark(across(room, false))]);
        for row in &mut self.rows[first..] {
            row.code = Some(code.clone());
        }
    }

    /// A table, in columns that fit.
    fn table(&mut self, node: Node<'_>, width: u16, prefix: &Prefix) {
        let mut rows: Vec<Vec<Shaped>> = Vec::new();
        let mut aligns: Vec<Align> = Vec::new();
        let mut cursor = node.walk();
        for line in node.children(&mut cursor) {
            match line.kind() {
                "pipe_table_header" | "pipe_table_row" => {
                    rows.push(
                        line.children(&mut line.walk())
                            .filter(|cell| cell.kind() == "pipe_table_cell")
                            .map(|cell| self.cell(cell))
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
        self.grid(&rows, &aligns, true, width, prefix);
    }

    /// One cell of a table written in markdown: its words, read the way a
    /// paragraph's are -- the stars and backticks in it taken out and what
    /// they asked for worn, which a cell used to show as written.
    fn cell(&self, cell: Node<'_>) -> Shaped {
        let gathered = self.gather(cell, Join::Soft);
        let said = cell.byte_range();
        let inside = self
            .inline
            .iter()
            .find(|(range, _)| range.start >= said.start && range.end <= said.end)
            .map(|(_, root)| *root);
        let (shown, looks) = stripped(&gathered, self.words(), inside);
        let links = inside.map_or_else(Vec::new, |inside| links(inside, self.source));
        Shaped {
            shown,
            looks,
            links,
        }
    }

    /// A table: its cells in columns as wide as the room allows, in a box,
    /// with a rule under its first row where that row is the headings.
    ///
    /// One drawing for a table written in markdown and one written in HTML,
    /// because they are one thing on the page.
    fn grid(
        &mut self,
        rows: &[Vec<Shaped>],
        aligns: &[Align],
        heading: bool,
        width: u16,
        prefix: &Prefix,
    ) {
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
                    .map(|cell| text_width(cell.shown.text.trim()))
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
            // Every cell wrapped to its column, and the row as tall as its
            // tallest: a cell that does not fit goes on underneath, the way
            // a paragraph does, rather than losing its end at the border.
            // Where each line starts is kept as a place in the cell's own
            // words, which is what its looks are counted in.
            let wrapped: Vec<Vec<(String, usize)>> = widths
                .iter()
                .enumerate()
                .map(|(at, width)| {
                    let Some(cell) = row.get(at) else {
                        return Vec::new();
                    };
                    let said = cell.shown.text.trim();
                    let from = offset_in(&cell.shown.text, said);
                    obelus_text::wrapped_from(said, u16::try_from(*width).unwrap_or(u16::MAX))
                        .into_iter()
                        .map(|(line, at)| (line, from + at.start))
                        .collect()
                })
                .collect();
            let tall = wrapped.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for line in 0..tall {
                let mut spans = vec![mark("\u{2502}")];
                let mut links: Vec<(Range<usize>, String)> = Vec::new();
                for (at, width) in widths.iter().enumerate() {
                    let cell = row.get(at);
                    let (said, from) = wrapped[at].get(line).cloned().unwrap_or_default();
                    let align = aligns.get(at).copied().unwrap_or(Align::Left);
                    let (before, after) = padding(*width, text_width(&said), align);
                    spans.push(room_for(before));
                    match cell {
                        Some(cell) if !said.is_empty() => {
                            let words = split(&said, from, &cell.looks, &cell.shown);
                            links.extend(
                                cell.links
                                    .iter()
                                    .filter(|(on, _)| {
                                        words.iter().any(|span| {
                                            span.from.as_ref().is_some_and(|from| {
                                                from.start < on.end && on.start < from.end
                                            })
                                        })
                                    })
                                    .cloned(),
                            );
                            spans.extend(words);
                        }
                        _ => spans.push(Span::new(said, Ink::Plain)),
                    }
                    spans.push(room_for(after));
                    spans.push(mark("\u{2502}"));
                }
                self.row(prefix, spans);
                if let Some(row) = self.rows.last_mut() {
                    row.links = links;
                }
            }
            if index == 0 && heading {
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
    let mut gone: Vec<Range<usize>> = Vec::new();
    let mut put: Vec<(usize, String)> = Vec::new();
    let mut tags: Vec<html::Tag> = Vec::new();
    if let Some(inside) = inside {
        marks(inside, gathered, &mut looks, &mut gone, &mut put, &mut tags);
    }
    wear(&mut looks, &html::styles(&tags));
    spelled(gathered, &looks, &mut gone, &mut put);
    keep(gathered, &looks, gone, put)
}

/// Words ready to be laid out: what is left of them once their marks have
/// gone, what each byte looks like, and the links on them.
struct Shaped {
    /// The words.
    shown: Gathered,
    /// What each byte of them looks like.
    looks: Vec<Look>,
    /// The links among them, by where their words are in the source.
    links: Vec<(Range<usize>, String)>,
}

/// A stretch of a block of HTML -- the whole of it, or one cell of a table
/// in it -- as words: its tags taken out and what they ask for worn or put
/// in their place, its character references and shortcodes what they name.
///
/// `rules` is whether a `<hr>` may be a rule across the page, which in a
/// cell of a table it may not: a cell is a box of words, and the rule is a
/// break between them there.
fn shaped(
    gathered: &Gathered,
    range: Range<usize>,
    tags: &[html::Tag],
    ink: Ink,
    rules: bool,
) -> Shaped {
    let part = Gathered {
        text: gathered
            .text
            .get(range.clone())
            .unwrap_or_default()
            .to_string(),
        runs: gathered.runs_in(range.clone()),
    };
    // Its own tags, counted from where it starts.
    let tags: Vec<html::Tag> = tags
        .iter()
        .filter(|tag| tag.range.start >= range.start && tag.range.end <= range.end)
        .map(|tag| html::Tag {
            range: tag.range.start - range.start..tag.range.end - range.start,
            ..tag.clone()
        })
        .collect();
    let mut looks = vec![
        Look {
            ink: Some(ink),
            ..Look::default()
        };
        part.text.len()
    ];
    wear(&mut looks, &html::styles(&tags));
    let mut gone: Vec<Range<usize>> = Vec::new();
    let mut put: Vec<(usize, String)> = Vec::new();
    for tag in &tags {
        // A rule is a row of its own, which words cannot say: it is put as a
        // line holding nothing but the mark `lay` draws one for.
        if tag.is_rule() {
            let said: &[&str] = match rules {
                true => &["\n", RULE, "\n"],
                false => &["\n"],
            };
            for said in said {
                put.push((tag.range.start, (*said).to_string()));
            }
            gone.push(tag.range.clone());
            continue;
        }
        let (taken, leaves) = tag.taken();
        if let (Some(leaves), Some(first)) = (leaves, taken.first()) {
            put.push((first.start, leaves));
        }
        gone.extend(taken);
    }
    for (range, character) in html::references(&part.text) {
        if !gone
            .iter()
            .any(|tag| tag.start < range.end && range.start < tag.end)
        {
            put.push((range.start, character));
            gone.push(range);
        }
    }
    spelled(&part, &looks, &mut gone, &mut put);
    // Where a link's words are in the source, which is what a row says its
    // links are by.
    //
    // Narrowed to the bytes of them that have one: the words of a link
    // written over lines begin and end at a line break, which the join made
    // a space that is in no source -- and a link said to start there was
    // dropped, the badge under it and all.
    let links: Vec<(Range<usize>, String)> = html::links(&tags)
        .into_iter()
        .filter_map(|(words, to)| {
            let said = part.text.get(words.clone())?;
            let start = said
                .char_indices()
                .find_map(|(at, _)| part.source_of(words.start + at))?;
            let end = said.char_indices().rev().find_map(|(at, character)| {
                part.source_of(words.start + at)
                    .map(|from| from + character.len_utf8())
            })?;
            Some((start..end, to))
        })
        .collect();
    let (shown, looks) = keep(&part, &looks, gone, put);
    Shaped {
        shown,
        looks,
        links,
    }
}

/// What a line holding a rule and nothing else says while it is being laid
/// out: a character of the private use area, which no source has any
/// business holding, and which `lay` turns into the rule.
const RULE: &str = "\u{e000}";

/// The looks a block's or a paragraph's tags ask for, worn by the words
/// between each pair of them.
fn wear(looks: &mut [Look], styles: &[(Range<usize>, html::Style)]) {
    for (range, style) in styles {
        paint(looks, range, |look| match style {
            html::Style::Bold => look.bold = true,
            html::Style::Italic => look.italic = true,
            html::Style::Code => look.ink = Some(Ink::Code),
            html::Style::Struck => look.strikeout = true,
            html::Style::Heading(level) => look.ink = Some(Ink::Heading(*level)),
        });
    }
}

/// Every shortcode in the words that names an emoji, taken out and the
/// emoji put in its place -- but not in a span of code, where `:x:` is what
/// was written, nor over anything already taken out.
fn spelled(
    gathered: &Gathered,
    looks: &[Look],
    gone: &mut Vec<Range<usize>>,
    put: &mut Vec<(usize, String)>,
) {
    for (range, emoji) in html::shortcodes(&gathered.text) {
        let code = looks
            .get(range.start)
            .is_some_and(|look| look.ink == Some(Ink::Code));
        let taken = gone
            .iter()
            .any(|other| other.start < range.end && range.start < other.end);
        if !code && !taken {
            put.push((range.start, emoji.to_string()));
            gone.push(range);
        }
    }
}

/// What is left of gathered words once `gone` is taken out of them, with
/// what `put` puts where something was taken out -- and what each byte
/// that is left looks like.
///
/// What is put is in nobody's source, so it carries none: a row says where
/// its words came from, and an emoji came from a shortcode three times its
/// length. A break put in starts a line, so the spaces after it are not the
/// start of that line, and two breaks in a row are one: a `</td></tr>` is
/// one end of a line, not a blank line.
fn keep(
    gathered: &Gathered,
    looks: &[Look],
    mut gone: Vec<Range<usize>>,
    mut put: Vec<(usize, String)>,
) -> (Gathered, Vec<Look>) {
    gone.sort_by_key(|range| range.start);
    put.sort_by_key(|(at, _)| *at);

    // What is left, in the order it was written, with where each run of it
    // came from carried through both hands it has passed by now.
    let mut text = String::new();
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut left: Vec<Look> = Vec::new();
    let mut at = 0usize;
    // The start is the start of a line too: what a block of HTML opened
    // with is gone, and the space after it was its indent.
    let mut broke = true;
    for range in gone
        .iter()
        .chain(std::iter::once(&(gathered.text.len()..0)))
    {
        let end = range.start.max(at).min(gathered.text.len());
        let mut from = at;
        if broke {
            from += gathered
                .text
                .get(from..end)
                .map_or(0, |part| part.len() - part.trim_start().len());
        }
        if from < end
            && let Some(part) = gathered.text.get(from..end)
        {
            // One run for each run it was gathered from, rather than one for
            // the whole of what is kept: two characters beside each other
            // here may be a line apart in the source, and a run that claimed
            // both would be claiming the break between them as well.
            runs.extend(
                gathered
                    .runs_in(from..end)
                    .into_iter()
                    .map(|(out, source, length)| (text.len() + out, source, length)),
            );
            text.push_str(part);
            left.extend(looks.get(from..end).unwrap_or_default().iter().copied());
            broke = false;
        }
        for (_, said) in put.iter().filter(|(put_at, _)| *put_at == range.start) {
            // A space put between two cells of a table is not the start of a
            // line.
            if said == " " && (text.is_empty() || text.ends_with(['\n', ' '])) {
                continue;
            }
            if said == "\n" {
                let trimmed = text.trim_end_matches(' ').len();
                left.truncate(trimmed);
                text.truncate(trimmed);
                // And where they came from, or what is put after them would
                // say it came from the spaces taken off.
                runs.retain(|(out, ..)| *out < trimmed);
                if let Some((out, _, length)) = runs.last_mut() {
                    *length = (*length).min(trimmed - *out);
                }
                if text.is_empty() || text.ends_with('\n') {
                    continue;
                }
                broke = true;
            }
            let look = looks.get(range.start).copied().unwrap_or_default();
            left.extend(std::iter::repeat_n(look, said.len()));
            text.push_str(said);
        }
        at = at.max(range.end);
    }
    // A break the end of a block asked for, with nothing after it, is not a
    // line.
    while text.ends_with('\n') {
        text.pop();
        left.pop();
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
fn marks(
    node: Node<'_>,
    gathered: &Gathered,
    looks: &mut [Look],
    gone: &mut Vec<Range<usize>>,
    put: &mut Vec<(usize, String)>,
    tags: &mut Vec<html::Tag>,
) {
    let here = |range: Range<usize>| -> Option<Range<usize>> {
        let start = gathered.text_of(range.start)?;
        Some(start..start + (range.end - range.start))
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let Some(range) = here(child.byte_range()) else {
            marks(child, gathered, looks, gone, put, tags);
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
            // A tag of HTML in the middle of the words, which is the asking
            // too -- see [`html`] -- and leaves a break or a picture's words
            // where it asked for one.
            //
            // Only a name GitHub would keep: anything else is words, and
            // `Vec<String>` keeps its `<String>`.
            "html_tag" => {
                let Some(mut tag) = gathered.text.get(range.clone()).and_then(html::tag) else {
                    continue;
                };
                tag.range = range;
                let (taken, leaves) = tag.taken();
                if let (Some(leaves), Some(first)) = (leaves, taken.first()) {
                    put.push((first.start, leaves));
                }
                gone.extend(taken);
                tags.push(tag);
                continue;
            }
            // A link is its words. Where it points is not on the page: a
            // reading is read, and a URL in the middle of a sentence is a
            // line the eye has to climb over.
            "inline_link" | "image" => {
                let words = child
                    .children(&mut child.walk())
                    .find(|part| part.kind() == "link_text" || part.kind() == "image_description")
                    .and_then(|part| here(part.byte_range()));
                // A picture is its words behind the mark a picture has, the
                // same as one written in HTML.
                if child.kind() == "image" {
                    put.push((range.start, html::PICTURE.to_string()));
                }
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
        marks(child, gathered, looks, gone, put, tags);
    }
}

/// Every link under an inline node that goes somewhere on the web: where
/// its words are in the source, and where it goes.
///
/// Only an address with a scheme: a link to `src/lib.rs` is a file in a
/// repository the browser has never heard of.
fn links(node: Node<'_>, source: &str) -> Vec<(Range<usize>, String)> {
    // The tags of HTML among the words, in order: an `<a href>` and the
    // `</a>` after it are two of them, with the link's words between. Once,
    // for the whole of the words: asked at every level of the walk below,
    // a link inside `**` was found twice.
    let mut tags = Vec::new();
    gather_tags(node, source, &mut tags);
    let mut found = html::links(&tags);
    written_links(node, source, &mut found);
    found
}

/// The links markdown writes, `[words](to)` and `![words](to)`, under an
/// inline node.
fn written_links(node: Node<'_>, source: &str, found: &mut Vec<(Range<usize>, String)>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "inline_link" && child.kind() != "image" {
            written_links(child, source, found);
            continue;
        }
        let part = |kind: &str| {
            child
                .children(&mut child.walk())
                .find(|part| part.kind() == kind)
        };
        let to = part("link_destination")
            .and_then(|to| source.get(to.byte_range()))
            .map(|to| to.trim_start_matches('<').trim_end_matches('>'))
            .filter(|to| to.starts_with("https://") || to.starts_with("http://"));
        if let (Some(words), Some(to)) =
            (part("link_text").or_else(|| part("image_description")), to)
        {
            found.push((words.byte_range(), to.to_string()));
        }
    }
}

/// Every tag of HTML under an inline node, where it is in the source.
fn gather_tags(node: Node<'_>, source: &str, tags: &mut Vec<html::Tag>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "html_tag" {
            if let Some(mut tag) = source.get(child.byte_range()).and_then(html::tag) {
                tag.range = child.byte_range();
                tags.push(tag);
            }
            continue;
        }
        gather_tags(child, source, tags);
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
    //
    // Nor for a place and nowhere: the space a soft break becomes is in
    // nobody's source, and a run that started on it said nowhere for the
    // whole of the line that followed -- which could then not be selected.
    // So the last character's own answer is kept, nowhere included:
    // `Some(None)` is a run of nothing anybody wrote, and only more of
    // nothing carries it on.
    let mut carries: Option<Option<usize>> = None;
    for character in said.chars() {
        let here = looks.get(byte).copied().unwrap_or_default();
        let source = gathered.source_of(byte);
        let broken = carries.is_some_and(|next| source != next);
        if (look.is_some_and(|had| had != here) || broken) && !run.is_empty() {
            spans.push(spanned(&run, from, look.unwrap_or_default(), gathered));
            run = String::new();
            from = byte;
        }
        look = Some(here);
        run.push(character);
        byte += character.len_utf8();
        carries = Some(source.map(|source| source + character.len_utf8()));
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
