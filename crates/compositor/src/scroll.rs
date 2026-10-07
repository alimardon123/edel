//! The scroll tiling style (roadmap M5.16c), as niri tiles: each window is
//! a column on a strip that may be wider than the screen, as tall as the
//! screen's area. A new window opens as a column right of the focused one,
//! half the screen wide, and the strip scrolls so the focused column is
//! always whole on screen, so windows keep their size however many are
//! open. A column's width steps through a third, a half and two thirds of
//! the screen (Super+R); a closed column's neighbours close the gap.

use smithay::utils::{Logical, Rectangle};

/// The widths a column steps through, as parts of the screen's width; a
/// new column takes the second.
pub const WIDTHS: [f32; 3] = [1.0 / 3.0, 0.5, 2.0 / 3.0];
const NEW: usize = 1;

/// One screen's strip of columns and how far it is scrolled.
#[derive(Debug, Clone)]
pub struct Strip<W> {
    /// Each window and its width, an index into [`WIDTHS`], left first.
    columns: Vec<(W, usize)>,
    /// How far the strip is scrolled, in logical pixels from its left end.
    view: i32,
}

impl<W> Default for Strip<W> {
    fn default() -> Self {
        Strip {
            columns: Vec::new(),
            view: 0,
        }
    }
}

impl<W: Clone + PartialEq> Strip<W> {
    /// `window` opens as a column right of `beside`, or at the right end
    /// when `beside` is none or not on the strip.
    pub fn insert(&mut self, window: W, beside: Option<&W>) {
        self.remove(&window);
        let at = beside
            .and_then(|b| self.columns.iter().position(|(w, _)| w == b))
            .map_or(self.columns.len(), |i| i + 1);
        self.columns.insert(at, (window, NEW));
    }

    /// `window`'s column leaves; the columns right of it close the gap.
    pub fn remove(&mut self, window: &W) {
        self.columns.retain(|(w, _)| w != window);
    }

    /// `a`'s and `b`'s columns trade places, each keeping its width.
    pub fn swap(&mut self, a: &W, b: &W) {
        let i = self.columns.iter().position(|(w, _)| w == a);
        let j = self.columns.iter().position(|(w, _)| w == b);
        if let (Some(i), Some(j)) = (i, j) {
            self.columns.swap(i, j);
        }
    }

    /// `window`'s column takes the next width of [`WIDTHS`], back to the
    /// first after the last; false if it is not on the strip.
    pub fn widen(&mut self, window: &W) -> bool {
        match self.columns.iter_mut().find(|(w, _)| w == window) {
            Some((_, width)) => {
                *width = (*width + 1) % WIDTHS.len();
                true
            }
            None => false,
        }
    }

    /// Each column's left end and width on the strip, from the strip's
    /// left end, for an area `area_w` wide with `gap` round and between.
    fn spans(&self, area_w: i32, gap: i32) -> Vec<(i32, i32)> {
        let room = (area_w - 2 * gap).max(1);
        let mut left = gap;
        self.columns
            .iter()
            .map(|(_, width)| {
                let w = ((room as f32 * WIDTHS[*width]) as i32 - gap / 2).max(1);
                let span = (left, w);
                left += w + gap;
                span
            })
            .collect()
    }

    /// Scrolls the strip as little as it takes for `focused`'s column to
    /// be whole in an area `area_w` wide, and no further than its ends;
    /// whether it moved.
    pub fn reveal(&mut self, focused: Option<&W>, area_w: i32, gap: i32) -> bool {
        let spans = self.spans(area_w, gap);
        let total = spans.last().map_or(0, |(l, w)| l + w + gap);
        let before = self.view;
        if let Some(i) = focused.and_then(|f| self.columns.iter().position(|(w, _)| w == f)) {
            let (left, width) = spans[i];
            if left - gap < self.view {
                self.view = left - gap;
            } else if left + width + gap > self.view + area_w {
                self.view = left + width + gap - area_w;
            }
        }
        self.view = self.view.clamp(0, (total - area_w).max(0));
        self.view != before
    }

