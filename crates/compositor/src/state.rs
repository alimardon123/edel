//! The compositor's state and the Wayland protocols it serves: surfaces,
//! shared memory, xdg-shell windows, the seat (keyboard and pointer), the
//! clipboard and outputs. Windows live in one smithay `Space`; where they
//! go is the window policies' job from M4.3.

use std::sync::Arc;

use anyhow::{Context, Result};
use smithay::backend::input::KeyState;
use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent, KeyboardKeyEvent,
    PointerButtonEvent, PointerMotionEvent,
};
use smithay::input::keyboard::{FilterResult, xkb};
use smithay::input::pointer::{ButtonEvent, MotionEvent, RelativeMotionEvent};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode as TriggerMode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::socket::ListeningSocketSource;

use smithay::desktop::{PopupManager, Space, Window, find_popup_root_surface};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::calloop::LoopSignal;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, DisplayHandle};
use smithay::utils::{Clock, Logical, Monotonic, Point, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_output, delegate_presentation,
    delegate_seat, delegate_shm, delegate_xdg_shell,
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
    /// A timer will log the telemetry once the screen is still (`drm.rs`).
    pub report_armed: bool,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    shm: ShmState,
    seat_state: SeatState<Edel>,
    data_device: DataDeviceState,
    _outputs: OutputManagerState,
    _presentation: PresentationState,
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
            // When a frame reached the screen, on the monotonic clock: what
            // CI's frame times are read from (M4.1).
            _presentation: PresentationState::new::<Edel>(
                &display,
                Clock::<Monotonic>::new().id() as u32,
            ),
            seat_state,
            seat,
            space: Space::default(),
            popups: PopupManager::default(),
            dirty: true,
            telemetry: Telemetry::default(),
            report_armed: false,
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

    /// Keyboard to the focused window; the pointer moves, focuses and
    /// clicks the window under it. Floating placement, moving and
    /// resizing come with the floating policy (M4.3). Returns the virtual
    /// terminal Ctrl+Alt+F1 to F12 asked for, which only a real seat can
    /// switch to, so a person can always reach a text console.
    pub fn input<B: InputBackend>(&mut self, event: InputEvent<B>) -> Option<i32> {
        let serial = SERIAL_COUNTER.next_serial();
        match event {
            InputEvent::Keyboard { event } => {
                let pressed = event.state() == KeyState::Pressed;
                let keyboard = self.seat.get_keyboard()?;
                return keyboard.input(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    event.time_msec(),
                    |_, _, keysym| {
                        let sym = keysym.modified_sym().raw();
                        let vts =
                            xkb::keysyms::KEY_XF86Switch_VT_1..=xkb::keysyms::KEY_XF86Switch_VT_12;
                        if pressed && vts.contains(&sym) {
                            FilterResult::Intercept(
                                (sym - xkb::keysyms::KEY_XF86Switch_VT_1 + 1) as i32,
                            )
                        } else {
                            FilterResult::Forward
                        }
                    },
                );
            }
            InputEvent::PointerMotion { event } => {
                let pointer = self.seat.get_pointer()?;
                let area = self.output_area()?;
                let mut location = pointer.current_location() + event.delta();
                location.x = location.x.clamp(
                    f64::from(area.loc.x),
                    f64::from(area.loc.x + area.size.w - 1),
                );
                location.y = location.y.clamp(
                    f64::from(area.loc.y),
                    f64::from(area.loc.y + area.size.h - 1),
                );
                let focus = self.surface_under(location);
                pointer.motion(
                    self,
                    focus.clone(),
                    &MotionEvent {
                        location,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.relative_motion(
                    self,
                    focus,
                    &RelativeMotionEvent {
                        delta: event.delta(),
                        delta_unaccel: event.delta_unaccel(),
                        utime: event.time(),
                    },
                );
                pointer.frame(self);
                self.dirty = true;
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let area = self.output_area()?;
                let location = event.position_transformed(area.size) + area.loc.to_f64();
                let focus = self.surface_under(location);
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.motion(
                        self,
                        focus,
                        &MotionEvent {
                            location,
                            serial,
                            time: event.time_msec(),
                        },
                    );
                    pointer.frame(self);
                }
                self.dirty = true;
            }
            InputEvent::PointerButton { event } => {
                let pointer = self.seat.get_pointer()?;
                if event.state() == ButtonState::Pressed {
                    let location = pointer.current_location();
                    let window = self.space.element_under(location).map(|(w, _)| w.clone());
                    if let Some(window) = window {
                        self.space.raise_element(&window, true);
                        let surface = window.toplevel().map(|t| t.wl_surface().clone());
                        if let Some(keyboard) = self.seat.get_keyboard() {
                            keyboard.set_focus(self, surface, serial);
                        }
                        self.dirty = true;
                    }
                }
                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: event.state(),
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            _ => {}
        }
        None
    }

    /// The first output's area in the layout.
    fn output_area(&self) -> Option<smithay::utils::Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space.output_geometry(output)
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

/// Opens the Wayland socket (`wayland-1` or the next free name) and
/// serves clients from the event loop; returns the socket's name.
pub fn listen(handle: &LoopHandle<'static, Edel>, display: Display<Edel>) -> Result<String> {
    let socket = ListeningSocketSource::new_auto().context("opening a Wayland socket")?;
    let name = socket.socket_name().to_string_lossy().into_owned();
    handle
        .insert_source(socket, |stream, _, state: &mut Edel| {
            let data = Arc::new(ClientState::default());
            if let Err(e) = state.display.insert_client(stream, data) {
                eprintln!("edel-compositor: a client could not connect: {e}");
            }
        })
        .map_err(|e| anyhow::anyhow!("listening on the socket: {e}"))?;
    handle
        .insert_source(
            Generic::new(display, Interest::READ, TriggerMode::Level),
            |_, display, state: &mut Edel| {
                // SAFETY: the display is never dropped while the loop runs.
                unsafe { display.get_mut().dispatch_clients(state)? };
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("watching the display: {e}"))?;
    Ok(name)
}

delegate_compositor!(Edel);
delegate_shm!(Edel);
delegate_xdg_shell!(Edel);
delegate_seat!(Edel);
delegate_data_device!(Edel);
delegate_output!(Edel);
delegate_presentation!(Edel);
