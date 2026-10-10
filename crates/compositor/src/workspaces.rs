//! Workspaces on screen (roadmap M5.2a): Super+1 to Super+9 show one,
//! Super+Shift+1 to 9 move the focused window to one, and the preset says
//! how many (`edel_compositor::desks` keeps them). A hidden workspace's
//! windows leave the `Space`, so they are not drawn and get no frame
//! callbacks; each workspace keeps its stacking order and its own policy.
//! Nothing happens during a drag, whose grab holds a shown window. A
//! switch slides at the tier's slide length (M5.2f): the shown windows'
//! pictures leave towards one side while the other workspace's windows
//! come in from the other, as the workspaces lie in a row; on Lite, whose
//! slides take no time, the switch is instant. Minimizing (M5.2h) hides a
//! window the same way, on its own workspace, until the window list
//! brings it back. With `layout.workspaces_per_screen` (M5.2k) each
//! screen shows its own workspace: Super+1 to Super+9 switch the screen
//! the pointer is on, and a window put on another screen joins the
//! workspace shown there.

use smithay::desktop::Window;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use edel_compositor::desks::{MOST, dynamic_plan, slide_by};

use crate::decoration::{data, title};
use crate::state::Edel;

impl Edel {
    /// Shows workspace `to`, from 0, if it is not shown: on every screen,
    /// or, with workspaces on each screen, on the pointer's.
    pub fn switch_workspace(&mut self, to: usize) {
        if self.dragging {
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
        for (window, _) in &shown {
            self.snapshot_leaving(window, by);
            self.hide(window);
        }
        // Each comes in from a screen's width away; the relayout below
        // slides it to its place, or puts it there at once on Lite.
        for (window, frame) in back {
            self.show(window, frame, Point::default() - by);
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
        if self.dragging {
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
        for (window, _) in &shown {
            self.snapshot_leaving(window, by);
            self.hide(window);
        }
        for (window, frame) in back {
            self.show(window, frame, Point::default() - by);
        }
        eprintln!("edel-compositor: workspace {} on {name}", to + 1);
        self.focus_top();
        self.relayout();
        self.repoint();
        self.settle_dynamic();
        self.announce_workspaces();
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
            self.show(window, frame, Point::default());
        }
    }

    /// `layout.workspaces_per_screen` changed to `on`. Turned off, every
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
            self.show(window, frame, Point::default());
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

    /// Dynamic workspaces (M5.2i): applies `desks::dynamic_plan` to the
    /// workspaces as they are, closing the empty ones it names and adding
    /// the empty one that waits at the end, then tells the clients and the
    /// state file. The count is logged when it changes. Nothing happens
    /// unless `layout.dynamic_workspaces` is on.
    pub fn settle_dynamic(&mut self) {
        if !self.settings.dynamic() {
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
            self.show(window, frame, Point::default());
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
        self.show(window.clone(), frame, Point::default());
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

    /// `window` comes back on top, its frame at `frame`, drawn `from`
    /// away from it; the relayout after it fits the frame to the screen as
    /// it is now, sliding the window there.
    fn show(&mut self, window: Window, frame: Rectangle<i32, Logical>, from: Point<i32, Logical>) {
        let place = self.insets(&window).window(frame);
        self.space.map_element(window, place.loc + from, false);
    }

    /// The keyboard goes to the top window, or to nothing on an empty
    /// workspace, so no key reaches a hidden window. With workspaces on
    /// each screen, the top one on the pointer's screen goes first.
    fn focus_top(&mut self) {
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
}
