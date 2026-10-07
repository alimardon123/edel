//! The split tiling style (roadmap M5.16a), as Hyprland tiles by default:
//! a new window takes half of the focused window's space, cut across that
//! space's longer side, and a closed window gives its space back to the
//! window or windows that shared it. The windows are the leaves of a
//! binary tree; where each half lies is worked out from the area every
//! time, so a screen that changes its size or turns keeps the same tree.

use smithay::utils::{Logical, Rectangle};

#[derive(Debug, Clone)]
enum Node<W> {
    Window(W),
    /// Two halves of one space: the older first, left or on top.
    Halves(Box<Node<W>>, Box<Node<W>>),
}

/// The tree of one screen's tiled windows.
#[derive(Debug, Clone)]
pub struct Tree<W> {
    root: Option<Node<W>>,
}

impl<W> Default for Tree<W> {
    fn default() -> Self {
        Tree { root: None }
    }
}

impl<W: Clone + PartialEq> Tree<W> {
    /// `window` opens in half of `beside`'s space, or of the newest
    /// window's when `beside` is none or not in the tree.
    pub fn insert(&mut self, window: W, beside: Option<&W>) {
        self.remove(&window);
        let Some(root) = self.root.take() else {
            self.root = Some(Node::Window(window));
            return;
        };
        let target = match beside {
            Some(b) if root.holds(b) => b.clone(),
            _ => root.newest().clone(),
        };
        self.root = Some(root.split(&target, &window));
    }

    /// `window` leaves; the other half of its space takes all of it.
    pub fn remove(&mut self, window: &W) {
        self.root = self.root.take().and_then(|root| root.without(window));
    }

    /// `a` and `b` trade places.
    pub fn swap(&mut self, a: &W, b: &W) {
        if let Some(root) = &mut self.root {
            if root.holds(a) && root.holds(b) {
                root.trade(a, b);
            }
        }
    }

    /// Every window's place in `area`, with `gap` between windows and
    /// round them, oldest first.
    pub fn places(
        &self,
        area: Rectangle<i32, Logical>,
        gap: i32,
    ) -> Vec<(W, Rectangle<i32, Logical>)> {
        let mut out = Vec::new();
        let Some(root) = &self.root else {
            return out;
        };
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
        root.place(inner, gap, &mut out);
        out
    }
}

impl<W: Clone + PartialEq> Node<W> {
    fn holds(&self, window: &W) -> bool {
        match self {
            Node::Window(w) => w == window,
            Node::Halves(a, b) => a.holds(window) || b.holds(window),
        }
    }

    /// The newest window: the second half all the way down.
    fn newest(&self) -> &W {
        match self {
            Node::Window(w) => w,
            Node::Halves(_, b) => b.newest(),
        }
    }

    fn split(self, target: &W, window: &W) -> Node<W> {
        match self {
            Node::Window(w) if w == *target => Node::Halves(
                Box::new(Node::Window(w)),
                Box::new(Node::Window(window.clone())),
            ),
            Node::Window(w) => Node::Window(w),
            Node::Halves(a, b) => Node::Halves(
                Box::new(a.split(target, window)),
                Box::new(b.split(target, window)),
            ),
        }
    }

    fn without(self, window: &W) -> Option<Node<W>> {
        match self {
            Node::Window(w) => (w != *window).then_some(Node::Window(w)),
            Node::Halves(a, b) => match (a.without(window), b.without(window)) {
                (Some(a), Some(b)) => Some(Node::Halves(Box::new(a), Box::new(b))),
                (Some(left), None) | (None, Some(left)) => Some(left),
                (None, None) => None,
            },
        }
    }

    fn trade(&mut self, a: &W, b: &W) {
        match self {
            Node::Window(w) if w == a => *w = b.clone(),
            Node::Window(w) if w == b => *w = a.clone(),
            Node::Window(_) => {}
            Node::Halves(x, y) => {
                x.trade(a, b);
                y.trade(a, b);
            }
        }
    }

