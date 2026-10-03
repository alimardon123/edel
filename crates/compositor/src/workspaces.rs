//! Workspaces on screen (roadmap M5.2a): Super+1 to Super+9 show one,
//! Super+Shift+1 to 9 move the focused window to one, and the preset says
//! how many (`edel_compositor::desks` keeps them). A hidden workspace's
//! windows leave the `Space`, so they are not drawn and get no frame
//! callbacks; each workspace keeps its stacking order and its own policy.
//! Nothing happens during a drag, whose grab holds a shown window.

use smithay::desktop::Window;
use smithay::utils::{Logical, Rectangle, SERIAL_COUNTER};

use edel_compositor::desks::MOST;

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
        let Some(back) = self.desks.switch(to, shown.clone()) else {
            return;
        };
        for (window, _) in &shown {
            self.hide(window);
        }
        for (window, frame) in back {
            self.show(window, frame);
        }
        eprintln!("edel-compositor: workspace {}", to + 1);
        self.focus_top();
        self.relayout();
        self.repoint();
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
            self.show(window, frame);
        }
        self.relayout();
    }

    /// `window`, on a hidden workspace, closed or dropped its buffer: it
    /// leaves its workspace and the state file, and what it was is
    /// forgotten, as `unmap` does for a shown window.
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

    /// `window` comes back on top, its frame at `frame`; the relayout
    /// after it fits the frame to the screen as it is now.
    fn show(&mut self, window: Window, frame: Rectangle<i32, Logical>) {
        let place = self.insets(&window).window(frame);
        self.space.map_element(window, place.loc, false);
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
