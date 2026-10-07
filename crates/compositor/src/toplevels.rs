//! wlr-foreign-toplevel-management (roadmap M5.2d): the windows as panels
//! and docks see them, shell-ui's window list first. Each window, in the
//! order the client first heard of it, with its title, app id, the screens it is on
//! and whether it is focused, maximized or minimized (M5.2h); and requests
//! to activate it (showing its workspace first, and bringing it back if
//! it is minimized), maximize, minimize and close it. A window on a
//! hidden workspace is on no screen, which is how a list of one
//! workspace's windows leaves it out; a minimized one stays on the
//! screens it was on. `sync` runs whenever the state file is written and
//! when the focus moves, and sends each client only what changed, then
//! `done`.

use smithay::desktop::Window;
use smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

use crate::state::Edel;

/// The protocol's states, as its enum numbers them.
const MAXIMIZED: u32 = 0;
const MINIMIZED: u32 = 1;
const ACTIVATED: u32 = 2;
const FULLSCREEN: u32 = 3;

/// Every client's manager and the handles it was given.
#[derive(Default)]
pub struct Toplevels {
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    handles: Vec<Entry>,
}

/// One window as one manager was told of it.
struct Entry {
    window: Window,
    handle: ZwlrForeignToplevelHandleV1,
    sent: Seen,
    /// Told nothing yet: the first `done` goes out even when there is
    /// nothing to say, as the protocol asks.
    fresh: bool,
}

/// What a client was last told of a window.
#[derive(Default, Clone, PartialEq)]
struct Seen {
    title: String,
    app_id: String,
    outputs: Vec<WlOutput>,
    states: Vec<u32>,
}

/// Offers `zwlr_foreign_toplevel_manager_v1` to every client.
pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, ZwlrForeignToplevelManagerV1, ()>(3, ());
}

impl Edel {
    /// Every window the lists show, in the order it first showed: those
    /// on screen, then those on hidden workspaces.
    fn listed(&self) -> Vec<Window> {
        self.space
            .elements()
            .chain(self.desks.hidden().map(|(_, window, _)| window))
            .cloned()
            .collect()
    }

    /// What `window` is now, as `client` would be told.
    fn seen(&self, window: &Window, client: &Client) -> Seen {
        let (title, app_id) = window
            .toplevel()
            .and_then(|t| {
                with_states(t.wl_surface(), |states| {
                    let data = states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()?
                        .lock()
                        .ok()?;
                    Some((data.title.clone(), data.app_id.clone()))
                })
            })
            .unwrap_or_default();
        // From the geometry, not the space's own list, which follows only
        // when the screens are next drawn; a window minimized on the shown
        // workspace is where it was.
        let minimized = self.desks.minimized(window);
        let place = self.space.element_geometry(window).or_else(|| {
            let (desk, frame) = minimized?;
            (desk == self.desks.active()).then(|| self.insets(window).window(frame))
        });
        let mut screens: Vec<&Output> = self
            .space
            .outputs()
            .filter(|output| {
                let screen = self.space.output_geometry(output);
                matches!((place, screen), (Some(p), Some(s)) if p.overlaps(s))
            })
            .collect();
        // A window minimized here whose screen went is on the one it would
        // come back on, so the window list still offers it.
        if screens.is_empty() && minimized.is_some_and(|(desk, _)| desk == self.desks.active()) {
            let name = self.desks.minimized_screen(window);
            screens.extend(
                self.space
                    .outputs()
                    .find(|o| Some(o.name().as_str()) == name)
                    .or_else(|| self.space.outputs().next()),
            );
        }
        let outputs = screens
            .into_iter()
            .flat_map(|output| output.client_outputs(client))
            .collect();
        let focused = self.seat.get_keyboard().and_then(|k| k.current_focus());
        let mut states = Vec::new();
        if self.is_maximized(window) {
            states.push(MAXIMIZED);
        }
        if minimized.is_some() {
            states.push(MINIMIZED);
        }
        if self.is_fullscreen(window) {
            states.push(FULLSCREEN);
        }
        if window
            .toplevel()
            .is_some_and(|t| focused.as_ref() == Some(t.wl_surface()))
        {
            states.push(ACTIVATED);
        }
        Seen {
            title: title.unwrap_or_default(),
            app_id: app_id.unwrap_or_default(),
            outputs,
            states,
        }
    }

