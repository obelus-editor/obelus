//! What is open, along the title bar: a tab for each document, and a press
//! on one goes to it.
//!
//! Where the reader has turned `bar_tabs` on: off, the bar has no room for
//! a row of tabs (`title::room`), and this is laid out in none.
//!
//! The application says what the tabs are -- `Drawing::tabs` -- and the
//! window says where they go, because only the window knows how wide a
//! name is in the face the bar is written in, and where on the bar the
//! system's own buttons stand: on the right on Windows, and on the left on
//! macOS, where the tabs start after them.
//!
//! More tabs than the bar has room for scroll, by the pixel, and slide to
//! where they are going rather than jumping there: a row that jumped a
//! tab's width under the pointer is a row the reader has to read again to
//! find their place in. So a tab may be cut by either end, the way the
//! system's own tabbed windows cut one -- what is cut is covered by the
//! end of the bar, where an arrow stands that walks the row on to the
//! next tab it is cutting. The wheel over the bar scrolls it too. The tab
//! being read is brought whole into sight whenever it changes: the reader
//! chose it, and a choice they cannot see is one they cannot check.
//!
//! A tab held down is dragged: it is drawn where the pointer has it, over
//! the others, and they slide aside as it passes their middles -- which is
//! when the application is told to put it there (`Strip::dropping`). Let
//! go of, it slides into the place it has been given. Carried to either
//! end of a row longer than the bar, or out past it, the row scrolls that
//! way under it -- faster the further out it is -- and what scrolls under
//! the pointer is passed as though the pointer had moved over it
//! (`Strip::edging`). Every tab slides to
//! its place when the order changes, whoever changed it: a tab that jumped
//! is a tab the reader has to find again, and the one they are carrying
//! is the order changing under their hand.
//!
//! Each tab has a cross at its end that closes it, drawn on the tab being
//! read and the one under the pointer and kept room for on every other:
//! a cross on every tab is a row of them, and one that came and went with
//! room of its own would push the name along under the pointer. It closes
//! when it is let go of where it was pressed, as a button does, rather
//! than as it is pressed, the way a tab is chosen: what a slip of the hand
//! takes away is the document and what was in it, and the press is said
//! to the application -- which asks first where that would lose anything
//! (`App::close_by_its_tab`).
//!
//! The slides are the window's own clock and none of the application's,
//! and they run only while something is somewhere other than where it is
//! going -- asked every frame from that (`Strip::sliding`), the way
//! everything that moves in the window is.

use std::{collections::HashMap, time::Instant};

use obelus_app::app::Tab;

/// What a place on the strip is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    /// A tab, by where it is in the list.
    Tab(usize),
    /// The cross at the end of one, which closes it.
    Shut(usize),
    /// The arrow that walks back towards the first tab.
    Earlier,
    /// And the one that walks on towards the last.
    Later,
}

/// Where on the bar the tabs may go, in pixels from the window's left, and
/// how big their names are written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Room {
    /// Where the strip starts: after the icon's place on Windows, after the
    /// three buttons on macOS.
    pub(crate) from: f32,
    /// And where it ends: before the buttons on Windows, a margin short of
    /// the edge on macOS.
    pub(crate) to: f32,
    /// How tall the bar is.
    pub(crate) titled: f32,
    /// How big a name is written.
    pub(crate) words: f32,
}

impl Room {
    /// How far a name stands in from either side of its tab.
    pub(crate) fn padding(&self) -> f32 {
        self.words.round()
    }

    /// How round a tab's corners are, the two at its top and the two that
    /// bend out into the bar at its foot.
    pub(crate) fn corner(&self) -> f32 {
        (self.words * 0.6).round()
    }

    /// How narrow a tab may be: a short name is still a tab to aim at, and
    /// a row of them all one width reads as a row of tabs rather than of
    /// words.
    fn narrowest(&self) -> f32 {
        (self.words * 9.0).round()
    }

    /// How big a tab's mark is drawn: a little bigger than the words, which
    /// is how a mark beside a name looks the same size as it.
    pub(crate) fn mark(&self) -> f32 {
        (self.words * 1.2).round()
    }

    /// How much of a tab its mark takes, with the room between it and the
    /// name.
    pub(crate) fn marked(&self) -> f32 {
        self.mark() + (self.words * 0.6).round()
    }

    /// How wide the cross at a tab's end is, and the room for a press on
    /// it.
    pub(crate) fn shut(&self) -> f32 {
        (self.words * 1.4).round()
    }

    /// And how wide, so that one long name does not take the bar.
    fn widest(&self) -> f32 {
        (self.words * 20.0).round()
    }

    /// How wide each arrow is.
    fn arrow(&self) -> f32 {
        (self.words * 2.4).round()
    }

    /// Whether there is room for a row at all: one of the narrowest tabs
    /// and an arrow either side. Less, and the ends cross -- a window
    /// dragged that narrow has its arrows drawn over each other.
    fn holds_a_tab(&self) -> bool {
        self.to - self.from >= self.narrowest() + 2.0 * self.arrow()
    }

    /// How far a notch of the wheel scrolls the row.
    fn notch(&self) -> f32 {
        (self.words * 6.0).round()
    }
}

/// One tab, placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Laid {
    /// Which tab, by where it is in the list.
    pub(crate) index: usize,
    /// Where its left edge is, in pixels from the window's left -- which
    /// may be before the row's start, for a tab the start cuts.
    pub(crate) left: f32,
    /// How wide it is, all of it.
    pub(crate) width: f32,
}

impl Laid {
    /// Where the cross that closes it is, as left and width: at its end,
    /// half a padding in from its edge.
    pub(crate) fn shut(&self, room: &Room) -> [f32; 2] {
        let width = room.shut();
        [self.left + self.width - room.padding() / 2.0 - width, width]
    }
}

/// How long the slide takes to come most of the way: the gap left after
/// this long is a third of what it was. Short, because what the reader is
/// waiting on is the tab they are going to, not the motion.
const SLIDE: f32 = 0.06;

/// The tabs, and what the window has made of them.
#[derive(Debug, Default)]
pub(crate) struct Strip {
    tabs: Vec<Tab>,
    /// How wide each tab is, from the last laying out.
    widths: Vec<f32>,
    /// How far along the row is scrolled, in pixels: where it is now, on
    /// its way to `target`.
    offset: f32,
    /// And where it is going.
    target: f32,
    /// When the slide last stepped, so the next step is as long as the time
    /// between them. Nothing while the row is still.
    stepped: Option<Instant>,
    /// Whether the tab being read has to be brought into sight at the next
    /// laying out, because it changed.
    reveal: bool,
    /// Where each tab with any of it in sight went, at the last laying out.
    laid: Vec<Laid>,
    /// Where the row is shown, from and to: the room less the arrows,
    /// where there are arrows.
    view: (f32, f32),
    /// Where the two arrows are, as left and width, while there are more
    /// tabs than room.
    arrows: Option<([f32; 2], [f32; 2])>,
    room: Option<Room>,
    /// Where the pointer is along the bar, while it is over it.
    pointer: Option<f32>,
    /// Whether a pane is over the bar -- see `cover`.
    covered: bool,
    /// What it is over, asked again of every laying out: the row moves
    /// under a pointer that does not -- the wheel, an arrow's slide, a tab
    /// opened from the keys -- and what a press there is on is what is
    /// under it on the screen, not what was when it last moved.
    hovered: Option<Spot>,
    /// Where each tab is drawn along the row, unscrolled, on its way to
    /// its place: by document, because a tab is still itself when the
    /// order changes, and its place is what moved.
    shown: HashMap<obelus_buffer::DocumentId, f32>,
    /// When the tabs last stepped towards their places, while any of them
    /// is on its way.
    settled: Option<Instant>,
    /// The tab held down, while it is.
    held: Option<Held>,
    /// Whether the tabs have been told in another order since they were
    /// last laid out -- see `told`.
    stale: bool,
    /// Where the tab carried was last said to go, until the tabs come back
    /// in another order: a drag finds the same place on every move of the
    /// pointer and every frame of a scroll until the application has
    /// answered, and each saying of it was a frame of the application's.
    asked: Option<(obelus_buffer::DocumentId, obelus_buffer::DocumentId, bool)>,
    /// When the row last scrolled under a tab carried to one of its ends,
    /// while it is scrolling.
    edged: Option<Instant>,
    /// The tab whose cross is pressed, until the button comes up.
    shutting: Option<obelus_buffer::DocumentId>,
}

