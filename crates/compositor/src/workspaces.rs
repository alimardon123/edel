//! Workspaces on screen (roadmap M5.2a): Super+1 to Super+9 show one,
//! Super+Shift+1 to 9 move the focused window to one, and the preset says
//! how many (`edel_compositor::desks` keeps them). A hidden workspace's
//! windows leave the `Space`, so they are not drawn and get no frame
//! callbacks; each workspace keeps its stacking order and its own policy.
//! Nothing happens during a drag, whose grab holds a shown window. A
//! switch is one spring per screen (M5.2f, M5.14) that the shown windows'
//! pictures and the other workspace's windows share: the pictures leave
//! towards one side while the windows come in from the other, as the
//! workspaces lie in a row. Three fingers on a touchpad hold that spring and
//! move the workspaces one to one (`swipe_moved`); on Lite, whose slides take
//! no time, a switch is instant. Minimizing (M5.2h) hides a
//! window the same way, on its own workspace, until the window list
//! brings it back. With `workspaces.per_screen` (M5.2k) each
//! screen shows its own workspace: Super+1 to Super+9 switch the screen
//! the pointer is on, and a window put on another screen joins the
//! workspace shown there.

use std::time::Duration;

use smithay::desktop::Window;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use edel::places;
use edel::settings::{self, WORKSPACE_NAMES, WORKSPACE_WALLPAPERS};
use edel_compositor::animation::{SWIPE_START, swipe_goes, swipe_offset};
use edel_compositor::desks::{MOST, dynamic_plan, names_to_write, reordered_names, slide_by};
use edel_compositor::messages;

use crate::decoration::{data, title};
use crate::state::Edel;

/// A touchpad swipe under way (M5.2j-b4, M5.14): its fingers, how far
/// they slid across and down, and, once three took the workspaces, the
/// switch they hold.
pub struct Swipe {
    /// How many fingers are on the touchpad.
    pub fingers: u32,
    /// How far they slid across, in touchpad pixels, from where they began.
    pub dx: f64,
    /// How far they slid down, in touchpad pixels; up is negative.
    pub dy: f64,
    /// The workspaces three fingers hold, once they took them.
    pub held: Option<Held>,
}

/// The workspaces three fingers hold: which screen's switch, its `by` along
/// x, the screen's width, and the offset and speed the fingers last gave it
/// with when.
pub struct Held {
    /// The screen whose switch it is, by name, or `None` for every screen.
    key: Option<String>,
    /// How far the switch moves the workspace along x, as `slide_by` gives.
    by_x: f64,
    /// The screen's width, which the fingers' slide is measured against.
    width: f64,
    /// Where the switch is held along x, as an offset from its place.
    offset: f64,
    /// How fast the fingers moved it along x, in logical px a second.
    velocity: f64,
    /// When the fingers last moved it, on the animations' clock.
    at: Duration,
    /// The workspace shown before the swipe, which a swipe back returns to.
    from: usize,
}

impl Edel {
    /// Shows workspace `to`, from 0, if it is not shown: on every screen,
    /// or, with workspaces on each screen, on the pointer's. Nothing
    /// happens while three fingers hold the workspaces (M5.14).
    pub fn switch_workspace(&mut self, to: usize) {
        if self.dragging || self.swipe.as_ref().is_some_and(|s| s.held.is_some()) {
            return;
        }
        if self.desks.per_screen() {
            if let Some((name, _)) = self.pointer_screen() {
                self.desks.use_screen(&name);
                self.switch_screen(to, &name);
            }
            return;
        }
        let shown: Vec<(Window, Rectangle<i32, Logical>)> = self
            .space
            .elements()
            .filter_map(|window| Some((window.clone(), self.frame_of(window)?)))
            .collect();
        let from = self.desks.active();
        let Some(back) = self.desks.switch(to, shown.clone()) else {
            return;
        };
        let width = self.window_area().map_or(0, |area| area.size.w);
        let by = Point::from((slide_by(from, to, width), 0));
        // One switch for every screen, so its key is `None`.
        let key: Option<String> = None;
        let slid = self.animations.begin_switch(&key, by.to_f64());
        for (window, _) in &shown {
            if slid {
                self.snapshot_leaving(window, by.to_f64(), key.clone());
            }
            self.hide(window);
        }
        // Each comes in from a screen's width away, with the switch.
        for (window, frame) in back {
            self.show(window, frame);
        }
        eprintln!("edel-compositor: workspace {}", to + 1);
        self.focus_top();
        self.relayout();
        self.repoint();
        self.settle_dynamic();
        self.announce_workspaces();
    }

