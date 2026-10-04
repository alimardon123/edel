//! Workspaces (M5.2a): a screen's windows are split among a preset's
//! number of workspaces, one shown at a time. Each keeps its own
//! [`Workspace`] of policies, so one can tile while another floats, and,
//! while hidden, its windows in their stacking order with their frames.
//! A minimized window (M5.2h) leaves its workspace's policies, so tiling
//! closes its gap, and waits on its workspace, shown or not, until the
//! window list brings it back.
//! Plain data, so it is tested without a display; `workspaces.rs` in the
//! compositor maps and unmaps the windows.

use smithay::utils::{Logical, Rectangle};

use crate::layout::{Areas, Workspace};

/// The most workspaces a preset can ask for.
pub const MOST: usize = edel::presets::MOST_WORKSPACES;

/// How far the shown windows slide when workspace `to` replaces `from`
/// on a screen `width` wide (M5.2f): off to the left for a higher number,
/// to the right for a lower one, as the workspaces lie in a row; the
/// other workspace's windows come in from the opposite side.
pub fn slide_by(from: usize, to: usize, width: i32) -> i32 {
    if to > from { -width } else { width }
}

/// The screen called `name` in `areas`, else the first, with its area.
pub fn screen_in<'a>(
    areas: &'a Areas,
    name: Option<&str>,
) -> Option<(&'a str, Rectangle<i32, Logical>)> {
    areas
        .iter()
        .find(|(n, _)| Some(n.as_str()) == name)
        .or(areas.first())
        .map(|(n, area)| (n.as_str(), *area))
}

/// One workspace.
pub struct Desk<W> {
    pub layout: Workspace<W>,
    /// While it is not shown: its windows, bottom of the stack first, and
    /// each one's frame.
    hidden: Vec<(W, Rectangle<i32, Logical>)>,
    /// Its minimized windows, each with the frame it had and its screen.
    minimized: Vec<(W, Rectangle<i32, Logical>, Option<String>)>,
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
                    minimized: Vec::new(),
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

    /// The workspace `window` is on, if it is off screen: on a hidden
    /// workspace, or minimized.
    pub fn hidden_on(&self, window: &W) -> Option<usize> {
        self.desks.iter().position(|d| {
            d.hidden.iter().any(|(w, _)| w == window)
                || d.minimized.iter().any(|(w, ..)| w == window)
        })
    }

    /// Every window off screen with its workspace and its frame: those on
    /// hidden workspaces, then the minimized ones, workspace by workspace.
    pub fn hidden(&self) -> impl Iterator<Item = (usize, &W, Rectangle<i32, Logical>)> {
        self.desks.iter().enumerate().flat_map(|(i, d)| {
            d.hidden
                .iter()
                .map(move |(w, at)| (i, w, *at))
                .chain(d.minimized.iter().map(move |(w, at, _)| (i, w, *at)))
        })
    }

    /// The workspace `window` is minimized on, and the frame it had.
    pub fn minimized(&self, window: &W) -> Option<(usize, Rectangle<i32, Logical>)> {
        self.desks.iter().enumerate().find_map(|(i, d)| {
            d.minimized
                .iter()
                .find(|(w, ..)| w == window)
                .map(|(_, at, _)| (i, *at))
        })
    }

    /// Minimizes `window`, shown on this workspace with its frame at
    /// `frame`: it leaves the policies, so the others may fill its place.
    /// The screen a minimized window comes back on, by name.
    pub fn minimized_screen(&self, window: &W) -> Option<&str> {
        self.desks.iter().find_map(|d| {
            d.minimized
                .iter()
                .find(|(w, ..)| w == window)
                .and_then(|(_, _, screen)| screen.as_deref())
        })
    }

    /// The shown workspace's minimized windows, the latest last.
    pub fn minimized_here(&self) -> impl Iterator<Item = &W> {
        self.desks[self.active].minimized.iter().map(|(w, ..)| w)
    }

    pub fn minimize(&mut self, window: W, frame: Rectangle<i32, Logical>) {
        let desk = &mut self.desks[self.active];
        let screen = desk.layout.screen_of(&window).map(str::to_string);
        desk.layout.close(&window);
        desk.minimized.retain(|(w, ..)| *w != window);
        desk.minimized.push((window, frame, screen));
    }

