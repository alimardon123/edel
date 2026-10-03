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
//! brings it back.

use smithay::desktop::Window;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use edel_compositor::desks::{MOST, slide_by};

use crate::decoration::{data, title};
use crate::state::Edel;

impl Edel {
    /// Shows workspace `to`, from 0, if it is not shown.
    pub fn switch_workspace(&mut self, to: usize) {
        if self.dragging {
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
        self.announce_workspaces();
    }

    /// Moves the focused window to workspace `to`, on top of its windows;
    /// this workspace stays shown.
    pub fn move_to_workspace(&mut self, to: usize) {
        if self.dragging {
            return;
        }
        let Some(window) = self.focused_window() else {
            return;
        };
        let (Some(frame), Some(area)) = (self.frame_of(&window), self.window_area()) else {
            return;
        };
        // A maximized window keeps the size it goes back to.
        let size = data(&window)
            .borrow()
            .restore
            .map_or(frame.size, |r| r.size);
        if self.desks.send(window.clone(), to, size, area).is_none() {
            return;
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
        self.dirty = true;
        self.state_changed();
        self.repoint();
    }

    /// The preset asks for `count` workspaces: if the shown one goes, the
    /// last that stays is shown first, and the windows of those that go
    /// join it.
    pub fn set_workspace_count(&mut self, count: usize) {
        let count = count.clamp(1, MOST);
        if self.desks.active() >= count {
            self.switch_workspace(count - 1);
        }
        let area = self.window_area().unwrap_or_default();
        let policy = self.settings.policy();
        let Some(joined) = self.desks.set_count(count, area, policy) else {
            return;
        };
        eprintln!("edel-compositor: {count} workspaces");
        for (window, frame) in joined {
            self.show(window, frame, Point::default());
        }
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
        self.switch_workspace(desk);
        let area = self.window_area().unwrap_or_default();
        let Some(frame) = self.desks.restore(window, area) else {
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
    /// workspace, so no key reaches a hidden window.
    fn focus_top(&mut self) {
        if let Some(top) = self.space.elements().last().cloned() {
            self.focus(&top);
        } else if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
    }
}