/// How far into the row from either end a tab carried there starts it
/// scrolling, in arrows' widths: the arrow itself and as much again, so
/// that it starts before the pointer is over the arrow.
const EDGE: f32 = 2.0;

/// How fast the row scrolls under a tab carried right out past an end, in
/// the words' size a second: about a tab a quarter of a second.
const EDGE_SPEED: f32 = 40.0;

/// The strip as one frame paints it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painted {
    pub(crate) room: Room,
    /// Each tab with any of it in sight.
    pub(crate) tabs: Vec<Shown>,
    /// Where the row is shown, from and to, where a tab may be cut by
    /// either end: what is past them is covered by the bar again.
    pub(crate) cut: Option<(f32, f32)>,
    /// The two arrows, where they are shown: where, whether there is
    /// anything that way, and whether the pointer is over it.
    pub(crate) arrows: Vec<([f32; 2], bool, bool, Spot)>,
    /// Whether the window has the reader's keys.
    pub(crate) active: bool,
}

/// One tab as a frame paints it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    /// What it says.
    pub(crate) said: String,
    /// The mark before it, where marks are drawn.
    pub(crate) icon: Option<char>,
    /// Where it is.
    pub(crate) laid: Laid,
    /// Whether it is the one being read.
    pub(crate) reading: bool,
    /// Whether it is the one under the pointer.
    pub(crate) hovered: bool,
    /// Whether it is held, and dragged: drawn over every other tab.
    pub(crate) lifted: bool,
    /// Whether its cross is drawn.
    pub(crate) crossed: bool,
    /// Whether the pointer is over its cross.
    pub(crate) on_the_cross: bool,
    /// Whether its cross is pressed, and the pointer still over it.
    pub(crate) pressing: bool,
}

/// A tab held down, and where the pointer has it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    /// Which document's tab.
    document: obelus_buffer::DocumentId,
    /// How far into the tab the press was, so that it stays under the
    /// pointer at the place it was taken hold of rather than jumping its
    /// left edge there.
    grab: f32,
    /// Where the pointer is along the bar.
    x: f32,
    /// Where it was pressed.
    from: f32,
    /// Whether it has gone far enough from there to be a drag -- see
    /// `DRAG`.
    moved: bool,
}

/// How far a tab held has to be carried before it is being dragged, in the
/// words' size: half a letter. Until then it is a press -- one on a tab
/// cut by an end, to bring it into sight, is the commonest there is, and
/// a hand that does not move is a press however long it is held. Taken
/// for a drag, it scrolled the row from under the reader and could put
/// the tab somewhere else.
const DRAG: f32 = 0.5;

/// What a tab says: its name, and a mark after it where the file has
/// changes that are not on disk -- the mark the list of what is open puts
/// on the same row when no glyphs are drawn.
fn said(tab: &Tab) -> String {
    match tab.unwritten {
        true => format!("{} \u{2022}", tab.name),
        false => tab.name.clone(),
    }
}

impl Strip {
    /// What the application says is open now.
    pub(crate) fn told(&mut self, tabs: Vec<Tab>) {
        let reading = |tabs: &[Tab]| tabs.iter().find(|tab| tab.reading).map(|tab| tab.document);
        if reading(&tabs) != reading(&self.tabs) || tabs.len() != self.tabs.len() {
            self.reveal = true;
        }
        // Where each tab went is by where it is in the list, so a list in
        // another order makes every place the last laying out found a
        // place for some other tab -- and a drag that asked about one
        // before the next laying out would put its tab beside the wrong
        // one. Nothing is anywhere until then. Said rather than done by
        // emptying the places: an empty row is also what a first laying
        // out looks like, and the reveal does not slide on a first one,
        // so opening a tab made the row jump to it.
        if !tabs
            .iter()
            .map(|tab| tab.document)
            .eq(self.tabs.iter().map(|tab| tab.document))
        {
            self.stale = true;
            self.hovered = None;
            self.asked = None;
        }
        // A tab carried whose document has gone -- closed by a key the
        // other hand pressed, or by an agent -- is carried no longer: what
        // it held is not on the bar to put anywhere.
        if self
            .held
            .is_some_and(|held| !tabs.iter().any(|tab| tab.document == held.document))
        {
            self.held = None;
        }
        if self
            .shutting
            .is_some_and(|shutting| !tabs.iter().any(|tab| tab.document == shutting))
        {
            self.shutting = None;
        }
        self.tabs = tabs;
    }

    /// Lays the tabs out in the room there is, measuring each name with
    /// `wide`, and moves the slide on to `now` -- all of the way, where
    /// things do not move. No room -- a window in full screen, or one
    /// whose bar is the compositor's -- is no strip.
    pub(crate) fn lay(
        &mut self,
        room: Option<Room>,
        mut wide: impl FnMut(&str, f32) -> f32,
        now: Instant,
        animates: bool,
    ) {
        let room = room.filter(Room::holds_a_tab);
        // A screen of another density, which measures everything again in
        // its own pixels: the row stays where it was along itself, rather
        // than the same number of pixels along a row of another width.
        if let (Some(was), Some(now)) = (self.room, room)
            && was.words > 0.0
            && (now.words - was.words).abs() > f32::EPSILON
        {
            let scale = now.words / was.words;
            self.offset *= scale;
            self.target *= scale;
            for along in self.shown.values_mut() {
                *along *= scale;
            }
        }
        self.room = room;
        self.stale = false;
        let Some(room) = room else {
            self.laid.clear();
            self.arrows = None;
            // Arrived, with nothing to show the arriving on: a slide left
            // under way would keep `sliding` true, and the window drawing
            // frames for it, for as long as there is no bar -- the whole
            // of a stay in full screen. And the same for the tabs' own
            // slides, a tab held -- whose button may come up while there
            // is no bar to let go of it on -- and a row scrolling under it.
            self.offset = self.target;
            self.stepped = None;
            self.settled = None;
            self.edged = None;
            self.held = None;
            self.shutting = None;
            return;
        };
        self.widths = self
            .tabs
            .iter()
            .map(|tab| {
                let marked = match tab.icon {
                    Some(_) => room.marked(),
                    None => 0.0,
                };
                (wide(&said(tab), room.words).ceil() + marked + 2.0 * room.padding() + room.shut())
                    .clamp(room.narrowest(), room.widest())
            })
            .collect();
        let total: f32 = self.widths.iter().sum();
        if total <= room.to - room.from {
            self.view = (room.from, room.to);
            self.arrows = None;
        } else {
            let arrow = room.arrow();
            self.view = (room.from + arrow, room.to - arrow);
            self.arrows = Some(([room.from, arrow], [room.to - arrow, arrow]));
        }
        if std::mem::take(&mut self.reveal)
            && let Some(keep) = self.tabs.iter().position(|tab| tab.reading)
        {
            let (start, end) = self.edges(keep);
            let span = self.span();
            let was = self.target;
            if start < self.target {
                self.target = start;
            } else if end > self.target + span {
                self.target = end - span;
            }
            // Slid there like any other move, from now -- a row still since
            // the last move has no step to count from, and would jump. Not
            // on the first laying out, where there is no row yet to slide.
            if self.target != was && !self.laid.is_empty() {
                self.stepped.get_or_insert(now);
            }
        }
        self.target = self.target.clamp(0.0, self.most());
        self.edge(now);
        self.slide(now, animates);
        let (start, end) = self.view;
        let mut x = start - self.offset;
        self.laid.clear();
        for (index, &width) in self.widths.iter().enumerate() {
            if x + width > start && x < end {
                self.laid.push(Laid {
                    index,
                    left: x,
                    width,
                });
            }
            x += width;
        }
        self.settle(now, animates);
        self.hovered = self.pointer.and_then(|x| self.at(x));
    }

