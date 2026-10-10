//! Workspaces (M5.2a): a screen's windows are split among a preset's
//! number of workspaces, one shown at a time. Each keeps its own
//! [`Workspace`] of policies, so one can tile while another floats, and,
//! while hidden, its windows in their stacking order with their frames.
//! A minimized window (M5.2h) leaves its workspace's policies, so tiling
//! closes its gap, and waits on its workspace, shown or not, until the
//! window list brings it back.
//! Dynamic workspaces (M5.2i): [`dynamic_plan`] says which empty workspaces
//! close and whether one more waits at the end, from the windows each one
//! holds; a named workspace stays when it empties.
//! Workspaces on each screen (M5.2k): with `layout.workspaces_per_screen`
//! each screen shows a workspace of its own, a workspace's windows on a
//! screen show where that screen shows it, and a window put on another
//! screen joins the workspace shown there; else every screen shows the
//! same one.
//! Plain data, so it is tested without a display; `workspaces.rs` in the
//! compositor maps and unmaps the windows.

use smithay::utils::{Logical, Rectangle};

use crate::layout::{Areas, Workspace};
use crate::tiling::Style;

/// The most workspaces a preset can ask for.
pub const MOST: usize = edel::presets::MOST_WORKSPACES;

/// How far the shown windows slide when workspace `to` replaces `from`
/// on a screen `width` wide (M5.2f): off to the left for a higher number,
/// to the right for a lower one, as the workspaces lie in a row; the
/// other workspace's windows come in from the opposite side.
pub fn slide_by(from: usize, to: usize, width: i32) -> i32 {
    if to > from { -width } else { width }
}

/// The workspace after `active` when `forward`, else the one before it,
/// among `count`; none at either end, as Super+Ctrl+Left and Right stop
/// rather than wrap (M5.2i).
pub fn step(active: usize, count: usize, forward: bool) -> Option<usize> {
    if forward {
        (active + 1 < count).then_some(active + 1)
    } else {
        active.checked_sub(1)
    }
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
    /// Its name (M5.2i); empty when it has none, so it shows its number.
    name: String,
}

impl<W> Desk<W> {
    /// An empty workspace with no name, holding `layout`.
    fn new(layout: Workspace<W>) -> Desk<W> {
        Desk {
            layout,
            hidden: Vec::new(),
            minimized: Vec::new(),
            name: String::new(),
        }
    }
}

/// What dynamic workspaces do with the workspaces now (M5.2i): the empty
/// ones that are neither shown nor named close, and one more empty
/// workspace is added at the end unless the last one left is already an
/// empty, unnamed one, as GNOME and COSMIC keep one free.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The workspaces to close, by their index now, ascending.
    pub remove: Vec<usize>,
    /// Whether an empty workspace is added at the end.
    pub add: bool,
}

/// The plan for workspaces holding `counts` windows each, with those in
/// `shown` on a screen and `kept(i)` true for a named workspace `i`. A
/// shown one never closes, and at most [`MOST`] workspaces are kept.
pub fn dynamic_plan(counts: &[usize], shown: &[usize], kept: impl Fn(usize) -> bool) -> Plan {
    let remove: Vec<usize> = (0..counts.len())
        .filter(|&i| counts[i] == 0 && !shown.contains(&i) && !kept(i))
        .collect();
    let remaining: Vec<usize> = (0..counts.len()).filter(|i| !remove.contains(i)).collect();
    let waiting = remaining
        .last()
        .is_some_and(|&last| counts[last] == 0 && !kept(last));
    let add = !waiting && remaining.len() < MOST;
    Plan { remove, add }
}

/// Every workspace, and which one is shown.
pub struct Desks<W> {
    desks: Vec<Desk<W>>,
    /// The workspace in use: shown on every screen, or, with workspaces on
    /// each screen, on the one in use (`here`).
    active: usize,
    gap: u32,
    /// The tiling style every workspace's tiling uses (M5.16).
    style: Style,
    /// Whether each screen shows its own workspace (M5.2k).
    per_screen: bool,
    /// Then the workspace each screen shows, by the screen's name.
    screens: Vec<(String, usize)>,
    /// The screen in use, the pointer's, which shows `active`.
    here: Option<String>,
}

