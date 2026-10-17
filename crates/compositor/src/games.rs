//! Game mode (roadmap M7.7). `edel_compositor::game` holds the rule; this
//! module keeps it on `Edel`, answers Super+G (`toggle_game_mode`, logged
//! `game mode on` or `game mode off` and sent to shell-ui as the
//! `game_mode` event, which shows it), says which screen wants variable
//! refresh (`wants_vrr`: game mode is on and the screen's top window is
//! fullscreen; `drm.rs` asks each screen's DRM surface for it), and
//! offers tearing-control-v1. smithay 0.7 has no asynchronous page flips,
//! so a surface's `async` hint is kept and logged, never torn: such a game
//! still draws as fast as it likes (Mesa offers Vulkan's immediate present
//! mode once the global exists) and each refresh shows its newest frame,
//! and in game mode variable refresh shows it the moment it is done.
//! Seam: real tearing waits for smithay's async flips, and needs only
//! `drm.rs` to pass the hint on.

use std::cell::Cell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use smithay::desktop::Window;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::wp::tearing_control::v1::server::wp_tearing_control_manager_v1::{
    self, WpTearingControlManagerV1,
};
use smithay::reexports::wayland_protocols::wp::tearing_control::v1::server::wp_tearing_control_v1::{
    self, PresentationHint, WpTearingControlV1,
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
};
use smithay::wayland::compositor::with_states;

use edel_compositor::game::{GameMode, When};

use crate::state::Edel;

const VERSION: u32 = 1;

/// How long a reading of the battery holds: a machine unplugged during a
/// game leaves variable refresh within this.
const BATTERY_FOR: Duration = Duration::from_secs(5);

/// Game mode on `Edel`.
#[derive(Default)]
pub struct Game {
    pub mode: GameMode,
    /// The last reading of the battery and when it was taken.
    battery: Cell<Option<(Instant, bool)>>,
}

/// Whether the machine runs on its battery, read at most every
/// [`BATTERY_FOR`] and kept in `reading`.
fn on_battery(reading: &Cell<Option<(Instant, bool)>>) -> bool {
    match reading.get() {
        Some((at, on)) if at.elapsed() < BATTERY_FOR => on,
        _ => {
            let on = crate::tiers::on_battery();
            reading.set(Some((Instant::now(), on)));
            on
        }
    }
}

impl Game {
    /// Whether game mode is on now.
    pub fn on(&self) -> bool {
        self.mode.on(|| on_battery(&self.battery))
    }
}

/// A surface's tearing control: whether it has one, and its hint.
#[derive(Default)]
struct Tearing {
    taken: bool,
    tears: bool,
}

impl Edel {
    /// Follows `power.game_mode` after the settings were read.
    pub fn follow_game_mode(&mut self, when: When) {
        let was = self.game.on();
        self.game.mode.set_when(when);
        if self.game.on() != was {
            self.game_mode_changed("power.game_mode");
        }
    }

    /// Super+G: game mode on or off until the next login.
    pub fn toggle_game_mode(&mut self) {
        let Game { mode, battery } = &mut self.game;
        mode.toggle(|| on_battery(battery));
        self.game_mode_changed("Super+G");
    }

    /// Says game mode's new state, tells shell-ui and draws again, so each
    /// screen takes variable refresh or leaves it.
    fn game_mode_changed(&mut self, why: &str) {
        let on = self.game.on();
        eprintln!(
            "edel-compositor: game mode {} ({why})",
            if on { "on" } else { "off" }
        );
        self.tell_game_mode(on);
        self.dirty = true;
        self.state_changed();
    }

    /// The window on top of `output` when it is fullscreen there.
    pub fn fullscreen_top(&self, output: &Output) -> Option<&Window> {
        let screen = self.space.output_geometry(output)?;
        self.space
            .elements()
            .rev()
            .find(|window| {
                self.space
                    .element_geometry(window)
                    .is_some_and(|place| place.overlaps(screen))
            })
            .filter(|window| self.is_fullscreen(window))
    }

    /// Whether `output` should run at a variable refresh: game mode is on
    /// and a fullscreen window is its top window, while nothing locks it.
    pub fn wants_vrr(&self, output: &Output) -> bool {
        self.locked.is_none() && self.fullscreen_top(output).is_some() && self.game.on()
    }
}

pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, WpTearingControlManagerV1, ()>(VERSION, ());
}

impl GlobalDispatch<WpTearingControlManagerV1, ()> for Edel {
    fn bind(
        _: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpTearingControlManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpTearingControlManagerV1, ()> for Edel {
    fn request(
        _: &mut Edel,
        _: &Client,
        manager: &WpTearingControlManagerV1,
        request: wp_tearing_control_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let wp_tearing_control_manager_v1::Request::GetTearingControl { id, surface } = request
        else {
            return;
        };
        let taken = with_states(&surface, |states| {
            let tearing = states
                .data_map
                .get_or_insert_threadsafe(|| Mutex::new(Tearing::default()));
            let mut tearing = tearing.lock().unwrap();
            std::mem::replace(&mut tearing.taken, true)
        });
        if taken {
            manager.post_error(
                wp_tearing_control_manager_v1::Error::TearingControlExists,
                "the surface already has a tearing control",
            );
            return;
        }
        data_init.init(id, surface.downgrade());
    }
}

impl Dispatch<WpTearingControlV1, Weak<WlSurface>> for Edel {
    fn request(
        _: &mut Edel,
        _: &Client,
        _: &WpTearingControlV1,
        request: wp_tearing_control_v1::Request,
        surface: &Weak<WlSurface>,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        let Ok(surface) = surface.upgrade() else {
            return;
        };
        let (taken, tears) = match request {
            wp_tearing_control_v1::Request::SetPresentationHint { hint } => {
                (true, matches!(hint, WEnum::Value(PresentationHint::Async)))
            }
            wp_tearing_control_v1::Request::Destroy => (false, false),
            _ => return,
        };
        let was = with_states(&surface, |states| {
            let tearing = states
                .data_map
                .get_or_insert_threadsafe(|| Mutex::new(Tearing::default()));
            let mut tearing = tearing.lock().unwrap();
            tearing.taken = taken;
            std::mem::replace(&mut tearing.tears, tears)
        });
        if tears && !was {
            eprintln!(
                "edel-compositor: a window asks to tear; each refresh shows its newest frame"
            );
        }
    }
}