    /// Scrolls the row under a tab carried to one of its ends, by as much as
    /// the time since the last frame says -- straight there, not slid: the
    /// pointer is what is moving it, and it follows the pointer.
    fn edge(&mut self, now: Instant) {
        let Some(speed) = self.edging() else {
            self.edged = None;
            return;
        };
        let since = self.edged.map_or(0.0, |since| {
            now.saturating_duration_since(since).as_secs_f32()
        });
        self.target = (self.target + speed * since).clamp(0.0, self.most());
        self.offset = self.target;
        self.stepped = None;
        self.edged = Some(now);
    }

    /// How fast the row scrolls under the tab carried, in pixels a second
    /// and signed the way it goes: nothing unless one is carried to an end
    /// of a row with more than there is room for, and nothing once the row
    /// is as far that way as it goes -- or the window would draw frames for
    /// a row that cannot move.
    fn edging(&self) -> Option<f32> {
        let (held, room) = (self.dragged()?, self.room?);
        if self.arrows.is_none() || self.covered {
            return None;
        }
        let zone = EDGE * room.arrow();
        let (start, end) = self.view;
        // How far into the zone, from nothing at its inner edge to all of
        // it an arrow's width past the outer one.
        let into = |depth: f32| (depth / zone).clamp(0.0, 1.0);
        let fastest = EDGE_SPEED * room.words;
        let speed = match () {
            () if held.x < start - room.arrow() + zone => {
                -fastest * into(start - room.arrow() + zone - held.x)
            }
            () if held.x > end + room.arrow() - zone => {
                fastest * into(held.x - (end + room.arrow() - zone))
            }
            () => return None,
        };
        let open = match speed < 0.0 {
            true => self.target > 0.5,
            false => self.target < self.most() - 0.5,
        };
        (speed != 0.0 && open).then_some(speed)
    }

    /// Moves every tab on towards its place, as the row's own slide moves,
    /// and the one held to where the pointer has it.
    fn settle(&mut self, now: Instant, animates: bool) {
        let since = self
            .settled
            .map(|since| now.saturating_duration_since(since).as_secs_f32());
        let held = self
            .held
            .filter(|held| held.moved)
            .and_then(|held| Some((held.document, self.lifted_at(held)?)));
        let mut shown = HashMap::with_capacity(self.tabs.len());
        let mut moving = false;
        for (index, tab) in self.tabs.iter().enumerate() {
            let place = self.edges(index).0;
            let was = self.shown.get(&tab.document).copied();
            let at = match (held, was) {
                (Some((document, along)), _) if document == tab.document => along,
                // From where it was last drawn, which is where the eye has
                // it -- by as much of the way as the time since the last
                // step says, or not at all where the clock has only now
                // started: a tab put somewhere new since the last frame.
                (_, Some(was)) if animates => match since {
                    Some(since) => place + (was - place) * (-since / SLIDE).exp(),
                    None => was,
                },
                _ => place,
            };
            // The last half pixel is arriving, as the row's slide says.
            let at = match (at - place).abs() < 0.5 {
                true => place,
                false => at,
            };
            if at != place && held.is_none_or(|(document, _)| document != tab.document) {
                moving = true;
            }
            shown.insert(tab.document, at);
        }
        self.shown = shown;
        self.settled = moving.then_some(now);
    }