    /// With workspaces on each screen, shows workspace `to` on screen
    /// `name` alone, its windows there sliding in as a switch's do.
    pub fn switch_screen(&mut self, to: usize, name: &str) {
        if self.dragging || self.swipe.as_ref().is_some_and(|s| s.held.is_some()) {
            return;
        }
        let shown: Vec<(Window, Rectangle<i32, Logical>)> = self
            .space
            .elements()
            .filter(|window| self.desks.layout_of(window).screen_of(window) == Some(name))
            .filter_map(|window| Some((window.clone(), self.frame_of(window)?)))
            .collect();
        let from = self.desks.shown_on(name);
        let Some(back) = self.desks.switch_on(to, name, shown.clone()) else {
            return;
        };
        let width = self
            .window_areas()
            .into_iter()
            .find(|(n, _)| n == name)
            .map_or(0, |(_, area)| area.size.w);
        let by = Point::from((slide_by(from, to, width), 0));
        let key = self.switch_key(name);
        let slid = self.animations.begin_switch(&key, by.to_f64());
        for (window, _) in &shown {
            if slid {
                self.snapshot_leaving(window, by.to_f64(), key.clone());
            }
            self.hide(window);
        }
        for (window, frame) in back {
            self.show(window, frame);
        }
        eprintln!("edel-compositor: workspace {} on {name}", to + 1);
        self.focus_top();
        self.relayout();
        self.repoint();
        self.settle_dynamic();
        self.announce_workspaces();
    }

    /// The switch key of screen `name` (M5.14): the screen's own with
    /// workspaces on each screen, else `None`, one switch for every screen.
    fn switch_key(&self, screen: &str) -> Option<String> {
        self.desks.per_screen().then(|| screen.to_string())
    }

    /// The switch key of the screen `window` is on, for the look of its
    /// window (M5.14): with workspaces on each screen, its screen's, else
    /// `None`.
    pub fn window_switch_key(&self, window: &Window) -> Option<String> {
        if !self.desks.per_screen() {
            return None;
        }
        self.desks
            .layout_of(window)
            .screen_of(window)
            .map(str::to_string)
    }

    /// Shows workspace `desk` where `window` is: on every screen, or, with
    /// workspaces on each screen, on the window's.
    pub fn switch_for(&mut self, desk: usize, window: &Window) {
        let screen = self
            .desks
            .layout_of(window)
            .screen_of(window)
            .or(self.desks.minimized_screen(window))
            .map(str::to_string);
        match screen {
            Some(name) if self.desks.per_screen() => self.switch_screen(desk, &name),
            _ => self.switch_workspace(desk),
        }
    }

    /// With workspaces on each screen, a window on a screen showing
    /// another workspace than its own joins that one, and a hidden window
    /// whose screen shows its workspace comes back: after the screens
    /// changed, a window was put on another screen or the key was turned
    /// on (`desks::Desks::settle_screens`).
    pub fn settle_screens(&mut self) {
        if !self.desks.per_screen() {
            return;
        }
        let areas = self.window_areas();
        let shown: Vec<(Window, Rectangle<i32, Logical>)> = self
            .space
            .elements()
            .filter_map(|window| Some((window.clone(), self.frame_of(window)?)))
            .collect();
        for (window, frame) in self.desks.settle_screens(&shown, &areas) {
            self.show(window, frame);
        }
    }

    /// `workspaces.per_screen` changed to `on`. Turned off, every
    /// screen shows the workspace in use first.
    pub fn set_per_screen(&mut self, on: bool) {
        if on == self.desks.per_screen() {
            return;
        }
        if !on {
            let active = self.desks.active();
            for (name, _) in self.window_areas() {
                self.switch_screen(active, &name);
            }
        }
        for (window, frame) in self.desks.set_per_screen(on) {
            self.show(window, frame);
        }
        self.settle_screens();
        eprintln!(
            "edel-compositor: workspaces per screen {}",
            if on { "on" } else { "off" }
        );
        self.relayout();
        self.announce_workspaces();
    }

