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
//! The slide is the window's own clock and none of the application's, and
//! it runs only while the row is somewhere other than where it is going --
//! asked every frame from that (`Strip::sliding`), the way everything that
//! moves in the window is.

use std::time::Instant;

use obelus_app::app::Tab;

/// What a place on the strip is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    /// A tab, by where it is in the list.
    Tab(usize),
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
}

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
}

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
        }
        self.room = room;
        let Some(room) = room else {
            self.laid.clear();
            self.arrows = None;
            // Arrived, with nothing to show the arriving on: a slide left
            // under way would keep `sliding` true, and the window drawing
            // frames for it, for as long as there is no bar -- the whole
            // of a stay in full screen.
            self.offset = self.target;
            self.stepped = None;
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
                (wide(&said(tab), room.words).ceil() + marked + 2.0 * room.padding())
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
        self.hovered = self.pointer.and_then(|x| self.at(x));
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
        if !self.sliding() {
            self.offset = self.target;
        }
        self.offset = self.offset.clamp(0.0, self.most());
        self.stepped = self.sliding().then_some(now);
    }

    /// Whether the row is still on its way somewhere: what keeps the window
    /// drawing frames for it.
    pub(crate) fn sliding(&self) -> bool {
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
        if self.covered {
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
        self.laid
            .iter()
            .find(|laid| inside([laid.left, laid.width]))
            .map(|laid| Spot::Tab(laid.index))
    }

    /// Which document a tab is.
    pub(crate) fn document(&self, index: usize) -> Option<obelus_buffer::DocumentId> {
        self.tabs.get(index).map(|tab| tab.document)
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
            Spot::Tab(_) => None,
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
        let tabs = self
            .laid
            .iter()
            .filter_map(|laid| {
                let tab = self.tabs.get(laid.index)?;
                Some(Shown {
                    said: said(tab),
                    icon: tab.icon,
                    laid: *laid,
                    reading: tab.reading,
                    hovered: self.hovered == Some(Spot::Tab(laid.index)),
                })
            })
            .collect();
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
    /// the middle of a slide draws frames for it until it comes back.
    #[test]
    fn a_tab_chosen_slides_into_sight_and_no_bar_ends_the_slide() {
        let mut strip = strip(0);
        strip.told((0..10).map(|at| tab(at, at == 6)).collect());
        strip.lay(Some(room()), |_, _| 4.0, Instant::now(), true);
        assert!(strip.sliding(), "jumped to the tab chosen");
        strip.lay(None, |_, _| 4.0, Instant::now(), true);
        assert!(!strip.sliding(), "still sliding with no bar");
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
}
