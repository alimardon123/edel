//! Fullscreen (M5.20): xdg-shell's fullscreen state. A fullscreen window
//! covers its whole screen, panels and docks too, with no title bar or
//! border, until it leaves fullscreen and goes back to its frame from
//! before, maximized again if it was. It is drawn over the top layer only
//! while it is the top window on its screen, so a window raised over it
//! (the switcher, a click on the window list) shows with the panel as
//! usual; overlays such as the launcher stay over it. Super+F
//! (`toggle_fullscreen`) does the same for any window.

use smithay::desktop::Window;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State;
use smithay::utils::{Logical, Point};
use smithay::wayland::shell::wlr_layer::Layer;

use crate::decoration::data;
use crate::state::Edel;

/// Only overlays stay over a fullscreen window.
const OVER_FULLSCREEN: [Layer; 1] = [Layer::Overlay];

impl Edel {
    pub fn is_fullscreen(&self, window: &Window) -> bool {
        data(window).borrow().before_fullscreen.is_some()
    }

    pub fn toggle_fullscreen(&mut self, window: &Window) {
        if self.is_fullscreen(window) {
            self.unfullscreen(window);
        } else {
            self.fullscreen(window, None);
        }
    }

    /// `window` covers `output`, or its own screen.
    pub fn fullscreen(&mut self, window: &Window, output: Option<Output>) {
        let output = output.or_else(|| {
            let (name, _) = self.home(window)?;
            self.space.outputs().find(|o| o.name() == name).cloned()
        });
        let Some(screen) = output.and_then(|o| self.space.output_geometry(&o)) else {
            return;
        };
        if !self.is_fullscreen(window) {
            // Taken before the flag is set, while the frame still counts.
            let Some(frame) = self.frame_of(window) else {
                return;
            };
            data(window).borrow_mut().before_fullscreen = Some(frame);
        }
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.states.set(State::Fullscreen);
            state.size = Some(screen.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        if self.desks.hidden_on(window).is_some() {
            return;
        }
        eprintln!(
            "edel-compositor: window {} fullscreen",
            crate::decoration::title(window)
        );
        self.slide(window, screen.loc);
        self.space.map_element(window.clone(), screen.loc, true);
        self.focus(window);
        self.dirty = true;
        self.state_changed();
    }

    /// `window` goes back to its frame from before, maximized again if it
    /// was.
    pub fn unfullscreen(&mut self, window: &Window) {
        let Some(frame) = data(window).borrow_mut().before_fullscreen.take() else {
            return;
        };
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| state.states.unset(State::Fullscreen));
        eprintln!(
            "edel-compositor: window {} not fullscreen",
            crate::decoration::title(window)
        );
        if self.is_maximized(window) {
            self.maximize(window);
            return;
        }
        let place = self.insets(window).window(frame);
        toplevel.with_pending_state(|state| state.size = Some(place.size));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        if self.desks.hidden_on(window).is_none() {
            self.placed(window, place);
        }
    }

    /// The layers drawn and reached over the windows on `output`: none but
    /// the overlays while a fullscreen window is its top window.
    pub fn layers_over(&self, output: &Output) -> &'static [Layer] {
        let Some(screen) = self.space.output_geometry(output) else {
            return &crate::layers::ABOVE;
        };
        let top = self.space.elements().rev().find(|window| {
            self.space
                .element_geometry(window)
                .is_some_and(|place| place.overlaps(screen))
        });
        match top {
            Some(window) if self.is_fullscreen(window) => &OVER_FULLSCREEN,
            _ => &crate::layers::ABOVE,
        }
    }

    /// The layers over the windows at `point`, as [`Edel::layers_over`].
    pub fn layers_over_point(&self, point: Point<f64, Logical>) -> &'static [Layer] {
        match self.space.output_under(point).next() {
            Some(output) => self.layers_over(output),
            None => &crate::layers::ABOVE,
        }
    }
}