    /// The pointer moved: with workspaces on each screen, the screen it is
    /// on is in use, so the panels' switcher and the state file show its
    /// workspace. Not during a drag, whose window keeps its workspace
    /// until it is put down.
    pub fn note_screen(&mut self) {
        if !self.desks.per_screen() || self.dragging {
            return;
        }
        let Some((name, _)) = self.pointer_screen() else {
            return;
        };
        if self.desks.use_screen(&name) {
            self.announce_workspaces();
            self.state_changed();
        }
    }

    /// A drag of a switcher button (M5.2p, `edel-shell-v1`'s
    /// `move_workspace`) or of a frame in the overview's strip (M5.2j-b2):
    /// workspace `from` moves to place `to` with its windows, name and
    /// policy; the clients and the state file hear it, and the person's
    /// settings file keeps the names in their new order.
    pub fn move_workspace(&mut self, from: usize, to: usize) {
        let count = self.desks.count();
        if !self.desks.reorder(from, to) {
            return;
        }
        eprintln!(
            "edel-compositor: workspace {} moved to {}",
            from + 1,
            to + 1
        );
        keep_order(WORKSPACE_NAMES, from, to, count);
        keep_order(WORKSPACE_WALLPAPERS, from, to, count);
        self.announce_workspaces();
        self.state_changed();
    }

    /// Super+Ctrl+Right and Left: shows the workspace after the shown one,
    /// or before it, where there is one (M5.2i).
    pub fn switch_neighbour(&mut self, forward: bool) {
        if let Some(to) = self.desks.neighbour(forward) {
            self.switch_workspace(to);
        }
    }

    /// Super+Ctrl+Shift+Right and Left: moves the focused window to the
    /// workspace after the shown one, or before it, and shows that
    /// workspace with the window (M5.2i). Nothing happens without one.
    pub fn move_to_neighbour(&mut self, forward: bool) {
        let (Some(to), Some(window)) = (self.desks.neighbour(forward), self.focused_window())
        else {
            return;
        };
        if self.move_to_workspace(to) {
            // Dynamic workspaces may have closed an empty one before it
            // while the window moved, so the window's workspace is looked
            // up where it is now, not where it was sent.
            if let Some(desk) = self.desks.hidden_on(&window) {
                self.switch_for(desk, &window);
            }
        }
    }

    /// Moves the focused window to workspace `to`, on top of its windows;
    /// this workspace stays shown. False when nothing moved.
    pub fn move_to_workspace(&mut self, to: usize) -> bool {
        if self.dragging {
            return false;
        }
        let Some(window) = self.focused_window() else {
            return false;
        };
        let Some(frame) = self.frame_of(&window) else {
            return false;
        };
        // A maximized window keeps the size it goes back to.
        let size = data(&window)
            .borrow()
            .restore
            .map_or(frame.size, |r| r.size);
        let areas = self.window_areas();
        if self.desks.send(window.clone(), to, size, &areas).is_none() {
            return false;
        }
        eprintln!(
            "edel-compositor: window {} to workspace {}",
            title(&window),
            to + 1
        );
        self.hide(&window);
        self.focus_top();
        if self.desks.layout().rearranges() {
            self.relayout();
        }
        self.settle_dynamic();
        self.dirty = true;
        self.state_changed();
        self.repoint();
        true
    }

    /// Moves `window`, shown or on a hidden workspace, to workspace `to`
    /// on screen `onto`, else its own, as a drag in the overview does
    /// (M5.2j, M5.2j-b4): it shows there on top if that screen shows `to`,
    /// else it waits on top of `to`'s windows. Logs `window T to workspace
    /// N`, with `on SCREEN` when it changed screens. False when nothing
    /// moved.
    pub fn carry_window(&mut self, window: &Window, to: usize, onto: Option<&str>) -> bool {
        let shown = self.space.elements().any(|w| w == window);
        let frame = if shown {
            self.frame_of(window)
        } else {
            self.desks
                .hidden()
                .find(|(_, w, _)| *w == window)
                .map(|(_, _, frame)| frame)
        };
        let Some(frame) = frame else {
            return false;
        };
        // A maximized window keeps the size it goes back to.
        let size = data(window).borrow().restore.map_or(frame.size, |r| r.size);
        let areas = self.window_areas();
        let before = self
            .desks
            .layout_of(window)
            .screen_of(window)
            .map(str::to_string);
        let Some((place, shows)) = self.desks.carry(window.clone(), to, onto, size, &areas) else {
            return false;
        };
        match onto.filter(|o| before.as_deref() != Some(*o)) {
            Some(screen) => eprintln!(
                "edel-compositor: window {} to workspace {} on {screen}",
                title(window),
                to + 1
            ),
            None => eprintln!(
                "edel-compositor: window {} to workspace {}",
                title(window),
                to + 1
            ),
        }
        if shown {
            self.hide(window);
        }
        if shows {
            self.show(window.clone(), place);
        }
        self.focus_top();
        self.relayout();
        self.settle_dynamic();
        self.dirty = true;
        self.state_changed();
        self.repoint();
        true
    }