    /// Where the tab held is along the row, unscrolled: under the pointer,
    /// at the place it was taken hold of -- and no further than either end
    /// of the row, which is where a tab dragged out past them waits.
    fn lifted_at(&self, held: Held) -> Option<f32> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.document == held.document)?;
        let width = self.widths.get(index).copied()?;
        let along = held.x - held.grab - self.view.0 + self.offset;
        let total: f32 = self.widths.iter().sum();
        Some(along.clamp(0.0, (total - width).max(0.0)))
    }

    /// Takes hold of a tab, under the pointer, which then drags it until
    /// it is let go of.
    pub(crate) fn hold(&mut self, index: usize) {
        let (Some(x), Some(laid), Some(tab)) = (
            self.pointer,
            self.laid.iter().find(|laid| laid.index == index),
            self.tabs.get(index),
        ) else {
            return;
        };
        self.held = Some(Held {
            document: tab.document,
            grab: x - laid.left,
            x,
            from: x,
            moved: false,
        });
    }

    /// The document whose tab is held, while one is.
    pub(crate) fn holding(&self) -> Option<obelus_buffer::DocumentId> {
        self.dragged().map(|held| held.document)
    }

    /// The tab held, once it is being dragged.
    fn dragged(&self) -> Option<Held> {
        self.held.filter(|held| held.moved)
    }

    /// Where the tab carried goes, with the pointer where it is now: asked
    /// when the pointer moves, and on every frame the row scrolls under a
    /// pointer that does not. Only somewhere not already said -- see
    /// `asked`.
    pub(crate) fn carried(
        &mut self,
    ) -> Option<(obelus_buffer::DocumentId, obelus_buffer::DocumentId, bool)> {
        let held = self.dragged()?;
        let (beside, after) = self.dropping(held.document, held.x)?;
        let going = (held.document, beside, after);
        if self.asked == Some(going) {
            return None;
        }
        self.asked = Some(going);
        Some(going)
    }

    /// Drags the tab held to where the pointer is now. Whether one is
    /// held, which is whether there is anything new to draw.
    pub(crate) fn drag(&mut self, x: f32) -> bool {
        let far = self.room.map_or(0.0, |room| DRAG * room.words);
        let Some(held) = self.held.as_mut() else {
            return false;
        };
        held.x = x;
        let was = held.moved;
        held.moved |= (x - held.from).abs() >= far;
        held.moved || was
    }

    /// Lets go of the tab held, which slides from where it was dropped
    /// into the place it was given. Whether one was held.
    pub(crate) fn let_go(&mut self) -> bool {
        let was = self.held.take().is_some();
        if was {
            // From now, so that the first frame after it is already on its
            // way: with no clock a tab stays where it was last drawn for a
            // frame, which here is a tab that hangs where it was dropped.
            self.settled = Some(Instant::now());
        }
        was
    }

    /// Moves the row on towards where it is going, by as much of the way
    /// as the time since the last step says.
    fn slide(&mut self, now: Instant, animates: bool) {
        let gap = self.offset - self.target;
        let step = |since: Instant| now.saturating_duration_since(since).as_secs_f32();
        self.offset = match (animates, self.stepped) {
            (true, Some(since)) if gap.abs() >= 0.5 => {
                self.target + gap * (-step(since) / SLIDE).exp()
            }
            _ => self.target,
        };
        // The last half pixel is arriving, not sliding: a curve that only
        // ever comes most of the way would keep the window drawing.
        if !self.scrolling() {
            self.offset = self.target;
        }
        self.offset = self.offset.clamp(0.0, self.most());
        self.stepped = self.scrolling().then_some(now);
    }

    /// Whether the row is still on its way somewhere: what keeps the window
    /// drawing frames for it.
    pub(crate) fn sliding(&self) -> bool {
        self.scrolling() || self.settled.is_some() || self.edging().is_some()
    }

    /// Whether the row itself is on its way somewhere, which is the slide
    /// the arrows and the wheel start.
    fn scrolling(&self) -> bool {
        (self.offset - self.target).abs() >= 0.5
    }

    /// How wide the room the row is shown in is.
    fn span(&self) -> f32 {
        self.view.1 - self.view.0
    }

    /// The furthest the row may be scrolled: where the last tab ends at
    /// the end of the room.
    fn most(&self) -> f32 {
        (self.widths.iter().sum::<f32>() - self.span()).max(0.0)
    }

    /// Where a tab starts and ends along the row, unscrolled.
    fn edges(&self, index: usize) -> (f32, f32) {
        let start: f32 = self.widths.iter().take(index).sum();
        (
            start,
            start + self.widths.get(index).copied().unwrap_or(0.0),
        )
    }

    /// Starts the row towards `target`, from where it is now. Whether that
    /// is anywhere new.
    fn go_to(&mut self, target: f32) -> bool {
        let target = target.clamp(0.0, self.most());
        if (target - self.target).abs() < 0.5 {
            return false;
        }
        self.target = target;
        // Counted from the press, not from whenever the last frame was:
        // a row still since a minute ago would otherwise arrive at once.
        self.stepped = Some(Instant::now());
        true
    }

    /// Says whether a pane runs up under the bar, which puts the tabs
    /// under its glass with the rest of the page (`Painter::tabs_on_the_bar`).
    /// Nothing on the bar is theirs while it is there: a press, the pointer
    /// and the wheel go to the bar, as they do where there are no tabs --
    /// a press on a tab nobody can see was the file it named taking the
    /// place of the settings.
    ///
    /// What the pointer is over is asked again here and not left to the
    /// next laying out: a press that arrives between the pane and the
    /// frame that draws it reads `hovered`, and would choose the tab.
    pub(crate) fn cover(&mut self, covered: bool) {
        self.covered = covered;
        self.hovered = self.pointer.and_then(|x| self.at(x));
    }

    /// What is at a place on the bar, in pixels from the window's left.
    pub(crate) fn at(&self, x: f32) -> Option<Spot> {
        if self.covered || self.stale {
            return None;
        }
        let inside = |[left, width]: [f32; 2]| (left..left + width).contains(&x);
        if let Some((earlier, later)) = self.arrows {
            if inside(earlier) {
                return Some(Spot::Earlier);
            }
            if inside(later) {
                return Some(Spot::Later);
            }
        }
        // Only where the row is shown: the part of a cut tab past the end
        // is under the arrow, and a press there is the arrow's.
        if !(self.view.0..self.view.1).contains(&x) {
            return None;
        }
        let room = self.room?;
        self.laid
            .iter()
            .find(|laid| inside([laid.left, laid.width]))
            .map(|laid| match inside(laid.shut(&room)) {
                true => Spot::Shut(laid.index),
                false => Spot::Tab(laid.index),
            })
    }

    /// The cross on a tab pressed: nothing is closed until it is let go of
    /// over the same cross -- see `shut`.
    pub(crate) fn press_the_cross(&mut self, index: usize) {
        self.shutting = self.document(index);
    }

    /// The button come up: the document whose cross it went down on, where
    /// it came up over that cross too. Nothing otherwise -- dragged off the
    /// cross before letting go, the way a button anywhere is given up on.
    pub(crate) fn shut(&mut self) -> Option<obelus_buffer::DocumentId> {
        let shutting = self.shutting.take()?;
        match self.hovered {
            Some(Spot::Shut(index)) => self.document(index).filter(|&over| over == shutting),
            _ => None,
        }
    }

    /// Which document a tab is.
    pub(crate) fn document(&self, index: usize) -> Option<obelus_buffer::DocumentId> {
        self.tabs.get(index).map(|tab| tab.document)
    }

    /// Where a tab being dragged goes, with the pointer at `x`: beside
    /// which tab, and whether after it -- or nowhere, where it is where it
    /// should be.
    ///
    /// Past the middle of the tab it is dragged onto, not onto its edge:
    /// tabs are as wide as their names, so a narrow tab moved past a wide
    /// one would land with the wide one under the pointer, and the next
    /// move would put it back. The middle is somewhere the pointer has
    /// already crossed from either side.
    ///
    /// Over an arrow or out past an end, by the end of the row in sight:
    /// the row scrolls there, and what it brings past the end is what the
    /// tab is being carried past.
    pub(crate) fn dropping(
        &self,
        dragged: obelus_buffer::DocumentId,
        x: f32,
    ) -> Option<(obelus_buffer::DocumentId, bool)> {
        if self.covered || self.stale {
            return None;
        }
        let from = self.tabs.iter().position(|tab| tab.document == dragged)?;
        let x = x.clamp(self.view.0, (self.view.1 - 1.0).max(self.view.0));
        let laid = self
            .laid
            .iter()
            .find(|laid| (laid.left..laid.left + laid.width).contains(&x))?;
        let over = laid.index;
        let middle = laid.left + laid.width / 2.0;
        let after = over > from;
        let past = match after {
            true => x > middle,
            false => x < middle,
        };
        (over != from && past).then(|| (self.tabs[over].document, after))
    }

    /// Where on the bar the strip has something, from its start to the end
    /// of whatever is last on it -- which is where a press is the strip's
    /// and not the bar's to drag.
    pub(crate) fn covers(&self) -> Option<(f32, f32)> {
        let room = self.room.filter(|_| !self.covered)?;
        let end = match self.arrows {
            Some((_, [left, width])) => left + width,
            None => self.laid.last().map(|laid| laid.left + laid.width)?,
        };
        Some((room.from, end))
    }

    /// Scrolls the row by some notches of the wheel, on towards the last
    /// tab where it is more than none. Whether that moved it.
    pub(crate) fn scroll(&mut self, notches: f32) -> bool {
        let Some(room) = self.room.filter(|_| self.arrows.is_some() && !self.covered) else {
            return false;
        };
        self.go_to(self.target + notches * room.notch())
    }

    /// Walks the row to the next tab one of its ends is cutting, or hiding,
    /// in the way an arrow points: that tab then whole at that end.
    pub(crate) fn step(&mut self, towards: Spot) -> bool {
        if self.arrows.is_none() || self.covered {
            return false;
        }
        let span = self.span();
        let edges: Vec<(f32, f32)> = (0..self.widths.len()).map(|at| self.edges(at)).collect();
        let target = match towards {
            Spot::Earlier => edges
                .iter()
                .rev()
                .find(|(start, _)| *start < self.target - 0.5)
                .map(|(start, _)| *start),
            Spot::Later => edges
                .iter()
                .find(|(_, end)| *end > self.target + span + 0.5)
                .map(|(_, end)| end - span),
            Spot::Tab(_) | Spot::Shut(_) => None,
        };
        target.is_some_and(|target| self.go_to(target))
    }

    /// What the pointer is over, which is what a press there is on.
    pub(crate) const fn hovered(&self) -> Option<Spot> {
        self.hovered
    }

    /// Says where the pointer is along the bar, or that it is not over
    /// it. Whether that changed what it is over, which is what is drawn.
    pub(crate) fn point(&mut self, x: Option<f32>) -> bool {
        self.pointer = x;
        let spot = x.and_then(|x| self.at(x));
        let changed = self.hovered != spot;
        self.hovered = spot;
        changed
    }

    /// The strip as this frame paints it.
    ///
    /// Nothing while nothing is open -- the welcome screen, a start that
    /// asks which project: the bar's shade is the ground the tabs stand on,
    /// and drawn under none it is an empty row of tabs across the top of a
    /// page that has nothing to put in it.
    pub(crate) fn painted(&self, active: bool) -> Option<Painted> {
        let room = self.room.filter(|_| !self.tabs.is_empty())?;
        let held = self.holding();
        let start = self.view.0 - self.offset;
        let mut tabs: Vec<Shown> = self
            .laid
            .iter()
            .filter_map(|laid| {
                let tab = self.tabs.get(laid.index)?;
                let along = self.shown.get(&tab.document).copied();
                // Nothing lit while a tab is carried: what is under the
                // pointer is the tab in the reader's hand.
                let over = held.is_none().then_some(self.hovered).flatten();
                let on_the_cross = over == Some(Spot::Shut(laid.index));
                let hovered = on_the_cross || over == Some(Spot::Tab(laid.index));
                Some(Shown {
                    said: said(tab),
                    icon: tab.icon,
                    laid: Laid {
                        left: along.map_or(laid.left, |along| start + along),
                        ..*laid
                    },
                    reading: tab.reading,
                    hovered,
                    lifted: held == Some(tab.document),
                    crossed: tab.reading || hovered,
                    on_the_cross,
                    pressing: on_the_cross && self.shutting == Some(tab.document),
                })
            })
            .collect();
        // Left to right as they are drawn, so that a seam is between two
        // neighbours on the screen -- and the one carried last, over every
        // other.
        tabs.sort_by(|one, other| {
            one.lifted
                .cmp(&other.lifted)
                .then(one.laid.left.total_cmp(&other.laid.left))
        });
        // Whether there is anything that way, by where the row is going:
        // an arrow that went dim only once the slide had arrived would say
        // the reader could press it again for the whole of the slide.
        let arrows = self
            .arrows
            .map(|(earlier, later)| {
                vec![
                    (
                        earlier,
                        self.target > 0.5,
                        self.hovered == Some(Spot::Earlier),
                        Spot::Earlier,
                    ),
                    (
                        later,
                        self.target < self.most() - 0.5,
                        self.hovered == Some(Spot::Later),
                        Spot::Later,
                    ),
                ]
            })
            .unwrap_or_default();
        Some(Painted {
            room,
            tabs,
            cut: self.arrows.map(|_| self.view),
            arrows,
            active,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn tab(document: usize, reading: bool) -> Tab {
        Tab {
            document: obelus_buffer::DocumentId::new(document),
            name: "ab".to_string(),
            reading,
            icon: None,
            unwritten: false,
        }
    }

    /// A bar 650 across with words of 10: arrows of 24, and no tab
    /// narrower than 90 -- which every short name here is.
    fn room() -> Room {
        Room {
            from: 0.0,
            to: 650.0,
            titled: 30.0,
            words: 10.0,
        }
    }

    /// Ten tabs on that bar, the one being read as given, laid out still.
    fn strip(reading: usize) -> Strip {
        let mut strip = Strip::default();
        strip.told((0..10).map(|at| tab(at, at == reading)).collect());
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        strip
    }

    /// Lays the strip out again, the slide all the way there.
    fn settle(strip: &mut Strip) {
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
    }

    /// Tabs that fit are laid one after another from where the bar starts,
    /// with no arrows and nothing cut.
    ///
    /// Deliberate break: lay them an arrow's width in whether or not they
    /// fit. The first tab then stands away from where the bar begins,
    /// beside an arrow that is not drawn.
    #[test]
    fn tabs_that_fit_are_laid_from_the_start() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        assert_eq!(strip.arrows, None);
        assert_eq!(
            strip.laid.iter().map(|laid| laid.left).collect::<Vec<_>>(),
            [0.0, 90.0, 180.0]
        );
        assert_eq!(strip.painted(true).and_then(|painted| painted.cut), None);
    }

    /// The tab being read is brought whole into sight, at whichever end of
    /// the row it was hidden past.
    ///
    /// Deliberate break: leave the reveal out of `lay`. The tab being read
    /// is the tenth, the row stays at its start, and it is off the end.
    #[test]
    fn the_tab_being_read_is_brought_into_sight() {
        let strip = strip(9);
        // Nine hundred of tabs in 602 of room: the last ends at the end.
        assert_eq!(strip.target, 900.0 - 602.0);
        let last = strip.laid.last().expect("a tab");
        assert_eq!((last.index, last.left + last.width), (9, 626.0));
        let first = strip.laid.first().expect("a tab");
        assert!(first.left < 24.0, "the first in sight is cut by the start");
    }

    /// A tab cut by an end is in sight, and a press is its only where the
    /// row is shown: past the end it is the arrow's.
    ///
    /// Deliberate break: leave the arrows out of `at`. A press on one is
    /// then a press on the tab under it, or on the bar.
    #[test]
    fn a_press_finds_what_is_under_it() {
        let strip = strip(0);
        assert_eq!(strip.at(5.0), Some(Spot::Earlier));
        assert_eq!(strip.at(30.0), Some(Spot::Tab(0)));
        // The seventh tab runs from 564 to 654, past the room's end at 626.
        assert_eq!(strip.at(600.0), Some(Spot::Tab(6)));
        assert_eq!(strip.at(640.0), Some(Spot::Later));
        assert!(
            strip.laid.iter().any(|laid| laid.index == 6),
            "cut, and in sight"
        );
    }

    /// The end of a tab is its cross, and the rest of it is the tab.
    ///
    /// Deliberate break: leave the cross out of `at`. A press on it is then
    /// a press on the tab, which chooses it and closes nothing.
    #[test]
    fn a_press_at_a_tab_s_end_is_on_its_cross() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        settle(&mut strip);
        // A tab of 90, its cross 14 wide and 5 in from its end: 71 to 85.
        assert_eq!(strip.at(60.0), Some(Spot::Tab(0)));
        assert_eq!(strip.at(80.0), Some(Spot::Shut(0)));
        assert_eq!(strip.at(170.0), Some(Spot::Shut(1)));
        assert_eq!(strip.at(178.0), Some(Spot::Tab(1)), "the end's padding");
    }

    /// A cross closes its tab when the button comes up over it, and over
    /// nothing else: not the rest of the tab, not another tab's cross.
    ///
    /// Deliberate break: hand back what was pressed in `shut` whatever is
    /// under the pointer. A press dragged off the cross to take it back
    /// then closes the tab anyway.
    #[test]
    fn a_cross_closes_only_where_it_is_let_go_of() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        settle(&mut strip);
        let second = obelus_buffer::DocumentId::new(1);

        strip.point(Some(170.0));
        strip.press_the_cross(1);
        assert_eq!(strip.shut(), Some(second));
        assert_eq!(strip.shut(), None, "said once");

        strip.press_the_cross(1);
        strip.point(Some(150.0));
        assert_eq!(strip.shut(), None, "let go of on the tab");

        strip.press_the_cross(1);
        strip.point(Some(260.0));
        assert_eq!(strip.at(260.0), Some(Spot::Shut(2)));
        assert_eq!(strip.shut(), None, "let go of on another cross");
    }

    /// The cross is drawn on the tab being read and the one under the
    /// pointer, and on no other; pressed, it says so only while the
    /// pointer is still on it.
    ///
    /// Deliberate break: draw every tab's cross in `painted`. The third
    /// tab, neither read nor under the pointer, wears one.
    #[test]
    fn a_cross_is_drawn_where_it_is_wanted() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        settle(&mut strip);
        strip.point(Some(170.0));
        strip.press_the_cross(1);
        let crosses = |strip: &Strip| {
            strip
                .painted(true)
                .expect("painted")
                .tabs
                .iter()
                .map(|shown| (shown.crossed, shown.on_the_cross, shown.pressing))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            crosses(&strip),
            [
                (true, false, false),
                (true, true, true),
                (false, false, false)
            ]
        );
        strip.point(Some(150.0));
        assert_eq!(
            crosses(&strip),
            [
                (true, false, false),
                (true, false, false),
                (false, false, false)
            ]
        );
    }

    /// An arrow walks the row on until the tab its end was cutting is
    /// whole, and the wheel scrolls by the pixel -- neither past either
    /// end.
    ///
    /// Deliberate break: step to the tab after the one being cut. A row
    /// with a tab half in sight walks past it, and the reader never sees
    /// it whole.
    #[test]
    fn an_arrow_brings_the_cut_tab_whole() {
        let mut strip = strip(0);
        assert!(strip.step(Spot::Later));
        settle(&mut strip);
        // The seventh tab, which the end cut, now ends at it.
        let seventh = strip
            .laid
            .iter()
            .find(|laid| laid.index == 6)
            .expect("in sight");
        assert_eq!(seventh.left + seventh.width, 626.0);
        assert!(strip.step(Spot::Earlier));
        settle(&mut strip);
        assert_eq!(strip.target, 0.0, "back to the first, whole");
        assert!(!strip.step(Spot::Earlier), "nothing before the first");
        assert!(strip.scroll(100.0));
        settle(&mut strip);
        assert_eq!(strip.target, strip.most(), "the wheel stops at the end");
        assert!(!strip.scroll(1.0));
    }

    /// What the pointer is over is the tab under it on the screen, after
    /// the row has moved under a pointer that has not.
    ///
    /// Deliberate break: leave the last line out of `lay`. The wheel moves
    /// the row, the lit tab goes with it, and a press without a move opens
    /// the tab that slid away rather than the one under the pointer.
    #[test]
    fn a_pointer_that_has_not_moved_is_over_what_is_under_it_now() {
        let mut strip = strip(0);
        strip.point(Some(30.0));
        assert_eq!(strip.hovered(), Some(Spot::Tab(0)));
        assert!(strip.scroll(1.0));
        settle(&mut strip);
        // Sixty along: the first tab's last thirty and the second's first.
        assert_eq!(strip.hovered(), Some(Spot::Tab(0)));
        assert!(strip.scroll(1.0));
        settle(&mut strip);
        assert_eq!(
            strip.hovered(),
            Some(Spot::Tab(1)),
            "the next, slid under it"
        );
    }

    /// What a press on the bar is the strip's is from its start to the end
    /// of what is last on it: the last tab where they fit, the arrow where
    /// they do not. Nothing where the window is too narrow for a tab.
    ///
    /// Deliberate breaks: answer `covers` with the room's whole width, and
    /// the bar past the last tab is the page's and cannot drag the window;
    /// leave `holds_a_tab` out of `lay`, and a window narrower than a tab
    /// and two arrows draws them over each other.
    #[test]
    fn the_strip_covers_what_is_on_it() {
        let mut few = Strip::default();
        few.told((0..3).map(|at| tab(at, at == 0)).collect());
        few.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        assert_eq!(few.covers(), Some((0.0, 270.0)), "to the last tab");
        assert_eq!(strip(0).covers(), Some((0.0, 650.0)), "to the arrow's end");
        let narrow = Room {
            to: 120.0,
            ..room()
        };
        few.lay(Some(narrow), |_, _| 4.0, Instant::now(), false);
        assert_eq!(few.covers(), None, "no room for a tab");
    }

    /// With nothing open there is nothing to paint, not an empty bar.
    ///
    /// Deliberate break: leave the filter out of `painted`. The strip is
    /// painted with no tabs in it, which is the bar's shade drawn across
    /// the top of the welcome screen.
    #[test]
    fn nothing_open_paints_no_strip() {
        let mut strip = Strip::default();
        strip.told(Vec::new());
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        assert_eq!(strip.painted(true), None);
        strip.told(vec![tab(0, true)]);
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        assert!(strip.painted(true).is_some(), "a tab, and the bar under it");
    }

    /// Under a pane nothing on the bar is the tabs': not a press, not the
    /// pointer, not the wheel -- and they are still painted, under its
    /// glass, and theirs again when it goes.
    ///
    /// Deliberate breaks: leave `covered` out of `at`, and a press over a
    /// tab under the settings opens its file in their place; leave the
    /// pointer out of `cover`, and so does one that arrives before the
    /// frame that draws them; out of `covers`, and on Windows the bar over
    /// a covered tab will not drag the window; out of `scroll` or `step`,
    /// and the wheel or an arrow moves a row nobody sees.
    #[test]
    fn a_pane_over_the_bar_takes_it_from_the_tabs() {
        let mut strip = strip(0);
        strip.point(Some(30.0));
        assert_eq!(strip.hovered(), Some(Spot::Tab(0)));
        strip.cover(true);
        assert_eq!(strip.hovered(), None, "nothing lit, before any frame");
        settle(&mut strip);
        assert_eq!(strip.hovered(), None, "nothing lit under the pane");
        assert_eq!(strip.at(30.0), None);
        assert_eq!(strip.at(5.0), None, "nor the arrows");
        assert_eq!(strip.covers(), None);
        assert!(!strip.scroll(1.0));
        assert!(!strip.step(Spot::Later));
        assert!(strip.painted(true).is_some(), "still painted, under it");
        strip.cover(false);
        settle(&mut strip);
        assert_eq!(strip.hovered(), Some(Spot::Tab(0)), "theirs again");
        assert!(strip.scroll(1.0));
    }

    /// Over a different density the row is as far along itself as it was.
    ///
    /// Deliberate break: leave the scaling out of `lay`. Twice the density
    /// is twice the pixels, and the row is left half as far along as it
    /// was, with the tab being read part way off the end.
    #[test]
    fn another_density_keeps_the_row_where_it_was() {
        let mut strip = strip(9);
        let was = strip.target;
        let doubled = Room {
            to: 1300.0,
            words: 20.0,
            ..room()
        };
        strip.lay(Some(doubled), |_, _| 8.0, Instant::now(), false);
        assert_eq!(strip.target, was * 2.0);
    }

    /// The row slides to where it is going, a frame at a time, and stops
    /// there; where things do not move, it is there at once.
    ///
    /// Deliberate break: set `offset` to `target` in `slide` whatever
    /// `animates` says. The row then jumps a tab's width under the
    /// reader, which is what this is for.
    #[test]
    fn the_row_slides_and_arrives() {
        let mut strip = strip(0);
        assert!(strip.step(Spot::Later));
        let start = Instant::now();
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_millis(16),
            true,
        );
        assert!(strip.sliding(), "on its way");
        assert!(strip.offset > 0.0 && strip.offset < strip.target);
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_secs(2),
            true,
        );
        assert!(!strip.sliding(), "arrived");
        assert_eq!(strip.offset, strip.target);
    }

    /// A tab chosen where the end cuts it slides into sight, as an arrow's
    /// step does; and a bar that goes in the middle of a slide ends it.
    ///
    /// Deliberate breaks: leave `stepped` unset in the reveal, and the row
    /// jumps a tab's width under the pointer that chose the tab; leave the
    /// arrival out of `lay`'s nowhere, and a window gone full screen in
    /// the middle of a slide draws frames for it until it comes back --
    /// and leave `settled` or `held` alone there, and so does one gone full
    /// screen while the tabs slide to a new order, or while one is held.
    #[test]
    fn a_tab_chosen_slides_into_sight_and_no_bar_ends_the_slide() {
        let mut chosen = strip(0);
        chosen.told((0..10).map(|at| tab(at, at == 6)).collect());
        chosen.lay(Some(room()), |_, _| 4.0, Instant::now(), true);
        assert!(chosen.sliding(), "jumped to the tab chosen");
        chosen.lay(None, |_, _| 4.0, Instant::now(), true);
        assert!(!chosen.sliding(), "still sliding with no bar");

        let mut moved = strip(0);
        moved.told(
            [1, 0]
                .into_iter()
                .chain(2..10)
                .map(|at| tab(at, at == 0))
                .collect(),
        );
        moved.lay(Some(room()), |_, _| 4.0, Instant::now(), true);
        assert!(moved.sliding(), "the tabs jumped to their new order");
        moved.lay(None, |_, _| 4.0, Instant::now(), true);
        assert!(!moved.sliding(), "the tabs still sliding with no bar");

        let mut held = strip(0);
        held.point(Some(30.0));
        held.hold(0);
        held.lay(None, |_, _| 4.0, Instant::now(), true);
        held.let_go();
        assert!(!held.sliding(), "let go of with no bar, and sliding");
    }

    /// The tab being read is brought into sight when it changes, and only
    /// then: a reader who scrolled the row to look along it is not thrown
    /// back every frame.
    ///
    /// Deliberate break: set `reveal` on every `told`. The row jumps back
    /// to the tab being read on the next frame after every notch.
    #[test]
    fn only_a_change_brings_the_tab_back() {
        let mut strip = strip(9);
        assert!(strip.scroll(-100.0));
        strip.told((0..10).map(|at| tab(at, at == 9)).collect());
        settle(&mut strip);
        assert_eq!(strip.target, 0.0, "the same tab, told again");
        strip.told((0..10).map(|at| tab(at, at == 8)).collect());
        settle(&mut strip);
        assert_eq!(strip.target, 810.0 - 602.0, "another");
    }

    /// Three tabs on that bar, the one named `wide` as wide as a tab may
    /// be and the others as narrow, in the order given.
    fn uneven(order: &[usize]) -> Strip {
        let mut strip = Strip::default();
        strip.told(
            order
                .iter()
                .map(|&at| Tab {
                    name: match at {
                        1 => "wide".to_string(),
                        _ => "ab".to_string(),
                    },
                    ..tab(at, at == 0)
                })
                .collect(),
        );
        lay_uneven(&mut strip);
        strip
    }

    fn lay_uneven(strip: &mut Strip) {
        strip.lay(
            Some(room()),
            |said, _| if said == "wide" { 180.0 } else { 4.0 },
            Instant::now(),
            false,
        );
    }

    /// A dragged tab goes past another once the pointer is past the middle
    /// of it, and not as soon as it is over its edge.
    ///
    /// Deliberate break: answer as soon as the pointer is over another tab,
    /// in `Strip::dropping`. The narrow tab is put after the wide one at
    /// 100 -- and then, the wide one being under the pointer where the
    /// narrow one was, the next move puts it back.
    #[test]
    fn a_dragged_tab_goes_past_the_middle_of_another() {
        let strip = uneven(&[0, 1, 2]);
        let id = obelus_buffer::DocumentId::new;
        // 0 is 0 to 90, the wide 1 is 90 to 290, and 2 is 290 to 380.
        assert_eq!(strip.dropping(id(0), 100.0), None, "over its edge");
        assert_eq!(strip.dropping(id(0), 200.0), Some((id(1), true)));
        assert_eq!(strip.dropping(id(2), 150.0), Some((id(1), false)));
        assert_eq!(strip.dropping(id(0), 40.0), None, "over itself");
        // And put there, still past the middle: it stays.
        let moved = uneven(&[1, 0, 2]);
        assert_eq!(moved.dropping(id(0), 200.0), None, "moved back");
    }

    /// Told the tabs in another order, the strip has nowhere for any of
    /// them until it lays them out again.
    ///
    /// Deliberate break: leave `stale` unset in `told`. The places from
    /// the last laying out are another order's, so a drag of the last tab
    /// at 150 is over where the wide one was second and is now first: it
    /// is put before the narrow one that took its place, which the pointer
    /// is nowhere near.
    #[test]
    fn a_new_order_has_nowhere_until_it_is_laid() {
        let mut strip = uneven(&[0, 1, 2]);
        let id = obelus_buffer::DocumentId::new;
        let wide = Tab {
            name: "wide".to_string(),
            ..tab(1, false)
        };
        strip.told(vec![wide, tab(0, true), tab(2, false)]);
        assert_eq!(strip.dropping(id(2), 150.0), None, "a stale place");
        lay_uneven(&mut strip);
        // Laid: the wide one is 0 to 200, its middle 100.
        assert_eq!(strip.dropping(id(2), 150.0), None);
        assert_eq!(strip.dropping(id(2), 80.0), Some((id(1), false)));
    }

    /// Where a tab is drawn this frame, by its document.
    fn drawn_at(strip: &Strip, document: usize) -> Option<(f32, bool)> {
        strip.painted(true)?.tabs.into_iter().find_map(|shown| {
            let tab = strip.tabs.get(shown.laid.index)?;
            (tab.document == obelus_buffer::DocumentId::new(document))
                .then_some((shown.laid.left, shown.lifted))
        })
    }

    /// A tab held down is drawn where the pointer has it, at the place it
    /// was taken hold of, and over every other tab; let go of, it slides
    /// back into its place rather than jumping there.
    ///
    /// Deliberate breaks: leave the held tab at its place in `settle`, and
    /// it stays put under a pointer that has moved on; leave out the
    /// sorting in `painted`, and it is drawn before the tab it is over;
    /// and leave `settled` unset in `let_go`, and it hangs for a frame
    /// where it was dropped before it starts home.
    #[test]
    fn a_held_tab_follows_the_pointer_and_slides_home() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        let start = Instant::now();
        strip.lay(Some(room()), |_, _| 4.0, start, true);
        strip.point(Some(10.0));
        strip.hold(0);
        assert!(strip.drag(150.0));
        strip.lay(Some(room()), |_, _| 4.0, start, true);
        assert_eq!(drawn_at(&strip, 0), Some((140.0, true)));
        let painted = strip.painted(true).expect("a strip");
        assert!(
            painted.tabs.last().is_some_and(|shown| shown.lifted),
            "not drawn over the others"
        );
        assert!(!strip.sliding(), "nothing but the hand is moving it");

        assert!(strip.let_go());
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            Instant::now() + Duration::from_millis(30),
            true,
        );
        let (left, lifted) = drawn_at(&strip, 0).expect("drawn");
        assert!(!lifted, "still carried");
        assert!(left > 0.0 && left < 140.0, "jumped home: at {left}");
        assert!(strip.sliding(), "the window was not told to keep drawing");
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            Instant::now() + Duration::from_secs(2),
            true,
        );
        assert_eq!(drawn_at(&strip, 0), Some((0.0, false)));
        assert!(!strip.sliding(), "arrived");
    }

    /// Told a new order, a tab slides from where it was to its new place,
    /// and where nothing animates it is simply there.
    ///
    /// Deliberate break: draw every tab at its place in `settle`. The tab
    /// passed over jumps a tab's width the frame the order changes.
    #[test]
    fn a_tab_slides_to_a_new_place() {
        let mut strip = Strip::default();
        strip.told((0..3).map(|at| tab(at, at == 0)).collect());
        let start = Instant::now();
        strip.lay(Some(room()), |_, _| 4.0, start, true);
        strip.told(vec![tab(1, false), tab(0, true), tab(2, false)]);
        strip.lay(Some(room()), |_, _| 4.0, start, true);
        assert_eq!(drawn_at(&strip, 1), Some((90.0, false)), "jumped");
        assert!(strip.sliding());
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_millis(30),
            true,
        );
        let left = drawn_at(&strip, 1).expect("drawn").0;
        assert!(left > 0.0 && left < 90.0, "not on its way: at {left}");

        let mut still = Strip::default();
        still.told((0..3).map(|at| tab(at, at == 0)).collect());
        still.lay(Some(room()), |_, _| 4.0, start, false);
        still.told(vec![tab(1, false), tab(0, true), tab(2, false)]);
        still.lay(Some(room()), |_, _| 4.0, start, false);
        assert_eq!(drawn_at(&still, 1), Some((0.0, false)));
        assert!(!still.sliding());
    }

    /// A tab carried to the end of a row longer than the bar scrolls the
    /// row that way, and is put past what scrolls under it; brought back
    /// in, the row stops, and at the row's end it stops by itself.
    ///
    /// Deliberate breaks: leave `edge` out of `lay`, and the row stays
    /// where it was however long the tab is held at its end; leave the
    /// clamping out of `dropping`, and a tab carried out past the bar is
    /// put nowhere while the row scrolls under it; leave the row's end out of
    /// `edging`, and the window draws frames for a row that cannot move.
    #[test]
    fn a_tab_carried_to_an_end_scrolls_the_row() {
        let mut strip = strip(0);
        let start = Instant::now();
        assert_eq!(strip.target, 0.0);
        strip.point(Some(30.0));
        strip.hold(0);
        // Right out past the end of the bar, as a hand carrying a tab
        // somewhere goes: over no tab and no arrow.
        strip.drag(760.0);
        strip.lay(Some(room()), |_, _| 4.0, start, true);
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_millis(100),
            true,
        );
        assert!(strip.target > 0.0, "the row did not scroll");
        assert!(strip.sliding(), "the window was not told to keep drawing");
        // Frame by frame, as a window draws them: what scrolls past the
        // end under the pointer is passed, without the pointer moving.
        let carried = (1..60).find_map(|frame| {
            strip.lay(
                Some(room()),
                |_, _| 4.0,
                start + Duration::from_millis(100 + 16 * frame),
                true,
            );
            strip.carried()
        });
        let (document, _, after) = carried.expect("put nowhere");
        assert_eq!(document, obelus_buffer::DocumentId::new(0));
        assert!(after);

        // At the end of the row there is nowhere further to go.
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_secs(10),
            true,
        );
        assert_eq!(strip.target, strip.most());
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_secs(12),
            true,
        );
        assert!(!strip.sliding(), "still drawing at the end of the row");

        // Brought back in, the row stays where it got to.
        strip.drag(300.0);
        let was = strip.target;
        strip.lay(
            Some(room()),
            |_, _| 4.0,
            start + Duration::from_secs(13),
            true,
        );
        assert_eq!(strip.target, was);
    }

    /// A tab opened on a row longer than the bar slides into sight, as one
    /// chosen does.
    ///
    /// Deliberate break: empty the places in `told` when the tabs are
    /// another list, as it did, rather than marking them `stale`. The
    /// reveal takes the empty row for a first laying out, and the row
    /// jumps to the new tab.
    #[test]
    fn a_tab_opened_slides_into_sight() {
        let mut strip = strip(0);
        strip.told((0..11).map(|at| tab(at, at == 10)).collect());
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), true);
        assert!(strip.sliding(), "jumped to the tab opened");
    }

    /// A tab pressed and held without moving is a press: at the end of a
    /// row it does not scroll the row and is put nowhere, and only once it
    /// has gone half a letter is it carried.
    ///
    /// Deliberate break: make every hold `moved` from the start, in
    /// `Strip::hold`. A press on the tab cut by the end is a drag before
    /// the hand has moved, and scrolls the row from under the reader for
    /// as long as it is held.
    #[test]
    fn a_press_held_still_is_not_a_drag() {
        let mut strip = strip(0);
        let start = Instant::now();
        // Over the tab the end cuts, inside where a tab carried scrolls.
        strip.point(Some(600.0));
        assert_eq!(strip.hovered(), Some(Spot::Tab(6)));
        strip.hold(6);
        assert!(!strip.drag(603.0), "a press is drawn as a drag");
        for frame in 0..30 {
            strip.lay(
                Some(room()),
                |_, _| 4.0,
                start + Duration::from_millis(16 * frame),
                true,
            );
        }
        assert_eq!(strip.target, 0.0, "a press scrolled the row");
        assert_eq!(strip.carried(), None, "a press put the tab somewhere");
        assert_eq!(strip.holding(), None);

        assert!(strip.drag(606.0));
        assert_eq!(strip.holding(), Some(obelus_buffer::DocumentId::new(6)));
        assert!(strip.edging().is_some(), "carried, and not scrolling");
    }

    /// Where a tab carried goes is said once, until the tabs come back in
    /// another order; and a tab carried whose document goes is let go of.
    ///
    /// Deliberate breaks: leave the `asked` check out of `carried`, and the
    /// same place is said again on the next move; leave the letting go out
    /// of `told`, and the bar goes on scrolling under a hand holding a tab
    /// that is not there.
    #[test]
    fn a_place_is_said_once_and_a_tab_gone_is_let_go_of() {
        let id = obelus_buffer::DocumentId::new;
        let mut short = Strip::default();
        short.told((0..3).map(|at| tab(at, at == 0)).collect());
        short.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        short.point(Some(10.0));
        short.hold(0);
        short.drag(150.0);
        assert_eq!(short.carried(), Some((id(0), id(1), true)));
        short.drag(151.0);
        assert_eq!(short.carried(), None, "said again");
        // The application's answer, and a move back past the middle.
        short.told(vec![tab(1, false), tab(0, true), tab(2, false)]);
        short.lay(Some(room()), |_, _| 4.0, Instant::now(), false);
        short.drag(30.0);
        assert_eq!(short.carried(), Some((id(0), id(1), false)));

        let mut long = strip(0);
        long.point(Some(30.0));
        long.hold(0);
        long.drag(760.0);
        assert!(long.edging().is_some());
        long.told((1..10).map(|at| tab(at, at == 1)).collect());
        assert_eq!(long.holding(), None, "still holding a tab that has gone");
        assert!(long.edging().is_none());
    }
}
