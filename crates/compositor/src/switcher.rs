//! The window switcher (M5.3c): Alt+Tab, or the `switcher` keys, steps
//! through the shown workspace's windows, the most recently used first
//! (the space's stacking order, top first, as focus raises), then its
//! minimized ones; with Shift it steps back. It stays open while the
//! keys' modifiers are held and switches to the chosen window when they
//! are let go; Escape leaves things as they were. The compositor owns
//! the order and the choice, so switching works without shell-ui;
//! shell-ui only draws the list it is sent over `edel-shell-v1`.

use smithay::desktop::Window;

use crate::decoration::title;
use crate::state::Edel;

/// Titles sent to shell-ui at once, around the chosen window.
const SHOWN: usize = 10;
/// A title's most characters in the list.
const LONGEST: usize = 60;

/// The open switcher: the windows it offers, in order, and the chosen one.
pub struct Switcher {
    windows: Vec<Window>,
    chosen: usize,
}

impl Edel {
    /// Alt+Tab: opens the switcher on the window after the focused one,
    /// or steps it on; `back` steps the other way.
    pub fn switcher_step(&mut self, back: bool) {
        if self.switcher.is_none() {
            let mut windows: Vec<Window> = self.space.elements().rev().cloned().collect();
            windows.extend(self.desks.minimized_here().cloned());
            if windows.is_empty() {
                return;
            }
            self.switcher = Some(Switcher { windows, chosen: 0 });
        }
        let Some(switcher) = self.switcher.as_mut() else {
            return;
        };
        let n = switcher.windows.len();
        switcher.chosen = if back {
            (switcher.chosen + n - 1) % n
        } else {
            (switcher.chosen + 1) % n
        };
        let start = switcher.chosen.saturating_sub(SHOWN - 1);
        let titles: Vec<String> = switcher.windows[start..n.min(start + SHOWN)]
            .iter()
            .map(|w| {
                title(w)
                    .chars()
                    .take(LONGEST)
                    .collect::<String>()
                    .replace('\n', " ")
            })
            .collect();
        let chosen = switcher.chosen - start;
        eprintln!(
            "edel-compositor: switcher at {}",
            title(&switcher.windows[switcher.chosen])
        );
        self.show_switcher(&titles.join("\n"), chosen as u32);
    }

    /// The modifiers were let go (`take`), or Escape pressed: the
    /// switcher closes, switching to the chosen window if it is still
    /// there.
    pub fn switcher_done(&mut self, take: bool) {
        let Some(switcher) = self.switcher.take() else {
            return;
        };
        self.hide_switcher();
        if !take {
            return;
        }
        let window = switcher.windows[switcher.chosen].clone();
        if self.desks.minimized(&window).is_some() {
            if !self.restore(&window) {
                return;
            }
        } else if self.space.elements().any(|w| *w == window) {
            self.focus(&window);
        } else {
            return;
        }
        eprintln!("edel-compositor: switched to window {}", title(&window));
        self.state_changed();
    }

    /// Forgets `window`, once gone, so the switcher never offers it.
    pub fn switcher_forget(&mut self, window: &Window) {
        let Some(switcher) = self.switcher.as_mut() else {
            return;
        };
        if let Some(i) = switcher.windows.iter().position(|w| w == window) {
            switcher.windows.remove(i);
            if switcher.windows.is_empty() {
                self.switcher_done(false);
            } else if switcher.chosen >= i && switcher.chosen > 0 {
                switcher.chosen -= 1;
            }
        }
    }
}
