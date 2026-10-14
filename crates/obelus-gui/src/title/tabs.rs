//! What is open, along the title bar: a tab for each document, and a press
//! on one goes to it.
//!
//! The application says what the tabs are -- `Drawing::tabs` -- and the
//! window says where they go, because only the window knows how wide a
//! name is in the face the bar is written in, and where on the bar the
//! system's own buttons stand: on the right on Windows, and on the left on
//! macOS, where the tabs start after them.
//!
//! More tabs than the bar has room for scroll, a whole tab at a time: a
//! tab cut in half at either end is a name the reader cannot read. An arrow
//! stands at each end while they do, and the wheel over the bar walks them
//! too. The tab being read is kept in sight whenever it changes -- the
//! reader chose it, and a choice they cannot see is one they cannot check.

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
    fn padding(&self) -> f32 {
        (self.words * 0.9).round()
    }

    /// How wide each arrow is.
    fn arrow(&self) -> f32 {
        (self.words * 1.8).round()
    }
}

/// One tab, placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Laid {
    /// Which tab, by where it is in the list.
    pub(crate) index: usize,
    /// Where its left edge is, in pixels from the window's left.
    pub(crate) left: f32,
    /// How wide it is.
    pub(crate) width: f32,
}

/// The tabs, and what the window has made of them.
#[derive(Debug, Default)]
pub(crate) struct Strip {
    tabs: Vec<Tab>,
    /// The first tab shown, which is how far the strip is scrolled.
    first: usize,
    /// The furthest it may be scrolled: where the last tab is the last one
    /// shown. From the last laying out.
    most: usize,
    /// Whether the tab being read has to be brought into sight at the next
    /// laying out, because it changed.
    reveal: bool,
    /// Where each tab shown went, at the last laying out.
    laid: Vec<Laid>,
    /// Where the two arrows are, as left and width, while there are more
    /// tabs than room.
    arrows: Option<([f32; 2], [f32; 2])>,
    room: Option<Room>,
    /// What the pointer is over.
    hovered: Option<Spot>,
}

