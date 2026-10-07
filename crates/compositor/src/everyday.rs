//! The protocols everyday apps expect (roadmap M5.23), each on smithay's
//! own handler:
//!
//! - pointer constraints and relative pointer, for games, 3D and
//!   remote-desktop tools: a window may lock the pointer where it is, or
//!   keep it inside itself, while it gets the mouse's motion as it comes
//!   (`input.rs` holds the pointer still and sends the motion);
//! - primary selection, so selected text pastes with a middle click;
//! - idle inhibit, so a playing video keeps the screen on: the state file
//!   counts the surfaces that ask (`idle_inhibitors`), which M7.8's idle
//!   honours;
//! - xdg-activation, so a link clicked in one app raises the browser that
//!   opens it: the window comes forward, to its workspace if it is on
//!   another, as a click in the window list does.

use smithay::delegate_idle_inhibit;
use smithay::delegate_pointer_constraints;
use smithay::delegate_primary_selection;
use smithay::delegate_relative_pointer;
use smithay::delegate_xdg_activation;
use smithay::input::pointer::PointerHandle;
use smithay::reexports::wayland_server::Resource as _;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point};
use smithay::wayland::idle_inhibit::IdleInhibitHandler;
use smithay::wayland::pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState,
};
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};

use crate::state::Edel;

/// How a window holds the pointer, if it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// Locked: the pointer stays where it is; only motion is sent.
    Locked,
    /// Confined: the pointer stays over the window.
    Confined,
}

/// Where the pointer goes when the mouse moves it from `from` to `to`,
/// with `hold` the focused window's constraint and `inside` whether `to`
/// is still over that window.
pub fn constrained(
    from: Point<f64, Logical>,
    to: Point<f64, Logical>,
    hold: Option<Hold>,
    inside: bool,
) -> Point<f64, Logical> {
    match hold {
        Some(Hold::Locked) => from,
        Some(Hold::Confined) if !inside => from,
        _ => to,
    }
}

impl Edel {
    /// The constraint `surface` holds on the pointer once it is active;
    /// one that waits, as the pointer just came over its window, is
    /// made active now.
    pub fn hold_on(&self, surface: &WlSurface, pointer: &PointerHandle<Edel>) -> Option<Hold> {
        let mut hold = None;
        with_pointer_constraint(surface, pointer, |constraint| {
            if let Some(constraint) = constraint {
                if !constraint.is_active() {
                    constraint.activate();
                }
                hold = Some(match &*constraint {
                    smithay::wayland::pointer_constraints::PointerConstraint::Locked(_) => {
                        Hold::Locked
                    }
                    smithay::wayland::pointer_constraints::PointerConstraint::Confined(_) => {
                        Hold::Confined
                    }
                });
            }
        });
        hold
    }
}

impl PointerConstraintsHandler for Edel {
    /// A window asks to hold the pointer: at once if the pointer is over
    /// it, else when the pointer comes.
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if pointer.current_focus().as_ref() == Some(surface) {
            self.hold_on(surface, pointer);
        }
    }

    fn cursor_position_hint(
        &mut self,
        _: &WlSurface,
        _: &PointerHandle<Self>,
        _: Point<f64, Logical>,
    ) {
    }
}

impl PrimarySelectionHandler for Edel {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection
    }
}

impl IdleInhibitHandler for Edel {
    fn inhibit(&mut self, surface: WlSurface) {
        if !self.idle_inhibitors.contains(&surface) {
            self.idle_inhibitors.push(surface);
            self.state_changed();
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| *s != surface);
        self.state_changed();
    }
}

impl Edel {
    /// The surfaces that keep the screen on now: those still alive.
    pub fn idle_inhibited(&self) -> usize {
        self.idle_inhibitors.iter().filter(|s| s.is_alive()).count()
    }
}

impl XdgActivationHandler for Edel {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation
    }

    /// A window asks to come forward with a token another app made, as
    /// a browser does when a link is clicked: to its workspace, then
    /// forward, a minimized one back.
    fn request_activation(
        &mut self,
        _: XdgActivationToken,
        _: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let Some(window) = self.window_of(&surface) else {
            return;
        };
        self.bring_forward(&window);
    }
}

delegate_pointer_constraints!(Edel);
delegate_relative_pointer!(Edel);
delegate_primary_selection!(Edel);
delegate_idle_inhibit!(Edel);
delegate_xdg_activation!(Edel);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_pointer_stays_and_a_confined_one_stays_over_its_window() {
        let (from, to) = (Point::from((10.0, 10.0)), Point::from((30.0, 15.0)));
        assert_eq!(constrained(from, to, None, false), to);
        assert_eq!(constrained(from, to, Some(Hold::Locked), true), from);
        assert_eq!(constrained(from, to, Some(Hold::Confined), true), to);
        assert_eq!(constrained(from, to, Some(Hold::Confined), false), from);
    }
}
