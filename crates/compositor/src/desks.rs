//! Workspaces (M5.2a): a screen's windows are split among a preset's
//! number of workspaces, one shown at a time. Each keeps its own
//! [`Workspace`] of policies, so one can tile while another floats, and,
//! while hidden, its windows in their stacking order with their frames.
//! Plain data, so it is tested without a display; `workspaces.rs` in the
//! compositor maps and unmaps the windows.

use smithay::utils::{Logical, Rectangle};

use crate::layout::Workspace;

/// The most workspaces a preset can ask for.
pub const MOST: usize = edel::presets::MOST_WORKSPACES;

/// How far the shown windows slide when workspace `to` replaces `from`
/// on a screen `width` wide (M5.2f): off to the left for a higher number,
/// to the right for a lower one, as the workspaces lie in a row; the
/// other workspace's windows come in from the opposite side.
pub fn slide_by(from: usize, to: usize, width: i32) -> i32 {
    if to > from { -width } else { width }
}

/// One workspace.
pub struct Desk<W> {
    pub layout: Workspace<W>,
    /// While it is not shown: its windows, bottom of the stack first, and
    /// each one's frame.
    hidden: Vec<(W, Rectangle<i32, Logical>)>,
}

/// Every workspace, and which one is shown.
pub struct Desks<W> {
    desks: Vec<Desk<W>>,
    active: usize,
    gap: u32,
}

