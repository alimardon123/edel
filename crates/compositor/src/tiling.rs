//! Dynamic tiling (roadmap M4.5), the second [`WindowPolicy`], in one of
//! the styles of `layout.tiling_style` (M5.16), which say how tiling lays
//! the windows out. `stack`, the default, is master and stack: the first
//! window of the workspace takes the left half, the rest share the right
//! half from the top down, with a gap between them and round them; one
//! window fills the area. A new window joins the bottom of the stack, so
//! the one being worked in stays put, and when the master closes the first
//! of the stack takes its place. `split` is `split.rs`, `scroll` is
//! `scroll.rs`. `full` (M9.4b, the Phone preset's) gives every window the
//! whole area, so the one in front is all there is, as on a phone; the
//! window list and the switcher bring another forward. A window moved or
//! resized by a person goes back to its tile. Title bars stay in tiling.
//! A style is one module and one line in [`Style`].

use smithay::utils::{Logical, Rectangle, Size};

use crate::layout::{Gaps, WindowPolicy};
use crate::scroll::Strip;
use crate::split::Tree;

/// How tiling lays windows out: `layout.tiling_style`'s values, the
/// default first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Stack,
    Split,
    Scroll,
    Full,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Stack, Style::Split, Style::Scroll, Style::Full];

    /// The name the settings file and the log use.
    pub fn name(self) -> &'static str {
        match self {
            Style::Stack => "stack",
            Style::Split => "split",
            Style::Scroll => "scroll",
            Style::Full => "full",
        }
    }

    pub fn parse(name: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.name() == name)
    }
}

#[derive(Debug)]
pub struct Tiling<W> {
    style: Style,
    /// The master first, then the stack from the top: the order windows
    /// opened in, which `stack` lays out.
    windows: Vec<W>,
    /// The halves `split` lays out.
    tree: Tree<W>,
    /// The columns `scroll` lays out.
    strip: Strip<W>,
    /// The window with the keyboard, which a new window halves in `split`.
    focused: Option<W>,
    gaps: Gaps,
    area: Rectangle<i32, Logical>,
}

impl<W> Tiling<W> {
    /// Tiling with `gaps` logical pixels between tiles and round them.
    pub fn new(gaps: Gaps) -> Tiling<W> {
        Tiling {
            style: Style::default(),
            windows: Vec::new(),
            tree: Tree::default(),
            strip: Strip::default(),
            focused: None,
            gaps: Gaps {
                inner: gaps.inner.min(100),
                outer: gaps.outer.min(100),
            },
            area: Rectangle::default(),
        }
    }
}

/// The tiles of `count` windows in `area`, master first.
pub fn tiles(
    count: usize,
    area: Rectangle<i32, Logical>,
    gaps: Gaps,
) -> Vec<Rectangle<i32, Logical>> {
    if count == 0 {
        return Vec::new();
    }
    let Gaps { inner: gap, outer } = gaps.within(area);
    let inner = Rectangle::new(
        (area.loc.x + outer, area.loc.y + outer).into(),
        (area.size.w - 2 * outer, area.size.h - 2 * outer).into(),
    );
    if count == 1 {
        return vec![inner];
    }
    let master_w = (inner.size.w - gap) / 2;
    let stack_x = inner.loc.x + master_w + gap;
    let stack_w = inner.size.w - master_w - gap;
    let mut tiles = vec![Rectangle::new(inner.loc, (master_w, inner.size.h).into())];
    let rows = (count - 1) as i32;
    let room = inner.size.h - gap * (rows - 1);
    let mut y = inner.loc.y;
    for row in 0..rows {
        // The last row takes what division leaves over, so the stack ends
        // exactly at the bottom gap.
        let h = if row == rows - 1 {
            inner.loc.y + inner.size.h - y
        } else {
            room / rows
        };
        tiles.push(Rectangle::new(
            (stack_x, y).into(),
            (stack_w, h.max(1)).into(),
        ));
        y += h + gap;
    }
    tiles
}

impl<W: Clone + PartialEq> Tiling<W> {
    /// Every window's place in `area` in the style.
    fn places(&self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)> {
        match self.style {
            Style::Stack => self
                .windows
                .iter()
                .cloned()
                .zip(tiles(self.windows.len(), area, self.gaps))
                .collect(),
            Style::Split => self.tree.places(area, self.gaps),
            Style::Scroll => self.strip.places(area, self.gaps),
            Style::Full => {
                let whole = tiles(1, area, self.gaps)[0];
                self.windows.iter().map(|w| (w.clone(), whole)).collect()
            }
        }
    }