    /// Brings back `window`, minimized on this workspace, to its screen
    /// in `areas` (else the first): its policy places it where it was if
    /// it can (floating), else where it says (a tile). Returns its frame;
    /// none when it is not minimized here or there is no screen.
    pub fn restore(&mut self, window: &W, areas: &Areas) -> Option<Rectangle<i32, Logical>> {
        let desk = &mut self.desks[self.active];
        let i = desk.minimized.iter().position(|(w, ..)| w == window)?;
        let (screen, area) = screen_in(areas, desk.minimized[i].2.as_deref())?;
        let (window, frame, _) = desk.minimized.remove(i);
        desk.layout.open(window.clone(), frame.size, screen, area);
        Some(desk.layout.moved(&window, frame, screen, area))
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
    /// of its windows on the same screen; its policy there places it,
    /// given its frame's size and the screen's area in `areas`; returns
    /// its frame there. None when `to` is this workspace or does not
    /// exist, or there is no screen.
    pub fn send(
        &mut self,
        window: W,
        to: usize,
        size: smithay::utils::Size<i32, Logical>,
        areas: &Areas,
    ) -> Option<Rectangle<i32, Logical>> {
        if to == self.active || to >= self.desks.len() {
            return None;
        }
        let from = self.desks[self.active].layout.screen_of(&window);
        let (screen, area) = screen_in(areas, from)?;
        self.desks[self.active].layout.close(&window);
        let place = self.desks[to]
            .layout
            .open(window.clone(), size, screen, area);
        self.desks[to].hidden.push((window, place));
        Some(place)
    }

    /// `window` closed, on whichever workspace it was.
    pub fn close(&mut self, window: &W) {
        for desk in &mut self.desks {
            desk.layout.close(window);
            desk.hidden.retain(|(w, _)| w != window);
            desk.minimized.retain(|(w, ..)| w != window);
        }
    }

    /// The preset now asks for `count` workspaces: new ones start in
    /// `policy`, and the windows of workspaces that go join the last one
    /// that stays, on top, each on its screen in `areas` (else the
    /// first), placed by its policy at its frame's size; their minimized
    /// windows stay minimized there.
    /// Returns those that join the shown workspace, to show, bottom first
    /// with their frames; none when the count stays, or when the shown
    /// workspace would go, so the compositor switches away first.
    pub fn set_count(
        &mut self,
        count: usize,
        areas: &Areas,
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
                minimized: Vec::new(),
            });
        }
        let gone: Vec<Desk<W>> = self.desks.drain(count..).collect();
        let last = count - 1;
        let mut joined = Vec::new();
        let mut minimized = Vec::new();
        for desk in gone {
            minimized.extend(desk.minimized);
            for (window, was) in desk.hidden {
                let screen = desk.layout.screen_of(&window).map(str::to_string);
                joined.push((window, was, screen));
            }
        }
        self.desks[last].minimized.extend(minimized);
        let mut placed = Vec::new();
        for (window, was, screen) in joined {
            // With no screen at all, it keeps its frame on its own screen,
            // and moves to the first that comes when they are laid out.
            let (screen, area) = match screen_in(areas, screen.as_deref()) {
                Some((name, area)) => (name.to_string(), area),
                None => (screen.unwrap_or_default(), was),
            };
            let frame = self.desks[last]
                .layout
                .open(window.clone(), was.size, &screen, area);
            placed.push((window, frame));
        }
        if last == self.active {
            return Some(placed);
        }
        self.desks[last].hidden.extend(placed);
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 760).into())
    }

    fn areas() -> Vec<(String, Rectangle<i32, Logical>)> {
        vec![("one".into(), area())]
    }

    fn at(x: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, 10).into(), (300, 200).into())
    }

    #[test]
    fn switching_hides_the_shown_windows_and_brings_back_the_others() {
        let mut desks: Desks<u32> = Desks::new(4, 8);
        assert_eq!((desks.count(), desks.active()), (4, 0));
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        desks.layout_mut().open(2, (300, 200).into(), "one", area());
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
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        let place = desks.send(1, 2, (300, 200).into(), &areas()).unwrap();
        assert_eq!(desks.hidden_on(&1), Some(2));
        assert_eq!(desks.hidden().next().map(|(_, _, p)| p), Some(place));
        assert!(
            desks.send(1, 0, (300, 200).into(), &areas()).is_none(),
            "already here"
        );
        assert!(
            desks.send(1, 3, (300, 200).into(), &areas()).is_none(),
            "no fourth"
        );
        desks.close(&1);
        assert_eq!(desks.hidden_on(&1), None);
    }

    #[test]
    fn a_minimized_window_waits_on_its_workspace_until_it_comes_back() {
        let mut desks: Desks<u32> = Desks::new(2, 8);
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        desks.layout_mut().open(2, (300, 200).into(), "one", area());
        desks.minimize(1, at(40));
        assert_eq!(desks.minimized(&1), Some((0, at(40))));
        assert_eq!(desks.hidden_on(&1), Some(0), "off screen");
        // It stays minimized across a switch, and is not shown on return.
        desks.switch(1, vec![(2, at(20))]);
        assert_eq!(desks.switch(0, Vec::new()).unwrap(), vec![(2, at(20))]);
        assert_eq!(desks.minimized(&1), Some((0, at(40))));
        // Floating, it comes back where it was.
        assert_eq!(desks.restore(&1, &areas()), Some(at(40)));
        assert_eq!(desks.minimized(&1), None);
        assert_eq!(desks.hidden_on(&1), None);
        assert_eq!(desks.restore(&1, &areas()), None, "not minimized");
        // Tiling, the other fills the screen while it is away, and it
        // comes back to a tile.
        desks.layout_mut().switch("tiling");
        desks.minimize(1, at(40));
        let tiles = desks.layout_mut().arrange(&areas());
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].0, 2);
        let back = desks.restore(&1, &areas()).unwrap();
        assert_ne!(back, at(40));
        assert_eq!(desks.layout_mut().arrange(&areas()).len(), 2);
        // Closed while minimized, it is gone.
        desks.minimize(1, at(40));
        desks.close(&1);
        assert_eq!(desks.minimized(&1), None);
    }

    #[test]
    fn fewer_workspaces_with_no_screen_keep_their_windows() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        let there = desks.send(1, 2, (300, 200).into(), &areas()).unwrap();
        // Every screen gone: the window joins workspace 2, hidden, where
        // it was.
        assert_eq!(desks.set_count(2, &[], "floating"), Some(Vec::new()));
        assert_eq!(desks.hidden_on(&1), Some(1));
        assert!(
            desks
                .hidden()
                .any(|(i, w, at)| (i, *w, at) == (1, 1, there))
        );
        // A screen comes back: the window moves onto it.
        desks.switch(1, Vec::new());
        assert_eq!(desks.layout_mut().arrange(&areas()).len(), 1);
        assert_eq!(desks.layout().screen_of(&1), Some("one"));
    }

    #[test]
    fn a_window_keeps_its_screen_when_sent_minimized_or_joined() {
        let right = Rectangle::new((1280, 0).into(), (1024, 768).into());
        let both = vec![("one".to_string(), area()), ("two".to_string(), right)];
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(1, (300, 200).into(), "two", right);
        desks.layout_mut().open(2, (300, 200).into(), "two", right);
        // Sent to the second workspace, it lands on the same screen.
        let place = desks.send(1, 1, (300, 200).into(), &both).unwrap();
        assert!(place.loc.x >= 1280, "{place:?}");
        // Minimized, it comes back to its screen even if the first is
        // named first.
        desks.minimize(2, Rectangle::new((1400, 50).into(), (300, 200).into()));
        let back = desks.restore(&2, &both).unwrap();
        assert_eq!(back.loc, (1400, 50).into());
        assert_eq!(desks.layout().screen_of(&2), Some("two"));
        // Its screen gone, it comes back to the first.
        desks.minimize(2, Rectangle::new((1400, 50).into(), (300, 200).into()));
        let one = vec![("one".to_string(), area())];
        desks.restore(&2, &one).unwrap();
        assert_eq!(desks.layout().screen_of(&2), Some("one"));
        assert_eq!(desks.restore(&2, &[]), None, "not minimized");
        // Joining the workspace that stays, each keeps its screen: 1 the
        // second, 2 the first, where it came back.
        desks.send(2, 2, (300, 200).into(), &both);
        let joined = desks.set_count(1, &both, "floating").unwrap();
        assert_eq!(desks.count(), 1);
        let x = |w: u32| joined.iter().find(|(j, _)| *j == w).unwrap().1.loc.x;
        assert!(x(1) >= 1280 && x(2) < 1280, "{joined:?}");
        assert_eq!(desks.layout().screen_of(&1), Some("two"));
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
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        desks.layout_mut().open(2, (300, 200).into(), "one", area());
        desks.send(1, 3, (300, 200).into(), &areas());
        desks.send(2, 2, (300, 200).into(), &areas());
        // The shown workspace stays: the compositor switches first.
        desks.switch(3, Vec::new());
        assert!(desks.set_count(2, &areas(), "floating").is_none());
        // From the second, the last that stays, the third's and the
        // fourth's windows join it on screen.
        desks.switch(1, vec![(1, at(10))]);
        let joined = desks.set_count(2, &areas(), "floating").unwrap();
        assert_eq!((desks.count(), desks.active()), (2, 1));
        assert_eq!(joined.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [2, 1]);
        // Shown on the first, the windows of the third go to the second.
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(3, (300, 200).into(), "one", area());
        desks.send(3, 2, (300, 200).into(), &areas());
        assert_eq!(desks.set_count(2, &areas(), "floating"), Some(Vec::new()));
        assert_eq!(desks.hidden_on(&3), Some(1));
        assert!(
            desks.set_count(2, &areas(), "floating").is_none(),
            "no change"
        );
        // A minimized window of a workspace that goes stays minimized.
        desks.set_count(3, &areas(), "floating");
        desks.switch(2, Vec::new());
        desks.layout_mut().open(5, (300, 200).into(), "one", area());
        desks.minimize(5, at(50));
        desks.switch(0, Vec::new());
        assert!(desks.set_count(2, &areas(), "floating").is_some());
        assert_eq!(desks.minimized(&5), Some((1, at(50))));
        // New workspaces start in the policy everyone has.
        assert!(desks.set_count(12, &areas(), "tiling").is_some());
        assert_eq!(desks.count(), MOST);
        desks.switch(8, Vec::new());
        assert_eq!(desks.layout().name(), "tiling");
        assert_eq!(Desks::<u32>::new(0, 8).count(), 1);
    }
}