    /// Every window's place in `area`, left first: whole columns as tall
    /// as the area, scrolled by the view, so some may lie off screen.
    pub fn places(
        &self,
        area: Rectangle<i32, Logical>,
        gap: i32,
    ) -> Vec<(W, Rectangle<i32, Logical>)> {
        let gap = if area.size.w > 4 * gap && area.size.h > 4 * gap {
            gap
        } else {
            0
        };
        let h = (area.size.h - 2 * gap).max(1);
        self.columns
            .iter()
            .zip(self.spans(area.size.w, gap))
            .map(|((w, _), (left, width))| {
                let x = area.loc.x + left - self.view;
                (
                    w.clone(),
                    Rectangle::new((x, area.loc.y + gap).into(), (width, h).into()),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rectangle<i32, Logical> {
        Rectangle::from_size((1280, 800).into())
    }

    /// Windows 1 to `n`, each opened beside the one before it and
    /// focused, as the compositor does.
    fn opened(n: u32) -> Strip<u32> {
        let mut strip = Strip::default();
        for w in 1..=n {
            strip.insert(w, (w > 1).then_some(&(w - 1)));
            strip.reveal(Some(&w), screen().size.w, 8);
        }
        strip
    }

    fn place(strip: &Strip<u32>, w: u32) -> Rectangle<i32, Logical> {
        strip
            .places(screen(), 8)
            .into_iter()
            .find(|(x, _)| *x == w)
            .unwrap()
            .1
    }

    fn whole(r: Rectangle<i32, Logical>) -> bool {
        r.loc.x >= 0 && r.loc.x + r.size.w <= 1280
    }

    #[test]
    fn columns_keep_their_width_and_the_newest_is_whole() {
        for n in 1..=6 {
            let strip = opened(n);
            let places = strip.places(screen(), 8);
            assert_eq!(places.len(), n as usize);
            // Every column half the screen less the gaps, as tall as it.
            for (_, r) in &places {
                assert_eq!(r.size.w, 628, "{n}");
                assert_eq!((r.loc.y, r.size.h), (8, 784), "{n}");
            }
            // Side by side, a gap apart, none overlapping.
            for pair in places.windows(2) {
                assert_eq!(pair[0].1.loc.x + pair[0].1.size.w + 8, pair[1].1.loc.x);
            }
            assert!(whole(place(&strip, n)), "the newest of {n} is whole");
        }
        // With four, the first lies off screen to the left.
        let four = opened(4);
        assert!(place(&four, 1).loc.x + place(&four, 1).size.w <= 0);
    }

    #[test]
    fn focusing_a_column_scrolls_it_whole_and_no_further() {
        let mut strip = opened(4);
        assert!(strip.reveal(Some(&1), 1280, 8));
        assert!(whole(place(&strip, 1)));
        assert_eq!(
            place(&strip, 1).loc.x,
            8,
            "the strip's left end, not past it"
        );
        assert!(!strip.reveal(Some(&2), 1280, 8), "2 is already whole");
        assert!(strip.reveal(Some(&4), 1280, 8));
        assert_eq!(
            place(&strip, 4).loc.x + 628 + 8,
            1280,
            "the right end, not past it"
        );
    }

    #[test]
    fn a_new_column_opens_right_of_the_focused_one() {
        let mut strip = opened(3);
        strip.insert(9, Some(&1));
        let order: Vec<u32> = strip.places(screen(), 8).iter().map(|(w, _)| *w).collect();
        assert_eq!(order, [1, 9, 2, 3]);
        strip.swap(&9, &3);
        let order: Vec<u32> = strip.places(screen(), 8).iter().map(|(w, _)| *w).collect();
        assert_eq!(order, [1, 3, 2, 9]);
    }

    #[test]
    fn a_closed_columns_neighbours_close_the_gap() {
        for gone in 1..=6 {
            let mut strip = opened(6);
            strip.remove(&gone);
            strip.reveal(None, 1280, 8);
            let places = strip.places(screen(), 8);
            assert_eq!(places.len(), 5);
            for pair in places.windows(2) {
                assert_eq!(
                    pair[0].1.loc.x + pair[0].1.size.w + 8,
                    pair[1].1.loc.x,
                    "{gone}"
                );
            }
        }
    }

    #[test]
    fn super_r_steps_a_columns_width_through_a_third_a_half_and_two_thirds() {
        let mut strip = opened(1);
        let width = |s: &Strip<u32>| place(s, 1).size.w;
        assert_eq!(width(&strip), 628);
        assert!(strip.widen(&1));
        assert_eq!(width(&strip), 838);
        assert!(strip.widen(&1));
        assert_eq!(width(&strip), 417);
        assert!(!strip.widen(&7));
    }
}