    /// Dynamic workspaces (M5.2i): applies `desks::dynamic_plan` to the
    /// workspaces as they are, closing the empty ones it names and adding
    /// the empty one that waits at the end, then tells the clients and the
    /// state file. The count is logged when it changes. Nothing happens
    /// unless `workspaces.dynamic` is on.
    pub fn settle_dynamic(&mut self) {
        // Fingers still hold the workspaces: their swipe settles it (M5.14).
        if !self.settings.dynamic() || self.animations.swiping {
            return;
        }
        let before = self.desks.count();
        let plan = dynamic_plan(&self.desks.window_counts(), &self.desks.shown(), |i| {
            self.desks.named(i)
        });
        if plan.remove.is_empty() && !plan.add {
            return;
        }
        // Highest first, so each index in the plan is still the right one.
        for index in plan.remove.iter().rev() {
            self.desks.remove(*index);
        }
        if plan.add {
            self.desks.add(self.settings.policy());
        }
        if self.desks.count() != before {
            eprintln!(
                "edel-compositor: workspaces now {} (dynamic)",
                self.desks.count()
            );
        }
        self.announce_workspaces();
        self.state_changed();
    }

    /// The preset asks for `count` workspaces: if a shown one goes, the
    /// last that stays is shown first, and the windows of those that go
    /// join it.
    pub fn set_workspace_count(&mut self, count: usize) {
        let count = count.clamp(1, MOST);
        if self.desks.per_screen() {
            for (name, _) in self.window_areas() {
                if self.desks.shown_on(&name) >= count {
                    self.switch_screen(count - 1, &name);
                }
            }
        } else if self.desks.active() >= count {
            self.switch_workspace(count - 1);
        }
        let areas = self.window_areas();
        let policy = self.settings.policy();
        let Some(joined) = self.desks.set_count(count, &areas, policy) else {
            return;
        };
        eprintln!("edel-compositor: {count} workspaces");
        for (window, frame) in joined {
            self.show(window, frame);
        }
        self.settle_screens();
        self.relayout();
        self.announce_workspaces();
    }

    /// Minimizes `window`, shown, as its title bar's button, the window
    /// itself or the window list asks: its picture fades as a closed
    /// window's does, the others may fill its place, and the keyboard goes
    /// to the window now on top. Nothing happens during a drag.
    pub fn minimize(&mut self, window: &Window) {
        if self.dragging {
            return;
        }
        let Some(frame) = self.frame_of(window) else {
            return;
        };
        self.desks.minimize(window.clone(), frame);
        eprintln!("edel-compositor: minimized window {}", title(window));
        self.snapshot_closing(window);
        self.hide(window);
        self.focus_top();
        if self.desks.layout().rearranges() {
            self.relayout();
        }
        self.dirty = true;
        self.state_changed();
        self.repoint();
    }

    /// Brings `window` back if it is minimized, showing its workspace
    /// first: on top, with the keyboard, where it was (a tile in tiling,
    /// the screen if it was maximized). False if it was not minimized, or
    /// during a drag.
    pub fn restore(&mut self, window: &Window) -> bool {
        let Some((desk, _)) = self.desks.minimized(window) else {
            return false;
        };
        if self.dragging {
            return false;
        }
        self.switch_for(desk, window);
        let areas = self.window_areas();
        let Some(frame) = self.desks.restore(window, &areas) else {
            return false;
        };
        eprintln!("edel-compositor: restored window {}", title(window));
        self.show(window.clone(), frame);
        self.animations.opened(window);
        self.focus(window);
        self.relayout();
        self.dirty = true;
        self.state_changed();
        self.repoint();
        true
    }

    /// `window`, on a hidden workspace or minimized, closed or dropped
    /// its buffer: it leaves its workspace and the state file, and what it
    /// was is forgotten, as `unmap` does for a shown window.
    pub fn forget_hidden(&mut self, window: &Window) {
        eprintln!("edel-compositor: unmapped window {}", title(window));
        let mut frame = data(window).borrow_mut();
        frame.restore = None;
        frame.shape = None;
        drop(frame);
        self.desks.close(window);
        self.forget(window);
        self.settle_dynamic();
        self.state_changed();
    }

