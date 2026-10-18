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
//!
//! A game that plays steps the desktop aside (M7.7c, `playing_on`): on a
//! screen whose top window is fullscreen in game mode, that window is
//! drawn alone and the windows behind it get no frame callbacks
//! (`behind_game`), and shell-ui is told to stop its timers. `playing`
//! says whether a game plays on any screen (the state file and the log),
//! `playing_everywhere` whether it plays on every screen (what shell-ui is
//! told, so a clock on another monitor never stops). The battery is
//! shell-ui's to read: it says so over `on_battery` (`battery_changed`);
//! the compositor reads it once, at start, for the starting effects tier.
//!
//! Each game's smoothness is recorded (M7.7d): while a window plays, each
//! of its commits that reaches its screen is one frame (`committed`, then
//! `game_frame_shown` from the backend's page flip), kept in
//! `edel::smoothness::Frames`; when it stops playing, its record replaces
//! its app's in the person's `games.toml`, which `edel report` shows, and
//! the log says it in one line.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

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

use edel::smoothness::Frames;
use edel_compositor::game::{GameMode, When};

use crate::state::Edel;

const VERSION: u32 = 1;

/// Game mode on `Edel`.
#[derive(Default)]
pub struct Game {
    pub mode: GameMode,
    /// Whether the machine runs on its battery, as shell-ui says (M7.7c).
    pub on_battery: bool,
    /// Whether a game played at the last state change, so the log says
    /// when one starts and when it ends (M7.7c).
    pub stepped_aside: Cell<bool>,
    /// The game that plays now and its frames (M7.7d).
    recording: RefCell<Option<Recording>>,
}

/// A game's frames while it plays (M7.7d).
struct Recording {
    window: Window,
    surface: WlSurface,
    app: String,
    /// The screen it plays on.
    output: String,
    /// Whether it committed since its last frame reached the screen.
    fresh: bool,
    frames: Frames,
}

impl Game {
    /// Whether game mode is on now.
    pub fn on(&self) -> bool {
        self.mode.on(|| self.on_battery)
    }

    /// A surface committed (M7.7d): when it is the game's, its next page
    /// flip shows a frame of it.
    pub fn committed(&self, surface: &WlSurface) {
        if let Some(recording) = self.recording.borrow_mut().as_mut() {
            if &recording.surface == surface {
                recording.fresh = true;
            }
        }
    }

    /// A frame reached the screen named `output` at `at`, a monotonic time
    /// (M7.7d): one of the game's when it plays there and committed since.
    pub fn frame_shown(&self, output: &str, at: Duration) {
        if let Some(recording) = self.recording.borrow_mut().as_mut() {
            if recording.output == output && std::mem::take(&mut recording.fresh) {
                recording.frames.shown(at);
            }
        }
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
        let on = self.game.on_battery;
        self.game.mode.toggle(|| on);
        self.game_mode_changed("Super+G");
    }

    /// shell-ui says whether the machine runs on its battery (M7.7c, the
    /// `on_battery` request). Game mode's plugged-in rule follows it; when
    /// that changes game mode, it is logged as the charger's doing.
    pub fn battery_changed(&mut self, on: bool) {
        let was = self.game.on();
        self.game.on_battery = on;
        if self.game.on() != was {
            self.game_mode_changed("the charger");
        }
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

    /// The window that plays on `output` (M7.7c): game mode is on, nothing
    /// locks the session, no overview is open and the screen's top window
    /// is fullscreen.
    pub fn playing_on(&self, output: &Output) -> Option<&Window> {
        if !self.game.on() || self.locked.is_some() || self.overview.is_some() {
            return None;
        }
        self.fullscreen_top(output)
    }

    /// Whether a game plays on any screen (M7.7c).
    pub fn playing(&self) -> bool {
        self.space
            .outputs()
            .any(|output| self.playing_on(output).is_some())
    }

    /// Starts or ends a game's record when the window that plays changes
    /// (M7.7d): the first screen's that plays, from its next commit. Run at
    /// each state change.
    pub fn follow_game_record(&self) {
        let playing = self.space.outputs().find_map(|output| {
            self.playing_on(output)
                .map(|window| (window.clone(), output.name()))
        });
        let mut recording = self.game.recording.borrow_mut();
        let same = match (recording.as_ref(), playing.as_ref()) {
            (Some(now), Some((window, output))) => &now.window == window && &now.output == output,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(ended) = recording.take() {
            keep_record(&ended);
        }
        *recording = playing.and_then(|(window, output)| {
            let surface = window.toplevel()?.wl_surface().clone();
            Some(Recording {
                app: crate::decoration::app_id(&window),
                window,
                surface,
                output,
                fresh: false,
                frames: Frames::default(),
            })
        });
    }

    /// Whether a game plays on every screen, and there is one (M7.7c).
    pub fn playing_everywhere(&self) -> bool {
        self.space.outputs().next().is_some()
            && self
                .space
                .outputs()
                .all(|output| self.playing_on(output).is_some())
    }

    /// Whether `output` should run at a variable refresh: a window plays
    /// on it (`playing_on`), which needs game mode and nothing locking.
    pub fn wants_vrr(&self, output: &Output) -> bool {
        self.playing_on(output).is_some()
    }

    /// Whether `window` lies behind a game on every screen it is on (M7.7c):
    /// each of them plays another window, so it gets no frame callback
    /// until it is visible again. A window on no screen is not behind.
    pub fn behind_game(&self, window: &Window) -> bool {
        let outputs = self.space.outputs_for_element(window);
        !outputs.is_empty()
            && outputs
                .iter()
                .all(|output| self.playing_on(output).is_some_and(|top| top != window))
    }
}

/// Logs a game's record and keeps it in the person's games file (M7.7d);
/// a game too short for one keeps nothing.
fn keep_record(ended: &Recording) {
    let Some(record) = ended.frames.record() else {
        return;
    };
    let app = if ended.app.is_empty() {
        "unknown"
    } else {
        ended.app.as_str()
    };
    eprintln!(
        "edel-compositor: game {app}: {} frames in {} s, average {} fps, slowest 1% {} ms, slowest 0.1% {} ms",
        record.frames,
        record.seconds,
        record.average_fps,
        record.slowest_1_percent_ms,
        record.slowest_0_1_percent_ms
    );
    let Some(dir) = edel::places::person_state_dir() else {
        return;
    };
    if let Err(e) = write_record(&dir, app, record) {
        eprintln!(
            "edel-compositor: {}",
            crate::messages::game_record_not_kept(e)
        );
    }
}

/// Replaces `app`'s record in the games file in `dir`, through a new file
/// renamed over it.
fn write_record(dir: &Path, app: &str, record: edel::smoothness::Record) -> std::io::Result<()> {
    let path = dir.join(edel::places::GAMES);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new = dir.join(format!("{}.new", edel::places::GAMES));
    std::fs::create_dir_all(dir)?;
    std::fs::write(&new, edel::smoothness::with(&text, app, record))?;
    std::fs::rename(&new, &path)
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