impl<W: Clone + PartialEq + 'static> Desks<W> {
    /// `count` floating workspaces (1 to [`MOST`]), the first shown.
    pub fn new(count: usize, gap: u32) -> Desks<W> {
        let count = count.clamp(1, MOST);
        Desks {
            desks: (0..count)
                .map(|_| Desk {
                    layout: Workspace::new(gap),
                    hidden: Vec::new(),
                })
                .collect(),
            active: 0,
            gap,
        }
    }

    pub fn count(&self) -> usize {
        self.desks.len()
    }

    /// The shown workspace, from 0.
    pub fn active(&self) -> usize {
        self.active
    }

    /// The shown workspace's policies.
    pub fn layout(&self) -> &Workspace<W> {
        &self.desks[self.active].layout
    }

    pub fn layout_mut(&mut self) -> &mut Workspace<W> {
        &mut self.desks[self.active].layout
    }

    /// Every workspace's policies, for a change that applies to all of
    /// them, such as `shell.tiling`.
    pub fn layouts_mut(&mut self) -> impl Iterator<Item = &mut Workspace<W>> {
        self.desks.iter_mut().map(|d| &mut d.layout)
    }

    /// The hidden workspace `window` is on, if it is hidden.
    pub fn hidden_on(&self, window: &W) -> Option<usize> {
        self.desks
            .iter()
            .position(|d| d.hidden.iter().any(|(w, _)| w == window))
    }

    /// Every hidden window with its workspace and its frame.
    pub fn hidden(&self) -> impl Iterator<Item = (usize, &W, Rectangle<i32, Logical>)> {
        self.desks
            .iter()
            .enumerate()
            .flat_map(|(i, d)| d.hidden.iter().map(move |(w, at)| (i, w, *at)))
    }

    /// Shows workspace `to`, hiding `shown`, the windows on screen bottom
    /// first with their frames; returns the windows to show, bottom first
    /// with their frames, or none when `to` is already shown or does not
    /// exist.
    pub fn switch(
        &mut self,
        to: usize,
        shown: Vec<(W, Rectangle<i32, Logical>)>,
    ) -> Option<Vec<(W, Rectangle<i32, Logical>)>> {
        if to == self.active || to >= self.desks.len() {
            return None;
        }
        self.desks[self.active].hidden = shown;
        self.active = to;
        Some(std::mem::take(&mut self.desks[to].hidden))
    }

    /// Moves `window`, shown on this workspace, to workspace `to`, on top
    /// of its windows; its policy there places it, given its frame's
    /// size and the area; returns its frame there. None when `to` is this
    /// workspace or does not exist.
    pub fn send(
        &mut self,
        window: W,
        to: usize,
        size: smithay::utils::Size<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) -> Option<Rectangle<i32, Logical>> {
        if to == self.active || to >= self.desks.len() {
            return None;
        }
        self.desks[self.active].layout.close(&window);
        let place = self.desks[to].layout.open(window.clone(), size, area);
        self.desks[to].hidden.push((window, place));
        Some(place)
    }

    /// `window` closed, on whichever workspace it was.
    pub fn close(&mut self, window: &W) {
        for desk in &mut self.desks {
            desk.layout.close(window);
            desk.hidden.retain(|(w, _)| w != window);
        }
    }

    /// The preset now asks for `count` workspaces: new ones start in
    /// `policy`, and the windows of workspaces that go join the last one
    /// that stays, on top,
    /// placed by its policy in `area` at their frames' sizes.
    /// Returns those that join the shown workspace, to show, bottom first
    /// with their frames; none when the count stays, or when the shown
    /// workspace would go, so the compositor switches away first.
    pub fn set_count(
        &mut self,
        count: usize,
        area: Rectangle<i32, Logical>,
        policy: &str,
    ) -> Option<Vec<(W, Rectangle<i32, Logical>)>> {
        let count = count.clamp(1, MOST);
        if count == self.desks.len() || self.active >= count {
            return None;
        }
        while self.desks.len() < count {
            let mut layout = Workspace::new(self.gap);
            layout.switch(policy);
            self.desks.push(Desk {
                layout,
                hidden: Vec::new(),
            });
        }
        let gone: Vec<Desk<W>> = self.desks.drain(count..).collect();
        let last = count - 1;
        let mut joined = Vec::new();
        for (window, was) in gone.into_iter().flat_map(|d| d.hidden) {
            let frame = self.desks[last].layout.open(window.clone(), was.size, area);
            joined.push((window, frame));
        }
        if last == self.active {
            return Some(joined);
        }
        self.desks[last].hidden.extend(joined);
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 760).into())
    }

    fn at(x: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, 10).into(), (300, 200).into())
    }

    #[test]
    fn switching_hides_the_shown_windows_and_brings_back_the_others() {
        let mut desks: Desks<u32> = Desks::new(4, 8);
        assert_eq!((desks.count(), desks.active()), (4, 0));
        desks.layout_mut().open(1, (300, 200).into(), area());
        desks.layout_mut().open(2, (300, 200).into(), area());
        // To workspace 2 (index 1): 1 and 2 hide, nothing comes back.
        let back = desks.switch(1, vec![(1, at(10)), (2, at(20))]).unwrap();
        assert!(back.is_empty());
        assert_eq!(desks.hidden_on(&2), Some(0));
        assert_eq!(desks.hidden().count(), 2);
        // Back to the first: 1 then 2, bottom first, as they were stacked.
        assert_eq!(
            desks.switch(0, Vec::new()).unwrap(),
            vec![(1, at(10)), (2, at(20))]
        );
        assert_eq!(desks.hidden_on(&1), None);
        assert!(desks.switch(0, Vec::new()).is_none(), "already shown");
        assert!(desks.switch(4, Vec::new()).is_none(), "no fifth");
    }

    #[test]
    fn a_higher_workspace_comes_in_from_the_right() {
        assert_eq!(slide_by(0, 1, 1280), -1280, "the first leaves to the left");
        assert_eq!(slide_by(3, 1, 1280), 1280, "the fourth leaves to the right");
    }

    #[test]
    fn a_window_sent_away_waits_on_top_of_its_new_workspace() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(1, (300, 200).into(), area());
        let place = desks.send(1, 2, (300, 200).into(), area()).unwrap();
        assert_eq!(desks.hidden_on(&1), Some(2));
        assert_eq!(desks.hidden().next().map(|(_, _, p)| p), Some(place));
        assert!(
            desks.send(1, 0, (300, 200).into(), area()).is_none(),
            "already here"
        );
        assert!(
            desks.send(1, 3, (300, 200).into(), area()).is_none(),
            "no fourth"
        );
        desks.close(&1);
        assert_eq!(desks.hidden_on(&1), None);
    }

    #[test]
    fn each_workspace_keeps_its_own_policy() {
        let mut desks: Desks<u32> = Desks::new(2, 8);
        assert!(desks.layout_mut().switch("tiling"));
        desks.switch(1, Vec::new());
        assert_eq!(desks.layout().name(), "floating");
        // shell.tiling switches every workspace.
        for layout in desks.layouts_mut() {
            layout.switch("tiling");
        }
        assert_eq!(desks.layout().name(), "tiling");
    }

    #[test]
    fn fewer_workspaces_move_their_windows_to_the_last_one_left() {
        let mut desks: Desks<u32> = Desks::new(4, 8);
        desks.layout_mut().open(1, (300, 200).into(), area());
        desks.layout_mut().open(2, (300, 200).into(), area());
        desks.send(1, 3, (300, 200).into(), area());
        desks.send(2, 2, (300, 200).into(), area());
        // The shown workspace stays: the compositor switches first.
        desks.switch(3, Vec::new());
        assert!(desks.set_count(2, area(), "floating").is_none());
        // From the second, the last that stays, the third's and the
        // fourth's windows join it on screen.
        desks.switch(1, vec![(1, at(10))]);
        let joined = desks.set_count(2, area(), "floating").unwrap();
        assert_eq!((desks.count(), desks.active()), (2, 1));
        assert_eq!(joined.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [2, 1]);
        // Shown on the first, the windows of the third go to the second.
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(3, (300, 200).into(), area());
        desks.send(3, 2, (300, 200).into(), area());
        assert_eq!(desks.set_count(2, area(), "floating"), Some(Vec::new()));
        assert_eq!(desks.hidden_on(&3), Some(1));
        assert!(
            desks.set_count(2, area(), "floating").is_none(),
            "no change"
        );
        // New workspaces start in the policy everyone has.
        assert!(desks.set_count(12, area(), "tiling").is_some());
        assert_eq!(desks.count(), MOST);
        desks.switch(8, Vec::new());
        assert_eq!(desks.layout().name(), "tiling");
        assert_eq!(Desks::<u32>::new(0, 8).count(), 1);
    }
}