    /// Tells every client what changed about the windows since it was
    /// last told: new windows get a handle, closed ones `closed`.
    pub fn sync_toplevels(&self) {
        let Ok(mut toplevels) = self.toplevels.try_borrow_mut() else {
            return;
        };
        let windows = self.listed();
        toplevels.managers.retain(|m| m.is_alive());
        // Closed windows, or handles a client let go of.
        toplevels.handles.retain(|entry| {
            let keep = entry.handle.is_alive() && windows.contains(&entry.window);
            if !keep && entry.handle.is_alive() {
                entry.handle.closed();
            }
            keep
        });
        let managers = toplevels.managers.clone();
        for manager in &managers {
            let Some(client) = manager.client() else {
                continue;
            };
            for window in &windows {
                let known = toplevels
                    .handles
                    .iter()
                    .any(|e| &e.window == window && e.handle.client().as_ref() == Some(&client));
                if known {
                    continue;
                }
                let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, (), Edel>(
                    &self.display,
                    manager.version(),
                    (),
                ) else {
                    continue;
                };
                manager.toplevel(&handle);
                toplevels.handles.push(Entry {
                    window: window.clone(),
                    handle,
                    sent: Seen::default(),
                    fresh: true,
                });
            }
        }
        for entry in &mut toplevels.handles {
            let Some(client) = entry.handle.client() else {
                continue;
            };
            let now = self.seen(&entry.window, &client);
            if now == entry.sent && !entry.fresh {
                continue;
            }
            entry.fresh = false;
            if now.title != entry.sent.title {
                entry.handle.title(now.title.clone());
            }
            if now.app_id != entry.sent.app_id {
                entry.handle.app_id(now.app_id.clone());
            }
            for output in &now.outputs {
                if !entry.sent.outputs.contains(output) {
                    entry.handle.output_enter(output);
                }
            }
            for output in &entry.sent.outputs {
                if !now.outputs.contains(output) && output.is_alive() {
                    entry.handle.output_leave(output);
                }
            }
            if now.states != entry.sent.states {
                let bytes = now.states.iter().flat_map(|s| s.to_ne_bytes()).collect();
                entry.handle.state(bytes);
            }
            entry.handle.done();
            entry.sent = now;
        }
    }

    /// The window a handle stands for.
    fn window_of_handle(&self, handle: &ZwlrForeignToplevelHandleV1) -> Option<Window> {
        self.toplevels
            .borrow()
            .handles
            .iter()
            .find(|e| &e.handle == handle)
            .map(|e| e.window.clone())
    }
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for Edel {
    fn bind(
        state: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let manager = data_init.init(resource, ());
        state.toplevels.borrow_mut().managers.push(manager);
        state.sync_toplevels();
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state
                .toplevels
                .borrow_mut()
                .managers
                .retain(|m| m != resource);
            resource.finished();
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        let Some(window) = state.window_of_handle(resource) else {
            return;
        };
        let shown = state.space.element_geometry(&window).is_some();
        match request {
            // Its workspace first, then the window, as a click in a
            // window list means; a minimized window comes back.
            zwlr_foreign_toplevel_handle_v1::Request::Activate { .. } => {
                if !state.restore(&window) {
                    if let Some(desk) = state.desks.hidden_on(&window) {
                        state.switch_workspace(desk);
                    }
                    if state.space.element_geometry(&window).is_some() {
                        state.focus(&window);
                        state.state_changed();
                    }
                }
            }
            zwlr_foreign_toplevel_handle_v1::Request::Close => state.close(&window),
            zwlr_foreign_toplevel_handle_v1::Request::SetMaximized if shown => {
                state.maximize(&window);
            }
            zwlr_foreign_toplevel_handle_v1::Request::UnsetMaximized if shown => {
                state.unmaximize(&window);
            }
            zwlr_foreign_toplevel_handle_v1::Request::SetMinimized if shown => {
                state.minimize(&window);
            }
            zwlr_foreign_toplevel_handle_v1::Request::UnsetMinimized => {
                state.restore(&window);
            }
            zwlr_foreign_toplevel_handle_v1::Request::SetFullscreen { output } if shown => {
                let output = output.as_ref().and_then(Output::from_resource);
                state.fullscreen(&window, output);
            }
            zwlr_foreign_toplevel_handle_v1::Request::UnsetFullscreen if shown => {
                state.unfullscreen(&window);
            }
            // The rectangle a list draws a window's entry in means nothing
            // here yet.
            _ => {}
        }
        state.sync_toplevels();
    }

    fn destroyed(
        state: &mut Edel,
        _: smithay::reexports::wayland_server::backend::ClientId,
        resource: &ZwlrForeignToplevelHandleV1,
        _: &(),
    ) {
        state
            .toplevels
            .borrow_mut()
            .handles
            .retain(|e| &e.handle != resource);
    }
}
