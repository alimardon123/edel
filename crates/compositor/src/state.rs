//! The compositor's state and the Wayland protocols it serves: surfaces,
//! shared memory, xdg-shell windows, the seat (keyboard and pointer), the
//! clipboard and outputs. Windows live in one smithay `Space`; where they
//! go is the window policies' job from M4.3.

use std::sync::Arc;

use smithay::desktop::{PopupManager, Space, Window, find_popup_root_surface};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::calloop::LoopSignal;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, DisplayHandle};
use smithay::utils::{Logical, Point, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
};

use edel_compositor::telemetry::Telemetry;
use edel_compositor::tokens::Tokens;

pub struct Edel {
    pub display: DisplayHandle,
    pub signal: LoopSignal,
    pub tokens: Tokens,
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub seat: Seat<Edel>,
    /// Something on screen changed since the last frame was drawn.
    pub dirty: bool,
    pub telemetry: Telemetry,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    shm: ShmState,
    seat_state: SeatState<Edel>,
    data_device: DataDeviceState,
    _outputs: OutputManagerState,
}

impl Edel {
    pub fn new(display: DisplayHandle, signal: LoopSignal, tokens: Tokens) -> anyhow::Result<Edel> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display, "seat0");
        seat.add_keyboard(Default::default(), 600, 25)?;
        seat.add_pointer();
        Ok(Edel {
            compositor: CompositorState::new::<Edel>(&display),
            xdg_shell: XdgShellState::new::<Edel>(&display),
            shm: ShmState::new::<Edel>(&display, Vec::new()),
            data_device: DataDeviceState::new::<Edel>(&display),
            _outputs: OutputManagerState::new_with_xdg_output::<Edel>(&display),
            seat_state,
            seat,
            space: Space::default(),
            popups: PopupManager::default(),
            dirty: true,
            telemetry: Telemetry::default(),
            display,
            signal,
            tokens,
        })
    }

    /// The window and the surface under `point`, with the surface's
    /// position, for pointer focus.
    pub fn surface_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (window, location) = self.space.element_under(point)?;
        let (surface, offset) = window.surface_under(
            point - location.to_f64(),
            smithay::desktop::WindowSurfaceType::ALL,
        )?;
        Some((surface, (offset + location).to_f64()))
    }

    fn window_of(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .cloned()
    }
}

/// Per client: the compositor's bookkeeping for its surfaces.
#[derive(Default)]
pub struct ClientState {
    pub compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client: ClientId) {}
    fn disconnected(&self, _client: ClientId, _reason: DisconnectReason) {}
}

impl CompositorHandler for Edel {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("every client is inserted with a ClientState")
            .compositor
    }

    fn commit(&mut self, surface: &WlSurface) {
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
        self.popups.commit(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_of(&root) {
                window.on_commit();
            }
        }
        // The first commit of a toplevel asks for its first configure.
        if let Some(window) = self.window_of(surface) {
            if let Some(toplevel) = window.toplevel() {
                if !toplevel.is_initial_configure_sent() {
                    toplevel.send_configure();
                }
            }
        }
        self.dirty = true;
    }
}

impl BufferHandler for Edel {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Edel {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl XdgShellHandler for Edel {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    /// Until the floating policy (M4.3), a new window opens at the top
    /// left, activated, at the size it asks for.
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.space.map_element(window, (0, 0), true);
        self.dirty = true;
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = self.popups.track_popup(surface.into());
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_of(surface.wl_surface()) {
            self.space.unmap_elem(&window);
        }
        self.dirty = true;
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        if let Ok(root) = find_popup_root_surface(&surface.into()) {
            if self.window_of(&root).is_some() {
                self.dirty = true;
            }
        }
    }
}

impl SeatHandler for Edel {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Edel> {
        &mut self.seat_state
    }
}

impl SelectionHandler for Edel {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Edel {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}

impl ClientDndGrabHandler for Edel {}
impl ServerDndGrabHandler for Edel {}

impl OutputHandler for Edel {}

/// The client data every new client gets.
pub fn new_client_data() -> Arc<ClientState> {
    Arc::new(ClientState::default())
}

delegate_compositor!(Edel);
delegate_shm!(Edel);
delegate_xdg_shell!(Edel);
delegate_seat!(Edel);
delegate_data_device!(Edel);
delegate_output!(Edel);