    /// `window` leaves the screen for a hidden workspace, and is no longer
    /// drawn as focused.
    fn hide(&mut self, window: &Window) {
        self.animations.forget(window);
        self.forget(window);
        self.space.unmap_elem(window);
        window.set_activated(false);
        if let Some(toplevel) = window.toplevel() {
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
    }

    /// `window` comes back on top, its frame at `frame`, where the layout
    /// has it; the relayout after it fits the frame to the screen as it is
    /// now. A window shown again is drawn itself, not the picture it left
    /// behind (M5.14).
    fn show(&mut self, window: Window, frame: Rectangle<i32, Logical>) {
        let place = self.insets(&window).window(frame);
        self.animations.came_back(&window);
        self.space.map_element(window, place.loc, false);
    }

    /// The keyboard goes to the top window, or to nothing on an empty
    /// workspace, so no key reaches a hidden window. With workspaces on
    /// each screen, the top one on the pointer's screen goes first.
    pub fn focus_top(&mut self) {
        // Locked, the keyboard stays with the lock screen (M5.10).
        if self.is_locked() {
            return;
        }
        let here = self
            .desks
            .per_screen()
            .then(|| self.pointer_screen().map(|(name, _)| name))
            .flatten();
        let top = self
            .space
            .elements()
            .rev()
            .find(|w| here.is_none() || self.desks.layout_of(w).screen_of(w) == here.as_deref())
            .or_else(|| self.space.elements().last())
            .cloned();
        if let Some(top) = top {
            self.focus(&top);
        } else if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
    }

    /// A touchpad swipe moved by `dx` across and `dy` down (touchpad
    /// pixels). Three fingers that slid far enough across take the
    /// workspaces, and from then on the switch they hold moves one to one
    /// with them (M5.14); other swipes only count for the four-finger
    /// overview, decided when they end (`swipe_ended`).
    pub fn swipe_moved(&mut self, dx: f64, dy: f64) {
        let Some(swipe) = self.swipe.as_mut() else {
            return;
        };
        swipe.dx += dx;
        swipe.dy += dy;
        let (fingers, across, down) = (swipe.fingers, swipe.dx, swipe.dy);
        match swipe.held.take() {
            Some(held) => {
                let held = self.hold_swipe(held, across);
                if let Some(swipe) = self.swipe.as_mut() {
                    swipe.held = Some(held);
                }
            }
            None if fingers == 3
                && across.abs() >= SWIPE_START
                && across.abs() > down.abs()
                && !self.dragging
                && self.overview.is_none()
                && !self.is_locked() =>
            {
                if let Some(held) = self.take_workspaces(across < 0.0, across) {
                    if let Some(swipe) = self.swipe.as_mut() {
                        swipe.held = Some(held);
                    }
                }
            }
            None => {}
        }
    }

    /// Fingers that hold the workspaces moved `across` from where they
    /// began: the switch follows them one to one, and its speed is what
    /// they moved it at since they last did (`held.velocity`, smoothed).
    fn hold_swipe(&mut self, mut held: Held, across: f64) -> Held {
        let now = self.animations.now();
        let offset = swipe_offset(held.by_x, across, held.width);
        let dt = now.saturating_sub(held.at).as_secs_f64();
        if dt > 0.0 {
            let new = (offset - held.offset) / dt;
            held.velocity = 0.5 * held.velocity + 0.5 * new;
        }
        held.offset = offset;
        held.at = now;
        if let Some(switch) = self.animations.switches.get_mut(&held.key) {
            switch.hold((offset, 0.0).into(), (held.velocity, 0.0).into());
        }
        self.dirty = true;
        held
    }

    /// Three fingers took the workspaces (M5.14): the next one (`forward`)
    /// or the one before comes in on the screen the pointer is on, held by
    /// the fingers from where they are, with `across` slid so far. None when
    /// nothing is there to switch to, or no screen.
    fn take_workspaces(&mut self, forward: bool, across: f64) -> Option<Held> {
        let (name, area) = if self.desks.per_screen() {
            let (name, area) = self.pointer_screen()?;
            (Some(name), area)
        } else {
            (None, self.window_area()?)
        };
        let width = area.size.w;
        if width <= 0 {
            return None;
        }
        let moved_screen = name
            .as_ref()
            .is_some_and(|name| self.desks.use_screen(name));
        if moved_screen {
            self.announce_workspaces();
            self.state_changed();
        }
        let to = self.desks.neighbour(forward)?;
        let from = self.desks.active();
        let by_x = f64::from(slide_by(from, to, width));
        let key = name.as_ref().and_then(|name| self.switch_key(name));
        self.animations.swiping = true;
        self.switch_workspace(to);
        let shown = match &name {
            Some(name) => self.desks.shown_on(name),
            None => self.desks.active(),
        };
        if shown != to || !self.animations.switches.contains_key(&key) {
            self.animations.swiping = false;
            return None;
        }
        let offset = swipe_offset(by_x, across, f64::from(width));
        if let Some(switch) = self.animations.switches.get_mut(&key) {
            switch.hold((offset, 0.0).into(), (0.0, 0.0).into());
        }
        eprintln!("edel-compositor: swipe to workspace {}", to + 1);
        self.dirty = true;
        Some(Held {
            key,
            by_x,
            width: f64::from(width),
            offset,
            velocity: 0.0,
            at: self.animations.now(),
            from,
        })
    }

    /// Three fingers lifted, or the touchpad cancelled the swipe (M5.14). A
    /// swipe that held the workspaces goes on to the incoming one when it is
    /// past half way or flicked, else it springs back to where it was; a
    /// swipe that did not hold them is the four-finger overview's, as before
    /// (M5.2j-b4).
    pub fn swipe_ended(&mut self, cancelled: bool) {
        let Some(swipe) = self.swipe.take() else {
            return;
        };
        let Some(held) = swipe.held else {
            let open = self.overview.is_some();
            if !cancelled
                && !self.is_locked()
                && edel_compositor::overview::swiped(swipe.fingers, swipe.dy, open)
            {
                eprintln!("edel-compositor: overview swipe");
                self.toggle_overview();
            }
            return;
        };
        let now = self.animations.now();
        self.animations.swiping = false;
        // Fingers that stopped before they lifted send no update, so an
        // old speed is no speed.
        let velocity = if now.saturating_sub(held.at) > Duration::from_millis(50) {
            0.0
        } else {
            held.velocity
        };
        let goes = !cancelled && swipe_goes(held.offset, velocity, held.by_x);
        let style = self.animations.style;
        if let Some(switch) = self.animations.switches.get_mut(&held.key) {
            switch.hold((held.offset, 0.0).into(), (velocity, 0.0).into());
            if goes {
                switch.release(now, style.slide, style.feel.workspaces);
            }
        }
        if goes {
            let shown = match held.key.as_deref() {
                Some(name) => self.desks.shown_on(name),
                None => self.desks.active(),
            };
            eprintln!("edel-compositor: swipe went to workspace {}", shown + 1);
        } else {
            // Back to where the fingers found it: the switch carries on from
            // where it is held, so it springs back (on Lite it jumps).
            self.switch_workspace(held.from);
            eprintln!("edel-compositor: swipe went back");
        }
        self.dirty = true;
        self.settle_dynamic();
    }
}

/// Writes the person's `key`, a list by workspace (`workspaces.names`,
/// M5.2p, or `workspaces.wallpapers`, M5.12b5), in its new order after the
/// workspace at place `from` moved to `to` among `count`, as Settings
/// writes a key (ADR-008). Nothing is written when no file holds the
/// list; a failure says what stays as it was.
fn keep_order(key: &str, from: usize, to: usize, count: usize) {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(places::found(&path)).ok();
    let machine = read(places::machine_settings());
    let person = places::person_settings().and_then(read);
    let names = settings::texts(key, machine.as_deref(), person.as_deref()).unwrap_or_default();
    if names.is_empty() {
        return;
    }
    let without = settings::texts(key, machine.as_deref(), None).unwrap_or_default();
    let list = reordered_names(&names, count, from, to);
    let value = names_to_write(&list, &without);
    match places::person_settings().map(|p| places::found(&p)) {
        Some(path) => {
            if let Err(e) = settings::write(&path, key, value.as_deref()) {
                eprintln!(
                    "edel-compositor: {}",
                    messages::order_not_kept(key, format!("{e:#}"))
                );
            }
        }
        None => eprintln!("edel-compositor: {}", messages::order_no_home(key)),
    }
}