    /// Lays the node out in `space`: halves are cut across the longer side.
    fn place(
        &self,
        space: Rectangle<i32, Logical>,
        gap: i32,
        out: &mut Vec<(W, Rectangle<i32, Logical>)>,
    ) {
        match self {
            Node::Window(w) => out.push((w.clone(), space)),
            Node::Halves(a, b) => {
                let (first, second) = if space.size.w >= space.size.h {
                    let w = (space.size.w - gap) / 2;
                    (
                        Rectangle::new(space.loc, (w, space.size.h).into()),
                        Rectangle::new(
                            (space.loc.x + w + gap, space.loc.y).into(),
                            (space.size.w - w - gap, space.size.h).into(),
                        ),
                    )
                } else {
                    let h = (space.size.h - gap) / 2;
                    (
                        Rectangle::new(space.loc, (space.size.w, h).into()),
                        Rectangle::new(
                            (space.loc.x, space.loc.y + h + gap).into(),
                            (space.size.w, space.size.h - h - gap).into(),
                        ),
                    )
                };
                a.place(first, gap, out);
                b.place(second, gap, out);
            }
        }
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

    /// Windows 1 to `n`, each opened beside the one before it, as each
    /// new window has the focus.
    fn opened(n: u32) -> Tree<u32> {
        let mut tree = Tree::default();
        for w in 1..=n {
            tree.insert(w, (w > 1).then_some(&(w - 1)));
        }
        tree
    }

    fn place_of(tree: &Tree<u32>, w: u32) -> Rectangle<i32, Logical> {
        tree.places(screen(), 8)
            .into_iter()
            .find(|(x, _)| *x == w)
            .unwrap()
            .1
    }

    #[test]
    fn each_new_window_halves_the_last_ones_space_across_its_longer_side() {
        assert_eq!(opened(1).places(screen(), 8), [(1, rect(8, 8, 1264, 784))]);
        assert_eq!(
            opened(2).places(screen(), 8),
            [(1, rect(8, 8, 628, 784)), (2, rect(644, 8, 628, 784))]
        );
        // The third halves the second's tall space: top and bottom.
        let three = opened(3);
        assert_eq!(place_of(&three, 2), rect(644, 8, 628, 388));
        assert_eq!(place_of(&three, 3), rect(644, 404, 628, 388));
        // The fourth shares the bottom right quarter with the third, side
        // by side, as desktop-test checks.
        let four = opened(4);
        assert_eq!(place_of(&four, 3), rect(644, 404, 310, 388));
        assert_eq!(place_of(&four, 4), rect(962, 404, 310, 388));
        assert_eq!(place_of(&four, 1), rect(8, 8, 628, 784), "1 keeps its half");
    }

    /// For 1 to 6 windows on three screens: every place inside the area,
    /// none overlapping, and together they cover the area but the gaps.
    #[test]
    fn up_to_six_windows_split_without_overlap() {
        for area in [screen(), rect(100, 50, 1366, 740), rect(0, 0, 1080, 1920)] {
            for n in 1..=6 {
                let places = opened(n).places(area, 8);
                assert_eq!(places.len(), n as usize);
                for (i, (_, a)) in places.iter().enumerate() {
                    assert!(a.size.w > 0 && a.size.h > 0, "{n}: {a:?}");
                    assert!(area.contains_rect(*a), "{n}: {a:?} outside {area:?}");
                    for (_, b) in &places[i + 1..] {
                        assert!(!a.overlaps(*b), "{n}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    /// Each window of six closed in turn: the window or windows that
    /// shared its space take it, covering where it was, and the older
    /// windows outside that space stay where they were.
    #[test]
    fn a_closed_window_gives_its_space_to_its_neighbour() {
        for gone in 1..=6 {
            let before = opened(6).places(screen(), 8);
            let mut tree = opened(6);
            tree.remove(&gone);
            let after = tree.places(screen(), 8);
            assert_eq!(after.len(), 5);
            let freed = before.iter().find(|(w, _)| *w == gone).unwrap().1;
            let middle =
                freed.loc + smithay::utils::Point::from((freed.size.w / 2, freed.size.h / 2));
            assert!(
                after.iter().any(|(_, r)| r.contains(middle)),
                "nobody took {gone}'s space"
            );
            // Its neighbours are the newer windows, or the one before the
            // newest when the newest closes.
            let kept = if gone == 6 { 4 } else { gone - 1 };
            for (w, r) in after.iter().filter(|(w, _)| *w <= kept) {
                let was = before.iter().find(|(b, _)| b == w).unwrap().1;
                assert_eq!(*r, was, "{w} moved when {gone} closed");
            }
        }
    }

    #[test]
    fn a_window_opens_beside_the_focused_one_and_swaps_trade_places() {
        let mut tree = opened(2);
        // With 1 focused, 3 halves 1's space, not the newest's.
        tree.insert(3, Some(&1));
        assert_eq!(place_of(&tree, 2), rect(644, 8, 628, 784));
        assert_eq!(place_of(&tree, 1), rect(8, 8, 628, 388));
        assert_eq!(place_of(&tree, 3), rect(8, 404, 628, 388));
        tree.swap(&1, &2);
        assert_eq!(place_of(&tree, 1), rect(644, 8, 628, 784));
        assert_eq!(place_of(&tree, 2), rect(8, 8, 628, 388));
        // A window not in the tree opens beside the newest.
        tree.insert(4, Some(&99));
        assert_eq!(tree.places(screen(), 8).len(), 4);
        tree.remove(&1);
        tree.remove(&2);
        tree.remove(&3);
        tree.remove(&4);
        assert!(tree.places(screen(), 8).is_empty());
    }
}