    fn tile_of(&self, window: &W) -> Option<Rectangle<i32, Logical>> {
        self.places(self.area)
            .into_iter()
            .find(|(w, _)| w == window)
            .map(|(_, place)| place)
    }
}

impl<W: Clone + PartialEq> WindowPolicy<W> for Tiling<W> {
    fn name(&self) -> &'static str {
        "tiling"
    }

    fn open(
        &mut self,
        window: W,
        _wanted: Size<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        self.area = area;
        self.windows.retain(|w| *w != window);
        self.windows.push(window.clone());
        self.tree.insert(window.clone(), self.focused.as_ref());
        self.strip.insert(window.clone(), self.focused.as_ref());
        self.tile_of(&window).unwrap_or(area)
    }

    fn close(&mut self, window: &W) {
        self.windows.retain(|w| w != window);
        self.tree.remove(window);
        self.strip.remove(window);
        if self.focused.as_ref() == Some(window) {
            self.focused = None;
        }
    }

    fn focused(&mut self, window: &W) -> bool {
        if !self.windows.contains(window) {
            return false;
        }
        self.focused = Some(window.clone());
        self.style == Style::Scroll
            && self
                .strip
                .reveal(self.focused.as_ref(), self.area.size.w, self.gaps)
    }

    fn widen(&mut self, window: &W) -> bool {
        self.style == Style::Scroll && self.strip.widen(window)
    }

    fn swap(&mut self, a: &W, b: &W) -> bool {
        let (Some(i), Some(j)) = (
            self.windows.iter().position(|w| w == a),
            self.windows.iter().position(|w| w == b),
        ) else {
            return false;
        };
        self.windows.swap(i, j);
        self.tree.swap(a, b);
        self.strip.swap(a, b);
        true
    }

    fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    fn set_gaps(&mut self, gaps: Gaps) {
        self.gaps = gaps;
    }

    fn moved(&mut self, window: &W, to: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        self.tile_of(window).unwrap_or(to)
    }

    fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)> {
        self.area = area;
        if self.style == Style::Scroll {
            self.strip
                .reveal(self.focused.as_ref(), area.size.w, self.gaps);
        }
        self.places(area)
    }

    fn rearranges(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rectangle<i32, Logical> {
        Rectangle::from_size((1280, 800).into())
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn one_window_fills_the_area_inside_the_gap() {
        assert_eq!(
            tiles(1, screen(), Gaps::uniform(8)),
            [rect(8, 8, 1264, 784)]
        );
        assert_eq!(tiles(0, screen(), Gaps::uniform(8)), []);
    }

    #[test]
    fn two_windows_share_the_area_side_by_side() {
        assert_eq!(
            tiles(2, screen(), Gaps::uniform(8)),
            [rect(8, 8, 628, 784), rect(644, 8, 628, 784)]
        );
    }

    /// For 1 to 6 windows: every tile inside the area, none overlapping,
    /// the gap between neighbours, and the stack reaching the bottom.
    #[test]
    fn up_to_six_windows_tile_without_overlap() {
        for area in [screen(), rect(100, 50, 1366, 740), rect(0, 0, 1920, 1080)] {
            for n in 1..=6 {
                let t = tiles(n, area, Gaps::uniform(8));
                assert_eq!(t.len(), n);
                for (i, a) in t.iter().enumerate() {
                    assert!(a.size.w > 0 && a.size.h > 0, "{n}: {a:?}");
                    assert!(area.contains_rect(*a), "{n}: {a:?} outside {area:?}");
                    for b in &t[i + 1..] {
                        assert!(!a.overlaps(*b), "{n}: {a:?} overlaps {b:?}");
                    }
                }
                let bottom = area.loc.y + area.size.h - 8;
                let last = t[n - 1];
                assert_eq!(last.loc.y + last.size.h, bottom, "{n} windows");
                for pair in t[1..].windows(2) {
                    assert_eq!(pair[0].loc.y + pair[0].size.h + 8, pair[1].loc.y, "{n}");
                    assert_eq!(pair[0].loc.x, pair[1].loc.x);
                    assert_eq!(pair[0].size.w, pair[1].size.w);
                }
                if n > 1 {
                    assert_eq!(t[0].loc.x + t[0].size.w + 8, t[1].loc.x, "{n}");
                    assert_eq!(t[1].loc.x + t[1].size.w, area.loc.x + area.size.w - 8);
                }
            }
        }
    }

    #[test]
    fn a_tiny_area_drops_the_gap() {
        assert_eq!(
            tiles(1, rect(0, 0, 20, 20), Gaps::uniform(8)),
            [rect(0, 0, 20, 20)]
        );
    }

    #[test]
    fn new_windows_join_the_stack_and_the_stack_fills_a_closed_master() {
        let mut tiling = Tiling::new(Gaps::uniform(8));
        assert_eq!(
            tiling.open(1, Size::default(), screen()),
            rect(8, 8, 1264, 784)
        );
        assert_eq!(
            tiling.open(2, Size::default(), screen()),
            rect(644, 8, 628, 784)
        );
        tiling.open(3, Size::default(), screen());
        let arranged = tiling.arrange(screen());
        assert_eq!(arranged[0], (1, rect(8, 8, 628, 784)), "1 stays master");
        assert_eq!(arranged[2].0, 3, "3 at the bottom of the stack");
        tiling.close(&1);
        assert_eq!(tiling.arrange(screen())[0], (2, rect(8, 8, 628, 784)));
        // A window a person drags goes back to its tile.
        assert_eq!(tiling.moved(&3, rect(0, 0, 50, 50)), rect(644, 8, 628, 784));
        assert!(tiling.rearranges());
        assert_eq!(tiling.name(), "tiling");
    }

    #[test]
    fn the_style_changes_the_places_and_keeps_the_windows() {
        let mut tiling = Tiling::new(Gaps::uniform(8));
        for w in 1..=4 {
            tiling.open(w, Size::default(), screen());
            tiling.focused(&w);
        }
        let stacked = tiling.arrange(screen());
        assert_eq!(stacked[3], (4, rect(644, 536, 628, 256)));
        tiling.set_style(Style::Split);
        let split = tiling.arrange(screen());
        assert_eq!(split[3], (4, rect(962, 404, 310, 388)));
        // Swapping trades places in either style.
        assert!(tiling.swap(&1, &4));
        assert_eq!(tiling.arrange(screen())[3], (1, rect(962, 404, 310, 388)));
        tiling.set_style(Style::Stack);
        assert_eq!(tiling.arrange(screen())[0], (4, rect(8, 8, 628, 784)));
        assert!(!tiling.swap(&1, &9));
        assert_eq!(Style::parse("split"), Some(Style::Split));
        assert_eq!(Style::parse("scroll"), Some(Style::Scroll));
        assert_eq!(Style::parse("full"), Some(Style::Full));
        assert_eq!(Style::parse("spiral"), None);
    }

    /// `full` gives every window the whole area inside the gap, so the one
    /// in front is all that shows, as on a phone.
    #[test]
    fn full_gives_every_window_the_whole_area() {
        let mut tiling = Tiling::new(Gaps::uniform(8));
        tiling.set_style(Style::Full);
        assert_eq!(
            tiling.open(1, Size::default(), screen()),
            rect(8, 8, 1264, 784)
        );
        assert_eq!(
            tiling.open(2, Size::default(), screen()),
            rect(8, 8, 1264, 784)
        );
        assert_eq!(
            tiling.arrange(screen()),
            [(1, rect(8, 8, 1264, 784)), (2, rect(8, 8, 1264, 784))]
        );
        tiling.close(&1);
        assert_eq!(tiling.arrange(screen()), [(2, rect(8, 8, 1264, 784))]);
    }

    /// The inner and outer gaps are their own: 4 from the edges, 12 between.
    #[test]
    fn the_inner_and_outer_gaps_are_their_own() {
        let gaps = Gaps {
            inner: 12,
            outer: 4,
        };
        assert_eq!(
            tiles(2, screen(), gaps),
            [rect(4, 4, 630, 792), rect(646, 4, 630, 792)]
        );
        let mut tiling = Tiling::new(gaps);
        tiling.open(1, Size::default(), screen());
        tiling.open(2, Size::default(), screen());
        assert_eq!(tiling.arrange(screen())[1], (2, rect(646, 4, 630, 792)));
        // Live: new gaps lay the tiles out again.
        tiling.set_gaps(Gaps::uniform(8));
        assert_eq!(tiling.arrange(screen())[1], (2, rect(644, 8, 628, 784)));
    }
}
