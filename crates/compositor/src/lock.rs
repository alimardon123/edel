//! The lock screen (roadmap M5.10): ext-session-lock-v1, which shell-ui
//! uses to cover every screen with its lock surface until the person's
//! password is typed. While locked the compositor draws only each
//! screen's lock surface, over the tokens' `backdrop_deep` until it has
//! drawn, gives the keyboard and the pointer to it alone and acts on no
//! shortcut, so a click on a window does nothing; Super+L (`lock_screen`)
//! asks shell-ui to lock over `edel-shell-v1`. A lock is confirmed to its
//! client only once every screen has drawn a locked frame, and a lock
//! whose client dies stays locked: shell-ui, started again, is asked to
//! lock once more. Logs `screen locked` and `screen unlocked`.

use std::collections::HashMap;

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::solid::{SolidColorBuffer, SolidColorRenderElement};
use smithay::backend::renderer::element::surface::render_elements_from_surface_tree;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Physical, Point, Scale};
use smithay::wayland::session_lock::{
    LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
};

use crate::render::{Drawn, Element};
use crate::state::Edel;

/// The session while it is locked.
pub struct Locked {
    /// The lock object, whose client alone may unlock.
    lock: ExtSessionLockV1,
    /// Each screen's lock surface, by the screen's name.
    pub surfaces: HashMap<String, LockSurface>,
    /// The lock, confirmed once every screen has drawn locked.
    confirm: Option<SessionLocker>,
    /// Each screen's plain cover, under its lock surface.
    covers: HashMap<String, SolidColorBuffer>,
    /// The screens that have drawn a locked frame.
    drawn: std::collections::HashSet<String>,
}

impl SessionLockHandler for Edel {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        // A second lock while a living one holds is refused (dropping it
        // tells its client `finished`); one whose client died is replaced,
        // so a shell-ui started again can show the lock screen.
        if self.locked.as_ref().is_some_and(|l| l.lock.is_alive()) {
            return;
        }
        let relock = self.locked.is_some();
        if self.overview.is_some() {
            self.leave_overview();
        }
        if self.switcher.take().is_some() {
            self.hide_switcher();
        }
        self.locked = Some(Locked {
            lock: confirmation.ext_session_lock().clone(),
            surfaces: HashMap::new(),
            confirm: Some(confirmation),
            covers: HashMap::new(),
            drawn: std::collections::HashSet::new(),
        });
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, smithay::utils::SERIAL_COUNTER.next_serial());
        }
        if !relock {
            eprintln!("edel-compositor: screen locked");
        }
        self.dirty = true;
        self.state_changed();
    }

    fn unlock(&mut self) {
        if self.locked.take().is_none() {
            return;
        }
        eprintln!("edel-compositor: screen unlocked");
        self.focus_top();
        self.dirty = true;
        self.state_changed();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return;
        };
        surface.with_pending_state(|state| {
            state.size = Some((area.size.w.max(1) as u32, area.size.h.max(1) as u32).into());
        });
        surface.send_configure();
        let wl_surface = surface.wl_surface().clone();
        let Some(locked) = self.locked.as_mut() else {
            return;
        };
        locked.surfaces.insert(output.name(), surface);
        // The keyboard goes to the lock on the pointer's screen, or the
        // first that comes.
        let first = locked.surfaces.len() == 1;
        let pointer = self.pointer_screen().map(|(name, _)| name);
        if pointer.as_deref() == Some(output.name().as_str()) || first {
            if let Some(keyboard) = self.seat.get_keyboard() {
                keyboard.set_focus(
                    self,
                    Some(wl_surface),
                    smithay::utils::SERIAL_COUNTER.next_serial(),
                );
            }
        }
        self.dirty = true;
    }
}

smithay::delegate_session_lock!(Edel);

impl Edel {
    /// Whether the session is locked.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// Whether the session is locked with no living lock client: shell-ui
    /// is asked to lock again when it links up.
    pub fn lock_orphaned(&self) -> bool {
        self.locked.as_ref().is_some_and(|l| !l.lock.is_alive())
    }

    /// `output`'s lock surface, if the session is locked and it has one.
    pub fn lock_surface(&self, output: &Output) -> Option<WlSurface> {
        self.locked
            .as_ref()?
            .surfaces
            .get(&output.name())
            .filter(|s| s.alive())
            .map(|s| s.wl_surface().clone())
    }

    /// What a locked screen draws: its lock surface over a plain cover in
    /// the tokens' `backdrop_deep`, which shows alone until the lock
    /// surface has drawn.
    pub fn lock_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: f64,
    ) -> Vec<Drawn> {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let mut drawn: Vec<Drawn> = Vec::new();
        if let Some(surface) = self.lock_surface(output) {
            drawn.extend(
                render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    Point::<i32, Physical>::from((0, 0)),
                    Scale::from(scale),
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e))),
            );
        }
        let colour = self.tokens.backdrop_deep.rgba();
        let Some(locked) = self.locked.as_mut() else {
            return drawn;
        };
        let cover = locked.covers.entry(output.name()).or_default();
        cover.update(area.size, colour);
        drawn.push(Drawn::Plain(Element::Border(
            SolidColorRenderElement::from_buffer(
                cover,
                Point::<i32, Physical>::from((0, 0)),
                scale,
                1.0,
                Kind::Unspecified,
            ),
        )));
        drawn
    }

    /// The lock surface under `point`, and where its corner lies.
    pub fn lock_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(point).next()?.clone();
        let area = self.space.output_geometry(&output)?;
        let surface = self.lock_surface(&output)?;
        Some((surface, area.loc.to_f64()))
    }

    /// A locked frame was drawn on the screen called `name`: once every
    /// screen has drawn one, the lock is confirmed to its client, as
    /// nothing of the session shows any more.
    pub fn lock_drawn(&mut self, name: &str) {
        let screens: Vec<String> = self.space.outputs().map(|o| o.name()).collect();
        let Some(locked) = self.locked.as_mut() else {
            return;
        };
        if locked.confirm.is_none() {
            return;
        }
        locked.drawn.insert(name.to_string());
        if screens.iter().all(|s| locked.drawn.contains(s)) {
            if let Some(confirm) = locked.confirm.take() {
                confirm.lock();
            }
        }
    }

    /// Frame callbacks for the lock surfaces on `output`.
    pub fn lock_frames(&self, output: &Output, now: std::time::Duration) {
        if let Some(surface) = self.lock_surface(output) {
            send_frames_surface_tree(&surface, output, now, None, |_, _| Some(output.clone()));
        }
    }

    /// Super+L: shell-ui is asked to lock the screen. With no shell-ui to
    /// ask, nothing locks, and the log says so.
    pub fn ask_lock(&mut self) {
        if self.is_locked() {
            return;
        }
        if !self.tell_lock() {
            eprintln!(
                "edel-compositor: the screen was not locked: edel-shell-ui, which draws the lock screen, is not running"
            );
        }
    }
}