impl<W: Clone + PartialEq + 'static> Desks<W> {
    /// `count` floating workspaces (1 to [`MOST`]), the first shown.
    pub fn new(count: usize, gap: u32) -> Desks<W> {
        let count = count.clamp(1, MOST);
        Desks {
            desks: (0..count).map(|_| Desk::new(Workspace::new(gap))).collect(),
            active: 0,
            gap,
            style: Style::default(),
            per_screen: false,
            screens: Vec::new(),
            here: None,
        }
    }

    /// Every workspace's tiling lays windows out in `style`, now and when
    /// workspaces are added.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
        for desk in &mut self.desks {
            desk.layout.set_style(style);
        }
    }

    pub fn count(&self) -> usize {
        self.desks.len()
    }

    /// The workspace after the shown one, or before it, if there is one.
    pub fn neighbour(&self, forward: bool) -> Option<usize> {
        step(self.active, self.desks.len(), forward)
    }

    /// Names the workspaces, the first one's name first; a workspace with
    /// no name in the list, or an empty one, has none (M5.2i).
    pub fn set_names(&mut self, names: &[String]) {
        for (i, desk) in self.desks.iter_mut().enumerate() {
            desk.name = names.get(i).cloned().unwrap_or_default();
        }
    }

    /// Whether workspace `index` has a name, so dynamic workspaces keep it.
    pub fn named(&self, index: usize) -> bool {
        self.desks.get(index).is_some_and(|d| !d.name.is_empty())
    }

    /// What each workspace is called, by its index: its name, or its number
    /// from 1 when it has none, as the panel's switcher shows it.
    pub fn labels(&self) -> Vec<String> {
        self.desks
            .iter()
            .enumerate()
            .map(|(i, d)| {
                if d.name.is_empty() {
                    (i + 1).to_string()
                } else {
                    d.name.clone()
                }
            })
            .collect()
    }

    /// Each workspace's name as the file says it, empty for none.
    pub fn names(&self) -> Vec<String> {
        self.desks.iter().map(|d| d.name.clone()).collect()
    }

    /// How many windows each workspace holds, shown, hidden or minimized.
    pub fn window_counts(&self) -> Vec<usize> {
        self.desks
            .iter()
            .map(|d| d.layout.windows() + d.minimized.len())
            .collect()
    }

    /// Whether workspace `index` holds no window at all; false for an index
    /// there is no workspace for.
    pub fn is_empty(&self, index: usize) -> bool {
        self.desks
            .get(index)
            .is_some_and(|d| d.layout.is_empty() && d.hidden.is_empty() && d.minimized.is_empty())
    }

    /// Adds an empty workspace at the end, floating or tiling as `policy`
    /// says, with the tiling style every workspace has.
    pub fn add(&mut self, policy: &str) {
        let mut layout = Workspace::new(self.gap);
        layout.switch(policy);
        layout.set_style(self.style);
        self.desks.push(Desk::new(layout));
    }

    /// Takes out workspace `index` if it is empty and not the shown one; the
    /// ones after it move down by one, and the shown one stays shown. False,
    /// and nothing changes, for the shown workspace, one with windows, or
    /// the only one.
    pub fn remove(&mut self, index: usize) -> bool {
        if self.shown().contains(&index) || self.desks.len() < 2 || !self.is_empty(index) {
            return false;
        }
        self.desks.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        for (_, desk) in &mut self.screens {
            if index < *desk {
                *desk -= 1;
            }
        }
        true
    }

    /// Whether each screen shows its own workspace (M5.2k).
    pub fn per_screen(&self) -> bool {
        self.per_screen
    }

    /// Each screen shows its own workspace from now on, or every screen
    /// the same one. Turned on, every screen starts with the one in use;
    /// before it is turned off, the compositor shows the one in use on
    /// every screen (`switch_on`), and any of its windows still hidden,
    /// on a screen that went, are returned to show with their frames.
    pub fn set_per_screen(&mut self, on: bool) -> Vec<(W, Rectangle<i32, Logical>)> {
        self.per_screen = on;
        for (_, desk) in &mut self.screens {
            *desk = self.active;
        }
        if on {
            return Vec::new();
        }
        std::mem::take(&mut self.desks[self.active].hidden)
    }

    /// The workspace screen `name` shows; the one in use for a screen not
    /// seen yet.
    pub fn shown_on(&self, name: &str) -> usize {
        if !self.per_screen {
            return self.active;
        }
        self.screens
            .iter()
            .find(|(n, _)| n == name)
            .map_or(self.active, |(_, desk)| *desk)
    }

    /// Every workspace on a screen, the one in use first.
    pub fn shown(&self) -> Vec<usize> {
        let mut shown = vec![self.active];
        if self.per_screen {
            for (_, desk) in &self.screens {
                if !shown.contains(desk) {
                    shown.push(*desk);
                }
            }
        }
        shown
    }

    /// The pointer is on screen `name`, which is in use from now on: its
    /// workspace is the one Super+1 to Super+9, the policy toggle and new
    /// windows act on. True when that is another workspace than before.
    pub fn use_screen(&mut self, name: &str) -> bool {
        if self.here.as_deref() == Some(name) {
            return false;
        }
        self.here = Some(name.to_string());
        let before = self.active;
        self.active = self.shown_on(name);
        self.active != before
    }

    /// The workspace holding `window`, shown, hidden or minimized.
    pub fn desk_of(&self, window: &W) -> Option<usize> {
        self.desks.iter().position(|d| {
            d.layout.screen_of(window).is_some() || d.minimized.iter().any(|(w, ..)| w == window)
        })
    }

    /// The policies of the workspace holding `window`, else the one in use.
    pub fn layout_of(&self, window: &W) -> &Workspace<W> {
        let desk = self.desk_of(window).unwrap_or(self.active);
        &self.desks[desk].layout
    }

    /// The policies of the workspace screen `name` shows, where a window
    /// opening there goes.
    pub fn layout_on_mut(&mut self, name: &str) -> &mut Workspace<W> {
        let desk = self.shown_on(name);
        &mut self.desks[desk].layout
    }

    pub fn layout_of_mut(&mut self, window: &W) -> &mut Workspace<W> {
        let desk = self.desk_of(window).unwrap_or(self.active);
        &mut self.desks[desk].layout
    }

    /// With workspaces on each screen, shows workspace `to` on screen
    /// `name`, hiding `shown`, the windows on that screen bottom first
    /// with their frames, and returns `to`'s windows on that screen to
    /// show; else as [`Desks::switch`] does for every screen. None when
    /// `to` is already shown there or does not exist.
    pub fn switch_on(
        &mut self,
        to: usize,
        name: &str,
        shown: Vec<(W, Rectangle<i32, Logical>)>,
    ) -> Option<Vec<(W, Rectangle<i32, Logical>)>> {
        if !self.per_screen {
            return self.switch(to, shown);
        }
        let from = self.shown_on(name);
        if to == from || to >= self.desks.len() {
            return None;
        }
        self.desks[from].hidden.extend(shown);
        match self.screens.iter_mut().find(|(n, _)| n == name) {
            Some((_, desk)) => *desk = to,
            None => self.screens.push((name.to_string(), to)),
        }
        if self.here.as_deref().is_none_or(|here| here == name) {
            self.active = to;
        }
        let desk = &mut self.desks[to];
        let (back, stay) = std::mem::take(&mut desk.hidden)
            .into_iter()
            .partition(|(w, _)| desk.layout.screen_of(w) == Some(name));
        desk.hidden = stay;
        Some(back)
    }

    /// Where the policies put the windows on screen now, given each
    /// screen's area in `areas`: the workspace in use's, or, with
    /// workspaces on each screen, each screen's own workspace's there.
    pub fn arrange(&mut self, areas: &Areas) -> Vec<(W, Rectangle<i32, Logical>)> {
        if !self.per_screen {
            return self.desks[self.active].layout.arrange(areas);
        }
        let mut placed = Vec::new();
        for desk in self.shown() {
            for (window, frame) in self.desks[desk].layout.arrange(areas) {
                let d = &self.desks[desk];
                let here = d
                    .layout
                    .screen_of(&window)
                    .is_some_and(|name| self.shown_on(name) == desk);
                if here && !d.hidden.iter().any(|(w, _)| *w == window) {
                    placed.push((window, frame));
                }
            }
        }
        placed
    }

    /// With workspaces on each screen, after the screens changed or a
    /// window was put on another screen: the screens in `areas` are the
    /// ones there are, a window in `on_screen` (with its frame) whose
    /// screen shows another workspace joins that one, where it is, and a
    /// hidden window whose screen shows its workspace comes back; those
    /// are returned, bottom first with their frames, to show. A window
    /// whose screen went counts as on the first. Nothing with every screen
    /// showing the same workspace.
    pub fn settle_screens(
        &mut self,
        on_screen: &[(W, Rectangle<i32, Logical>)],
        areas: &Areas,
    ) -> Vec<(W, Rectangle<i32, Logical>)> {
        if !self.per_screen {
            return Vec::new();
        }
        let active = self.active;
        self.screens
            .retain(|(n, _)| areas.iter().any(|(a, _)| a == n));
        for (name, _) in areas {
            if !self.screens.iter().any(|(n, _)| n == name) {
                self.screens.push((name.clone(), active));
            }
        }
        if self
            .here
            .as_ref()
            .is_some_and(|here| !areas.iter().any(|(a, _)| a == here))
        {
            // The pointer goes to the first screen, and so does the use.
            self.here = None;
            if let Some((first, _)) = areas.first() {
                self.active = self.shown_on(first);
            }
        }
        for (window, frame) in on_screen {
            let Some(from) = self.desk_of(window) else {
                continue;
            };
            let Some((name, area)) = screen_in(areas, self.desks[from].layout.screen_of(window))
            else {
                continue;
            };
            let to = self.shown_on(name);
            if to == from {
                continue;
            }
            self.desks[from].layout.close(window);
            let layout = &mut self.desks[to].layout;
            layout.open(window.clone(), frame.size, name, area);
            layout.moved(window, *frame, name, area);
        }
        let mut back = Vec::new();
        for i in 0..self.desks.len() {
            let hidden = std::mem::take(&mut self.desks[i].hidden);
            for (window, frame) in hidden {
                let screen = self.desks[i].layout.screen_of(&window);
                let gone = screen.is_none_or(|s| !areas.iter().any(|(a, _)| a == s));
                match screen_in(areas, screen) {
                    Some((name, area)) if self.shown_on(name) == i => {
                        if gone {
                            let layout = &mut self.desks[i].layout;
                            layout.close(&window);
                            layout.open(window.clone(), frame.size, name, area);
                        }
                        back.push((window, frame));
                    }
                    _ => self.desks[i].hidden.push((window, frame)),
                }
            }
        }
        back
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
    /// them, such as `layout.tiling`.
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

    /// The minimized windows of the workspaces on screen, each shown
    /// where its screen shows its workspace, the latest last.
    pub fn minimized_here(&self) -> impl Iterator<Item = &W> {
        self.desks.iter().enumerate().flat_map(move |(i, d)| {
            d.minimized
                .iter()
                .filter(move |(_, _, screen)| self.shown_on(screen.as_deref().unwrap_or("")) == i)
                .map(|(w, ..)| w)
        })
    }

    /// The frame `window` had, if it is minimized on a workspace its
    /// screen shows.
    pub fn minimized_shown(&self, window: &W) -> Option<Rectangle<i32, Logical>> {
        self.desks.iter().enumerate().find_map(|(i, d)| {
            d.minimized
                .iter()
                .find(|(w, ..)| w == window)
                .filter(|(_, _, screen)| self.shown_on(screen.as_deref().unwrap_or("")) == i)
                .map(|(_, at, _)| *at)
        })
    }

    pub fn minimize(&mut self, window: W, frame: Rectangle<i32, Logical>) {
        let desk = self.desk_of(&window).unwrap_or(self.active);
        let desk = &mut self.desks[desk];
        let screen = desk.layout.screen_of(&window).map(str::to_string);
        desk.layout.close(&window);
        desk.minimized.retain(|(w, ..)| *w != window);
        desk.minimized.push((window, frame, screen));
    }

    /// Brings back `window`, minimized, to its screen in `areas` (else
    /// the first): its policy places it where it was if it can
    /// (floating), else where it says (a tile). Returns its frame; none
    /// when it is not minimized or there is no screen.
    pub fn restore(&mut self, window: &W, areas: &Areas) -> Option<Rectangle<i32, Logical>> {
        let (desk, _) = self.minimized(window)?;
        let desk = &mut self.desks[desk];
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
        self.desks[self.active].hidden.extend(shown);
        self.active = to;
        Some(std::mem::take(&mut self.desks[to].hidden))
    }

    /// Moves `window`, shown, to workspace `to`, on top of its windows on
    /// the same screen; its policy there places it, given its frame's
    /// size and the screen's area in `areas`; returns its frame there.
    /// None when `to` is its workspace or does not exist, when it is not
    /// shown, or there is no screen.
    pub fn send(
        &mut self,
        window: W,
        to: usize,
        size: smithay::utils::Size<i32, Logical>,
        areas: &Areas,
    ) -> Option<Rectangle<i32, Logical>> {
        let desk = self.desk_of(&window).unwrap_or(self.active);
        if to == desk || to >= self.desks.len() || self.hidden_on(&window).is_some() {
            return None;
        }
        let from = self.desks[desk].layout.screen_of(&window);
        let (screen, area) = screen_in(areas, from)?;
        self.desks[desk].layout.close(&window);
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
        if count == self.desks.len() || self.shown().iter().any(|&d| d >= count) {
            return None;
        }
        while self.desks.len() < count {
            self.add(policy);
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
        // With workspaces on each screen, `settle_screens` shows those
        // whose screen shows the last.
        if last == self.active && !self.per_screen {
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
            "not shown"
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
        // layout.tiling switches every workspace.
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

    #[test]
    fn one_window_gives_two_workspaces_and_closing_it_gives_one() {
        let none = |_: usize| false;
        assert_eq!(
            dynamic_plan(&[1], &[0], none),
            Plan {
                remove: Vec::new(),
                add: true
            }
        );
        // Its window closed, the empty second one closes too, and the
        // first, empty and shown, is the one that waits.
        assert_eq!(
            dynamic_plan(&[0, 0], &[0], none),
            Plan {
                remove: vec![1],
                add: false
            }
        );
        assert_eq!(dynamic_plan(&[0], &[0], none), Plan::default());
    }

    #[test]
    fn a_named_empty_workspace_in_the_middle_stays() {
        let named_second = |i: usize| i == 1;
        assert_eq!(
            dynamic_plan(&[1, 0, 1], &[0], named_second),
            Plan {
                remove: Vec::new(),
                add: true
            }
        );
        // The unnamed empty one after it closes, and the named one is
        // the last left, so one more waits after it.
        assert_eq!(
            dynamic_plan(&[1, 0, 0], &[0], named_second),
            Plan {
                remove: vec![2],
                add: true
            }
        );
    }

    #[test]
    fn the_shown_workspace_never_closes_and_nine_full_ones_add_none() {
        let none = |_: usize| false;
        // Shown and empty, the second stays, the first closes.
        assert_eq!(
            dynamic_plan(&[0, 0, 1], &[1], none),
            Plan {
                remove: vec![0],
                add: true
            }
        );
        assert_eq!(dynamic_plan(&[1, 0], &[1], none), Plan::default());
        assert_eq!(dynamic_plan(&[1; MOST], &[0], none), Plan::default());
    }

    #[test]
    fn an_empty_workspace_before_the_shown_one_closes_and_the_shown_one_stays_shown() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.switch(2, Vec::new());
        assert!(!desks.remove(2), "the shown one");
        assert!(desks.remove(0));
        assert_eq!((desks.count(), desks.active()), (2, 1));
        assert!(desks.remove(0));
        assert_eq!((desks.count(), desks.active()), (1, 0));
        assert!(!desks.remove(0), "the only one");
    }

    #[test]
    fn a_workspace_with_windows_does_not_close() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        desks.send(1, 1, (300, 200).into(), &areas());
        assert!(!desks.is_empty(1));
        assert!(!desks.remove(1), "a hidden window is on it");
        assert!(desks.is_empty(2));
        assert!(desks.remove(2));
        assert_eq!(desks.window_counts(), [0, 1]);
        desks.switch(1, Vec::new());
        desks.minimize(1, at(40));
        desks.switch(0, Vec::new());
        assert!(!desks.remove(1), "a minimized window is on it");
        assert_eq!(desks.window_counts(), [0, 1]);
    }

    #[test]
    fn names_stay_with_their_workspace_and_the_others_show_their_number() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.set_names(&["Mail".to_string()]);
        assert_eq!(desks.labels(), ["Mail", "2", "3"]);
        assert!(desks.named(0));
        assert!(!desks.named(1));
        assert_eq!(desks.names(), ["Mail", "", ""]);
        // The second closes; the named first is untouched.
        assert!(desks.remove(1));
        assert_eq!(
            desks.labels(),
            ["Mail", "2"],
            "a number is the position now"
        );
        assert!(!desks.named(9), "no workspace there");
        // Set again by position, none for the rest.
        desks.set_names(&[String::new(), "Code".to_string()]);
        assert_eq!(desks.labels(), ["1", "Code"]);
    }

    fn two() -> Vec<(String, Rectangle<i32, Logical>)> {
        let right = Rectangle::new((1280, 0).into(), (1024, 768).into());
        vec![("one".to_string(), area()), ("two".to_string(), right)]
    }

    fn right(x: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((1280 + x, 10).into(), (300, 200).into())
    }

    #[test]
    fn each_screen_switches_on_its_own_and_together_again() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.set_per_screen(true);
        assert!(desks.settle_screens(&[], &two()).is_empty());
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        desks
            .layout_mut()
            .open(2, (300, 200).into(), "two", two()[1].1);
        // The pointer on the second screen: Super+2 shows the second
        // workspace there alone, hiding window 2.
        assert!(!desks.use_screen("two"), "both show the first");
        let back = desks.switch_on(1, "two", vec![(2, right(10))]).unwrap();
        assert!(back.is_empty());
        assert_eq!((desks.shown_on("one"), desks.shown_on("two")), (0, 1));
        assert_eq!(desks.active(), 1);
        assert_eq!(desks.shown(), [1, 0]);
        assert_eq!(desks.hidden_on(&2), Some(0));
        // Only window 1 is laid out: 2 waits on the second screen.
        let placed = desks.arrange(&two());
        assert_eq!(placed.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [1]);
        // The first screen in use again shows the first workspace.
        assert!(desks.use_screen("one"));
        assert_eq!(desks.active(), 0);
        assert!(
            desks.switch_on(0, "one", Vec::new()).is_none(),
            "shown there"
        );
        // A window sent away from the first screen lands on the third
        // workspace, still on its screen.
        desks.send(1, 2, (300, 200).into(), &two()).unwrap();
        assert_eq!(desks.desk_of(&1), Some(2));
        // Back on the second screen, Super+1 brings window 2 back there.
        desks.use_screen("two");
        assert_eq!(
            desks.switch_on(0, "two", Vec::new()).unwrap(),
            vec![(2, right(10))]
        );
        // Off: the screens follow the one in use, which the compositor
        // shows on each first.
        desks.set_per_screen(false);
        assert_eq!((desks.shown_on("one"), desks.shown_on("two")), (0, 0));
        assert_eq!(desks.shown(), [0]);
    }

    #[test]
    fn a_window_put_on_another_screen_joins_the_workspace_shown_there() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.set_per_screen(true);
        desks.settle_screens(&[], &two());
        desks.use_screen("two");
        desks.switch_on(2, "two", Vec::new());
        desks.use_screen("one");
        desks.layout_mut().open(1, (300, 200).into(), "one", area());
        // Dragged onto the second screen, it is on the third workspace.
        desks
            .layout_of_mut(&1)
            .moved(&1, right(40), "two", two()[1].1);
        assert!(desks.settle_screens(&[(1, right(40))], &two()).is_empty());
        assert_eq!(desks.desk_of(&1), Some(2));
        assert_eq!(desks.layout_of(&1).screen_of(&1), Some("two"));
        // Every workspace shown somewhere is kept by dynamic workspaces.
        assert_eq!(
            dynamic_plan(&[0, 0, 1], &desks.shown(), |_| false).remove,
            [1]
        );
        assert!(!desks.remove(2), "shown on the second screen");
    }

    #[test]
    fn a_screen_that_goes_brings_its_windows_to_the_first() {
        let mut desks: Desks<u32> = Desks::new(3, 8);
        desks.set_per_screen(true);
        desks.settle_screens(&[], &two());
        desks
            .layout_mut()
            .open(1, (300, 200).into(), "two", two()[1].1);
        desks.use_screen("two");
        // Window 1 hides on the second screen behind the second workspace.
        desks.switch_on(1, "two", vec![(1, right(0))]);
        desks
            .layout_mut()
            .open(2, (300, 200).into(), "two", two()[1].1);
        // The second screen goes. Window 2, shown there on the second
        // workspace, joins the first screen's, and window 1, whose
        // workspace the first shows, comes back.
        let back = desks.settle_screens(&[(2, right(20))], &areas());
        assert_eq!(back, vec![(1, right(0))]);
        assert_eq!(desks.desk_of(&2), Some(0));
        assert_eq!(desks.layout_of(&1).screen_of(&1), Some("one"));
        assert_eq!(desks.shown(), [0]);
        assert_eq!(desks.arrange(&areas()).len(), 2);
    }

    #[test]
    fn the_shortcuts_step_one_workspace_and_stop_at_the_ends() {
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(1, 3, true), Some(2));
        assert_eq!(step(2, 3, true), None, "the last does not wrap");
        assert_eq!(step(1, 3, false), Some(0));
        assert_eq!(step(0, 3, false), None, "the first does not wrap");
        let mut desks: Desks<u32> = Desks::new(2, 8);
        assert_eq!(desks.neighbour(true), Some(1));
        assert_eq!(desks.neighbour(false), None);
        desks.switch(1, Vec::new());
        assert_eq!(desks.neighbour(true), None);
    }
}
