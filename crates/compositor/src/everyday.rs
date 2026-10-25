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
//!   honours: ext-idle-notify (M7.8) tells clients when the person is idle,
//!   and holds off while a surface asks, even one that died without saying
//!   so (`sync_idle_inhibit`);
//! - xdg-activation, so a link clicked in one app raises the browser that
//!   opens it: the window comes forward, to its workspace if it is on
//!   another, as a click in the window list does;
//! - keyboard shortcuts inhibit (M5.13b), so the Shortcuts page's capture
//!   gets Super+W and its like: an inhibitor the client asks for is
//!   activated at once, and `input.rs` passes the keys over while the
//!   focused surface's inhibitor is active (the terminals, Super+L and the lock
//!   screen's keys aside).

use smithay::delegate_idle_inhibit;
use smithay::delegate_idle_notify;
use smithay::delegate_keyboard_shortcuts_inhibit;
use smithay::delegate_pointer_constraints;
use smithay::delegate_primary_selection;
use smithay::delegate_relative_pointer;
use smithay::delegate_xdg_activation;
use smithay::input::pointer::PointerHandle;
use smithay::reexports::wayland_server::Resource as _;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point};
use smithay::wayland::idle_inhibit::IdleInhibitHandler;
use smithay::wayland::idle_notify::{IdleNotifierHandler, IdleNotifierState};
use smithay::wayland::keyboard_shortcuts_inhibit::{
    KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
};
use smithay::wayland::pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState,
};
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};

use crate::messages;
use crate::state::{ClientState, Edel};

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
            self.sync_idle_inhibit();
            self.state_changed();
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| *s != surface);
        self.sync_idle_inhibit();
        self.state_changed();
    }
}

impl Edel {
    /// The surfaces that keep the screen on now: those still alive.
    pub fn idle_inhibited(&self) -> usize {
        self.idle_inhibitors.iter().filter(|s| s.is_alive()).count()
    }

    /// Holds the idle notifications while a surface keeps the screen on.
    /// Run after every dispatch: a surface that dies without asking to
    /// stop leaves the count with no call to `uninhibit`.
    pub fn sync_idle_inhibit(&mut self) {
        let inhibited = self.idle_inhibited() > 0;
        self.idle_notifier.set_is_inhibited(inhibited);
    }

    /// shell-ui asks for the screens to go off after the idle time
    /// (M7.16b, `edel-shell-v1`'s `screens_off`). The screens turn off at
    /// the next frame (`drm.rs`), which asks for it through `dirty`. A game
    /// playing on some screen keeps them all on: shell-ui never asks then,
    /// so this check is only a guard.
    pub fn screens_off_asked(&mut self) {
        if self.screens_off {
            return;
        }
        if self
            .space
            .outputs()
            .any(|output| self.playing_on(output).is_some())
        {
            eprintln!("edel-compositor: screens stay on: a game plays");
            return;
        }
        self.screens_off = true;
        self.dirty = true;
        self.state_changed();
    }

    /// Input came: the screens that were off turn on again (M7.16b). A
    /// full frame is asked, so each screen's first frame lights it.
    pub fn screens_on(&mut self) {
        if !std::mem::take(&mut self.screens_off) {
            return;
        }
        eprintln!("edel-compositor: screens on");
        self.repaint = true;
        self.dirty = true;
        self.state_changed();
    }
}

impl IdleNotifierHandler for Edel {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier
    }
}

impl KeyboardShortcutsInhibitHandler for Edel {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit
    }

    /// A client asks to have its keys without ours. The Shortcuts page's
    /// dialog does, while it takes a new chord (M5.13b); the compositor
    /// says so in the log, which CI waits for.
    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        let surface = inhibitor.wl_surface().clone();
        let title = self
            .window_of(&surface)
            .map(|window| crate::decoration::title(&window))
            .unwrap_or_default();
        inhibitor.activate();
        eprintln!("edel-compositor: shortcuts inhibited for {title}");
    }

    fn inhibitor_destroyed(&mut self, _inhibitor: KeyboardShortcutsInhibitor) {
        eprintln!("edel-compositor: shortcuts back");
    }
}

impl Edel {
    /// Whether the token `data` describes may bring a window forward, by
    /// `sandbox::may_activate`: who asked for it, how old it is, and whether
    /// its asker holds the keyboard with an input since it got it.
    fn token_allowed(&self, data: &XdgActivationTokenData) -> bool {
        let asker = data.client_id.clone();
        // A client inserted without our state (XWayland's own) is the
        // compositor's, as `sandbox::shell_may` holds; one that is gone is
        // judged as a sandboxed one.
        let sandboxed = asker
            .clone()
            .and_then(|id| self.display.backend_handle().get_client_data(id).ok())
            .is_none_or(|state| {
                state
                    .downcast_ref::<ClientState>()
                    .is_some_and(|state| state.security_context.is_some())
            });
        let keyboard = self.seat.get_keyboard();
        let holds_keyboard = asker.is_some()
            && keyboard
                .as_ref()
                .and_then(|k| k.current_focus())
                .and_then(|surface| surface.client())
                .map(|focused| focused.id())
                == asker;
        let input_since = data
            .serial
            .as_ref()
            .zip(keyboard.and_then(|k| k.last_enter()))
            .is_some_and(|((serial, _), entered)| serial.is_no_older_than(&entered));
        crate::sandbox::may_activate(
            sandboxed,
            data.timestamp.elapsed(),
            holds_keyboard,
            input_since,
        )
    }
}

impl XdgActivationHandler for Edel {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation
    }

    /// A window asks to come forward with a token another app made, as
    /// a browser does when a link is clicked: to its workspace, then
    /// forward, a minimized one back.
    /// Only a token `may_activate` allows does this; any other is logged and
    /// dropped, and the window stays where it is.
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let Some(window) = self.window_of(&surface) else {
            return;
        };
        if !self.token_allowed(&data) {
            self.activation.remove_token(&token);
            eprintln!(
                "edel-compositor: {}",
                messages::activation_refused(&crate::decoration::title(&window))
            );
            return;
        }
        self.bring_forward(&window);
    }
}

delegate_pointer_constraints!(Edel);
delegate_relative_pointer!(Edel);
delegate_primary_selection!(Edel);
delegate_idle_inhibit!(Edel);
delegate_idle_notify!(Edel);
delegate_keyboard_shortcuts_inhibit!(Edel);
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
