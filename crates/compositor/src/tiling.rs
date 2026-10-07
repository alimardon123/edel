//! Dynamic tiling (roadmap M4.5), the second [`WindowPolicy`], in one of
//! the styles of `layout.tiling_style` (M5.16), which say how tiling lays
//! the windows out. `stack`, the default, is master and stack: the first
//! window of the workspace takes the left half, the rest share the right
//! half from the top down, with a gap between them and round them; one
//! window fills the area. A new window joins the bottom of the stack, so
//! the one being worked in stays put, and when the master closes the first
//! of the stack takes its place. `split` is `split.rs`. A window moved or
//! resized by a person goes back to its tile. Title bars stay in tiling.
//! A style is one module and one line in [`Style`].

use smithay::utils::{Logical, Rectangle, Size};

use crate::layout::WindowPolicy;
use crate::split::Tree;

/// How tiling lays windows out: `layout.tiling_style`'s values, the
/// default first. `scroll` joins with M5.16c.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Stack,
    Split,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Stack, Style::Split];

    /// The name the settings file and the log use.
    pub fn name(self) -> &'static str {
        match self {
            Style::Stack => "stack",
            Style::Split => "split",
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
    /// The window with the keyboard, which a new window halves in `split`.
    focused: Option<W>,
    gap: i32,
    area: Rectangle<i32, Logical>,
}

impl<W> Tiling<W> {
    /// Tiling with `gap` logical pixels between tiles and round them.
    pub fn new(gap: u32) -> Tiling<W> {
        Tiling {
            style: Style::default(),
            windows: Vec::new(),
            tree: Tree::default(),
            focused: None,
            gap: gap.min(100) as i32,
            area: Rectangle::default(),
        }
    }
}

/// The tiles of `count` windows in `area`, master first.
pub fn tiles(
    count: usize,
    area: Rectangle<i32, Logical>,
    gap: i32,
) -> Vec<Rectangle<i32, Logical>> {
    if count == 0 {
        return Vec::new();
    }
    // The gap is dropped where it would leave no room.
    let gap = if area.size.w > 4 * gap && area.size.h > 4 * gap {
        gap
    } else {
        0
    };
    let inner = Rectangle::new(
        (area.loc.x + gap, area.loc.y + gap).into(),
        (area.size.w - 2 * gap, area.size.h - 2 * gap).into(),
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
                .zip(tiles(self.windows.len(), area, self.gap))
                .collect(),
            Style::Split => self.tree.places(area, self.gap),
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
        self.tile_of(&window).unwrap_or(area)
    }

    fn close(&mut self, window: &W) {
        self.windows.retain(|w| w != window);
        self.tree.remove(window);
        if self.focused.as_ref() == Some(window) {
            self.focused = None;
        }
    }

    fn focused(&mut self, window: &W) {
        if self.windows.contains(window) {
            self.focused = Some(window.clone());
        }
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
        true
    }

    fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    fn moved(&mut self, window: &W, to: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        self.tile_of(window).unwrap_or(to)
    }

    fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)> {
        self.area = area;
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
        assert_eq!(tiles(1, screen(), 8), [rect(8, 8, 1264, 784)]);
        assert_eq!(tiles(0, screen(), 8), []);
    }

    #[test]
    fn two_windows_share_the_area_side_by_side() {
        assert_eq!(
            tiles(2, screen(), 8),
            [rect(8, 8, 628, 784), rect(644, 8, 628, 784)]
        );
    }

    /// For 1 to 6 windows: every tile inside the area, none overlapping,
    /// the gap between neighbours, and the stack reaching the bottom.
    #[test]
    fn up_to_six_windows_tile_without_overlap() {
        for area in [screen(), rect(100, 50, 1366, 740), rect(0, 0, 1920, 1080)] {
            for n in 1..=6 {
                let t = tiles(n, area, 8);
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
        assert_eq!(tiles(1, rect(0, 0, 20, 20), 8), [rect(0, 0, 20, 20)]);
    }

    #[test]
    fn new_windows_join_the_stack_and_the_stack_fills_a_closed_master() {
        let mut tiling = Tiling::new(8);
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
        let mut tiling = Tiling::new(8);
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
        assert_eq!(Style::parse("scroll"), None);
    }
}