/// The strip as one frame paints it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painted {
    pub(crate) room: Room,
    /// Each tab shown: what it says, where it is, and whether it is the
    /// one being read and the one under the pointer.
    pub(crate) tabs: Vec<(String, Laid, bool, bool)>,
    /// The two arrows, where they are shown: where, whether there is
    /// anything that way, and whether the pointer is over it.
    pub(crate) arrows: Vec<([f32; 2], bool, bool, Spot)>,
    /// Whether the window has the reader's keys.
    pub(crate) active: bool,
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
    /// `wide`. No room -- a window in full screen, or one whose bar is
    /// the compositor's -- is no strip.
    pub(crate) fn lay(&mut self, room: Option<Room>, mut wide: impl FnMut(&str, f32) -> f32) {
        self.room = room;
        let Some(room) = room else {
            self.laid.clear();
            self.arrows = None;
            return;
        };
        let widths: Vec<f32> = self
            .tabs
            .iter()
            .map(|tab| wide(&said(tab), room.words).ceil() + 2.0 * room.padding())
            .collect();
        let keep = match std::mem::take(&mut self.reveal) {
            true => self.tabs.iter().position(|tab| tab.reading),
            false => None,
        };
        let Arranged {
            first,
            most,
            laid,
            arrows,
        } = arrange(&widths, room.from, room.to, room.arrow(), self.first, keep);
        self.first = first;
        self.most = most;
        self.laid = laid;
        self.arrows = arrows;
    }

    /// What is at a place on the bar, in pixels from the window's left.
    pub(crate) fn at(&self, x: f32) -> Option<Spot> {
        let inside = |[left, width]: [f32; 2]| (left..left + width).contains(&x);
        if let Some((earlier, later)) = self.arrows {
            if inside(earlier) {
                return Some(Spot::Earlier);
            }
            if inside(later) {
                return Some(Spot::Later);
            }
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
        let room = self.room?;
        let end = match self.arrows {
            Some((_, [left, width])) => left + width,
            None => self.laid.last().map(|laid| laid.left + laid.width)?,
        };
        Some((room.from, end))
    }

    /// Walks the strip on by this many tabs, or back where it is negative.
    /// Whether that moved it.
    pub(crate) fn scroll(&mut self, by: isize) -> bool {
        if self.arrows.is_none() {
            return false;
        }
        let first = self.first.saturating_add_signed(by).min(self.most);
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// What the pointer is over, which is what a press there is on.
    pub(crate) const fn hovered(&self) -> Option<Spot> {
        self.hovered
    }

    /// Says what the pointer is over. Whether that changed what is drawn.
    pub(crate) fn hover(&mut self, spot: Option<Spot>) -> bool {
        let changed = self.hovered != spot;
        self.hovered = spot;
        changed
    }

    /// The strip as this frame paints it.
    pub(crate) fn painted(&self, active: bool) -> Option<Painted> {
        let room = self.room?;
        let tabs = self
            .laid
            .iter()
            .filter_map(|laid| {
                let tab = self.tabs.get(laid.index)?;
                Some((
                    said(tab),
                    *laid,
                    tab.reading,
                    self.hovered == Some(Spot::Tab(laid.index)),
                ))
            })
            .collect();
        let arrows = self
            .arrows
            .map(|(earlier, later)| {
                let last = self.laid.last().map_or(0, |laid| laid.index);
                vec![
                    (
                        earlier,
                        self.first > 0,
                        self.hovered == Some(Spot::Earlier),
                        Spot::Earlier,
                    ),
                    (
                        later,
                        last + 1 < self.tabs.len(),
                        self.hovered == Some(Spot::Later),
                        Spot::Later,
                    ),
                ]
            })
            .unwrap_or_default();
        Some(Painted {
            room,
            tabs,
            arrows,
            active,
        })
    }
}

/// What `arrange` made of the tabs.
#[derive(Debug, PartialEq)]
struct Arranged {
    /// The first tab shown.
    first: usize,
    /// The furthest the strip may be scrolled.
    most: usize,
    /// Each tab shown, placed.
    laid: Vec<Laid>,
    /// The arrows, as left and width, where there is not room for them all.
    arrows: Option<([f32; 2], [f32; 2])>,
}

/// Where tabs this wide go between `from` and `to`, scrolled to start at
/// `first` and brought round to show `keep`.
///
/// Never scrolled past the point where the last tab is in sight, so that
/// the room at the end is never empty while tabs are hidden at the start.
fn arrange(
    widths: &[f32],
    from: f32,
    to: f32,
    arrow: f32,
    first: usize,
    keep: Option<usize>,
) -> Arranged {
    let lay = |first: usize, start: f32, end: f32| {
        let mut laid = Vec::new();
        let mut x = start;
        for (index, &width) in widths.iter().enumerate().skip(first) {
            // The first one whatever its width, cut to the room: a name
            // longer than the whole bar is still the tab being read.
            if x + width > end && index > first {
                break;
            }
            laid.push(Laid {
                index,
                left: x,
                width: width.min(end - x),
            });
            x += width;
        }
        laid
    };
    if widths.iter().sum::<f32>() <= to - from {
        return Arranged {
            first: 0,
            most: 0,
            laid: lay(0, from, to),
            arrows: None,
        };
    }
    let (start, end) = (from + arrow, to - arrow);
    let room = end - start;
    let fits = |range: &[f32]| range.iter().sum::<f32>() <= room;
    // The furthest it may go: the first tab from which the rest all fit.
    let most = (0..widths.len())
        .find(|&at| fits(&widths[at..]))
        .unwrap_or(widths.len().saturating_sub(1));
    let mut first = first.min(most);
    if let Some(keep) = keep.filter(|&keep| keep < widths.len()) {
        if keep < first {
            first = keep;
        }
        while first < keep && !fits(&widths[first..=keep]) {
            first += 1;
        }
    }
    Arranged {
        first,
        most,
        laid: lay(first, start, end),
        arrows: Some(([from, arrow], [to - arrow, arrow])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabs that fit are laid one after another from where the strip
    /// starts, with no arrows.
    ///
    /// Deliberate break: lay them an arrow's width in whether or not they
    /// fit. The first tab then stands away from where the bar begins,
    /// beside an arrow that is not drawn.
    #[test]
    fn tabs_that_fit_are_laid_from_the_start() {
        let Arranged {
            first,
            laid,
            arrows,
            ..
        } = arrange(&[50.0, 30.0], 100.0, 300.0, 20.0, 0, None);
        assert_eq!(first, 0);
        assert_eq!(arrows, None);
        assert_eq!(
            laid,
            vec![
                Laid {
                    index: 0,
                    left: 100.0,
                    width: 50.0
                },
                Laid {
                    index: 1,
                    left: 150.0,
                    width: 30.0
                },
            ]
        );
    }

    /// Too many for the room: arrows at both ends, whole tabs between
    /// them, and the one being read brought into sight.
    ///
    /// Deliberate break: leave the `while` out of `arrange`. The tab being
    /// read is the tenth, the strip stays on the first, and it is off the
    /// end of the bar.
    #[test]
    fn the_tab_being_read_is_brought_into_sight() {
        let widths = [40.0; 10];
        // 200 across, less two arrows of 20: room for four.
        let Arranged {
            first,
            laid,
            arrows,
            ..
        } = arrange(&widths, 0.0, 200.0, 20.0, 0, Some(9));
        assert_eq!(first, 6);
        assert_eq!(
            laid.iter().map(|laid| laid.index).collect::<Vec<_>>(),
            [6, 7, 8, 9]
        );
        assert_eq!(laid[0].left, 20.0, "after the arrow");
        assert_eq!(arrows, Some(([0.0, 20.0], [180.0, 20.0])));
        let back = arrange(&widths, 0.0, 200.0, 20.0, 6, Some(2));
        assert_eq!(back.first, 2, "back, to one before");
    }

    /// Scrolled as far as it goes, the last tab is the last one shown and
    /// no further.
    ///
    /// Deliberate break: take `first` as it is asked. A strip scrolled
    /// past the end shows one tab and an empty bar, with six hidden.
    #[test]
    fn a_strip_is_never_scrolled_past_its_last_tab() {
        let end = arrange(&[40.0; 10], 0.0, 200.0, 20.0, 9, None);
        assert_eq!((end.first, end.most), (6, 6));
        assert_eq!(end.laid.len(), 4);
    }

    fn tab(document: usize, name: &str, reading: bool) -> Tab {
        Tab {
            document: obelus_buffer::DocumentId::new(document),
            name: name.to_string(),
            reading,
            unwritten: false,
        }
    }

    fn room() -> Room {
        Room {
            from: 0.0,
            to: 200.0,
            titled: 30.0,
            // An arrow of 18 and a padding of 9 on each side.
            words: 10.0,
        }
    }

    /// A press is placed on the tab or the arrow under it, and the wheel
    /// walks a whole tab at a time and stops at either end.
    ///
    /// Deliberate breaks: leave the arrows out of `at`, so a press on one
    /// is a press on the bar; and leave `.min(self.most)` out of `scroll`,
    /// so the wheel walks on past the last tab and has to walk back as far
    /// before the strip moves again.
    #[test]
    fn a_press_finds_what_is_under_it() {
        let mut strip = Strip::default();
        strip.told((0..10).map(|at| tab(at, "ab", at == 0)).collect());
        // Every name is 4 wide: tabs of 22, room for seven between arrows.
        strip.lay(Some(room()), |_, _| 4.0);
        assert_eq!(strip.at(5.0), Some(Spot::Earlier));
        assert_eq!(strip.at(18.0), Some(Spot::Tab(0)));
        assert_eq!(strip.at(190.0), Some(Spot::Later));
        assert!(strip.scroll(1));
        strip.lay(Some(room()), |_, _| 4.0);
        assert_eq!(strip.at(18.0), Some(Spot::Tab(1)), "a tab on");
        assert!(strip.scroll(10));
        assert!(!strip.scroll(1), "the last tab is in sight");
        assert!(strip.scroll(-10));
        assert!(!strip.scroll(-1), "the first");
    }

    /// The tab being read is brought into sight when it changes, and only
    /// then: a reader who scrolled the strip to look along it is not
    /// thrown back every frame.
    ///
    /// Deliberate break: set `reveal` on every `told`. The strip jumps
    /// back to the tab being read on the next frame after every notch.
    #[test]
    fn only_a_change_brings_the_tab_back() {
        let mut strip = Strip::default();
        let tabs = |reading: usize| (0..10).map(|at| tab(at, "ab", at == reading)).collect();
        strip.told(tabs(9));
        strip.lay(Some(room()), |_, _| 4.0);
        assert_eq!(strip.first, 3);
        strip.scroll(-3);
        strip.told(tabs(9));
        strip.lay(Some(room()), |_, _| 4.0);
        assert_eq!(strip.first, 0, "the same tab, told again");
        strip.told(tabs(8));
        strip.lay(Some(room()), |_, _| 4.0);
        assert_eq!(strip.first, 2, "another");
    }
}
