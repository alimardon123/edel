//! The compositor's state and the Wayland protocols it serves: surfaces,
//! shared memory, xdg-shell windows, the seat (keyboard and pointer), the
//! clipboard, outputs, presentation time and server-side decorations.
//! Windows live in one smithay `Space`; the workspace's active policy,
//! floating (M4.3) or tiling (M4.5), decides where each frame goes, the
//! window sits inside its frame (M4.4), the settings from the system file
//! apply live (M4.5), and every change of what is on screen is written to
//! the state file.

use std::sync::Arc;

use anyhow::{Context, Result};
use smithay::desktop::{PopupManager, Space, Window, find_popup_root_surface};
use smithay::input::pointer::{CursorImageStatus, Focus};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{
    Interest, LoopHandle, LoopSignal, Mode as TriggerMode, PostAction,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::{
    ResizeEdge, State, WmCapabilities,
};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource as _};
use smithay::utils::{Clock, Logical, Monotonic, Rectangle, SERIAL_COUNTER, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
    with_states,
};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::tablet_manager::{TabletManagerState, TabletSeatHandler};
use smithay::wayland::viewporter::ViewporterState;
use smithay::{
    delegate_compositor, delegate_cursor_shape, delegate_data_device, delegate_fractional_scale,
    delegate_output, delegate_presentation, delegate_seat, delegate_shm, delegate_tablet_manager,
    delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
};
use toml::{Table, Value};

use edel::system::MACHINE_FILE;
use edel_compositor::frame::{Button, Text};
use edel_compositor::layout::Workspace;
use edel_compositor::settings::{self, Settings};
use edel_compositor::telemetry::Telemetry;
use edel_compositor::tokens::Tokens;

use crate::decoration::{data, server_side, title};
use crate::grabs::{Kind, WindowGrab};
use crate::pointer::Cursors;
use crate::statefile::{self, StateFile};

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
    /// A window is being moved or resized (`grabs.rs`). Kept here because
    /// asking the pointer from inside its grab would wait on its own lock.
    pub dragging: bool,
    /// The title font, once its thread has loaded it (`decoration.rs`).
    pub text: Option<Text>,
    /// The cursor and its images (`pointer.rs`).
    pub cursors: Cursors,
    /// The title bar button under the pointer.
    pub hover: Option<(Window, Button)>,
    /// The title bar button the left button went down on.
    pub pressed: Option<(Window, Button)>,
    /// The last click on a title bar, and when, for double clicks.
    pub last_title_click: Option<(Window, u32)>,
    /// What the system file says (`watch.rs`).
    pub settings: Settings,
    /// `[outputs]` changed: the backend scans and places its screens again
    /// (`drm.rs`).
    pub screens_changed: bool,
    /// The X11 display for X11 apps (`xwayland.rs`).
    pub x11: Option<crate::xwayland::X11Display>,
    /// Where window frames go; one workspace until M5.2.
    workspace: Workspace<Window>,
    /// Windows whose first buffer has not come yet, so their size is not
    /// known and they are not placed or shown.
    unplaced: Vec<Window>,
    state_file: StateFile,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    shm: ShmState,
    seat_state: SeatState<Edel>,
    data_device: DataDeviceState,
    _decorations: XdgDecorationState,
    _fractional_scale: FractionalScaleManagerState,
    _viewporter: ViewporterState,
    _tablets: TabletManagerState,
    _cursor_shapes: CursorShapeManagerState,
    _outputs: OutputManagerState,
    _presentation: PresentationState,
}

impl Edel {
    pub fn new(display: DisplayHandle, signal: LoopSignal, tokens: Tokens) -> Result<Edel> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display, "seat0");
        seat.add_keyboard(Default::default(), 600, 25)?;
        seat.add_pointer();
        Ok(Edel {
            compositor: CompositorState::new::<Edel>(&display),
            // Minimize waits for the window list that brings a window back
            // (M5.2), fullscreen and the window menu for their own steps;
            // apps that draw their own bars leave out what is missing.
            xdg_shell: XdgShellState::new_with_capabilities::<Edel>(
                &display,
                [WmCapabilities::Maximize],
            ),
            shm: ShmState::new::<Edel>(&display, Vec::new()),
            data_device: DataDeviceState::new::<Edel>(&display),
            _decorations: XdgDecorationState::new::<Edel>(&display),
            _fractional_scale: FractionalScaleManagerState::new::<Edel>(&display),
            _viewporter: ViewporterState::new::<Edel>(&display),
            // Pens and drawing tablets (M4.6b), and cursors named by shape,
            // as GTK 4 asks for them.
            _tablets: TabletManagerState::new::<Edel>(&display),
            _cursor_shapes: CursorShapeManagerState::new::<Edel>(&display),
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
            dragging: false,
            text: None,
            cursors: Cursors::new(),
            hover: None,
            pressed: None,
            last_title_click: None,
            settings: Settings::default(),
            screens_changed: false,
            x11: None,
            workspace: Workspace::new(tokens.gap),
            unplaced: Vec::new(),
            state_file: StateFile::start(),
            display,
            signal,
            tokens,
        })
    }

    /// Raises `window` and gives it the keyboard.
    pub fn focus(&mut self, window: &Window) {
        self.space.raise_element(window, true);
        let surface = window.toplevel().map(|t| t.wl_surface().clone());
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
        }
        self.dirty = true;
    }

    /// `window` was put at `place` by a person, or back from maximized:
    /// the policy decides where its frame stays (floating keeps it, tiling
    /// puts it back in its tile), and the state file says so.
    pub fn placed(&mut self, window: &Window, place: Rectangle<i32, Logical>) {
        let insets = self.insets(window);
        let frame = self.workspace.moved(window, insets.frame(place));
        self.put(window, frame);
        self.dirty = true;
        self.state_changed();
    }

    /// Puts `window`'s frame at `frame`: the window hears its new size,
    /// and whether it is tiled, so an app that draws its own frame leaves
    /// out its shadow and rounded corners there.
    fn put(&mut self, window: &Window, frame: Rectangle<i32, Logical>) {
        let place = self.insets(window).window(frame);
        let tiled = self.workspace.rearranges();
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.size = Some(place.size);
                for edge in [
                    State::TiledLeft,
                    State::TiledRight,
                    State::TiledTop,
                    State::TiledBottom,
                ] {
                    if tiled {
                        state.states.set(edge);
                    } else {
                        state.states.unset(edge);
                    }
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.space.map_element(window.clone(), place.loc, false);
        self.send_scale(window);
    }

    /// Every window where the active policy puts it; maximized windows
    /// fill the screen again, and the pointer stays on the screen.
    pub fn relayout(&mut self) {
        self.keep_pointer_on_screen();
        if let Some(area) = self.output_area() {
            for (window, frame) in self.workspace.arrange(area) {
                if self.is_maximized(&window) {
                    self.maximize(&window);
                } else {
                    self.put(&window, frame);
                }
            }
        }
        self.dirty = true;
        self.state_changed();
    }

    /// Makes `name` the workspace's policy and lays the windows out again.
    pub fn switch_policy(&mut self, name: &str) {
        if self.workspace.switch(name) {
            eprintln!("edel-compositor: windows now {name}");
            self.relayout();
        }
    }

    /// Super+T: the other policy, for this workspace only; the system file
    /// is left as it is.
    pub fn toggle_tiling(&mut self) {
        let next = self.workspace.next();
        self.switch_policy(next);
    }

    /// Reads both system files again and applies what changed.
    pub fn reload_settings(&mut self) {
        let person = settings::person_file();
        let (new, notes) = settings::load(std::path::Path::new(MACHINE_FILE), person.as_deref());
        for note in notes {
            eprintln!("edel-compositor: {note}");
        }
        let old = std::mem::replace(&mut self.settings, new.clone());
        if old.outputs != new.outputs {
            self.screens_changed = true;
        }
        let rescaled = self.apply_scales();
        if old.tiling != new.tiling {
            self.switch_policy(new.policy());
        } else if old.title_bars != new.title_bars || rescaled {
            self.relayout();
        }
    }

    /// Whether the compositor draws title bars under the active policy.
    pub fn bars_shown(&self) -> bool {
        self.settings.bars_in(self.workspace.name())
    }

    /// `window` leaves the screen, closed or hidden: the policy forgets its
    /// place and the keyboard goes to the window now on top.
    fn unmap(&mut self, window: &Window) {
        eprintln!("edel-compositor: unmapped window {}", title(window));
        self.forget(window);
        let mut frame = data(window).borrow_mut();
        frame.restore = None;
        frame.shape = None;
        drop(frame);
        self.workspace.close(window);
        self.space.unmap_elem(window);
        if let Some(top) = self.space.elements().last().cloned() {
            self.focus(&top);
        }
        if self.workspace.rearranges() {
            self.relayout();
        }
        self.dirty = true;
        self.state_changed();
    }

    /// A shown window may have drawn itself at a new size, or taken or
    /// given up its title bar: the policy and the state file learn its
    /// frame now. Its place stays.
    fn reshaped(&mut self, window: &Window) {
        let Some(place) = self.space.element_geometry(window) else {
            return;
        };
        let shape = (place.size, server_side(window));
        if data(window).borrow_mut().shape.replace(shape) == Some(shape) {
            return;
        }
        self.workspace
            .moved(window, self.insets(window).frame(place));
        self.state_changed();
    }

    /// The outputs changed: the policy fits every frame to the new area,
    /// and maximized windows fill it.
    pub fn outputs_changed(&mut self) {
        self.relayout();
    }

    /// Sends the state file what is on screen now. Never during a drag:
    /// the grab writes it once, when it ends.
    pub fn state_changed(&self) {
        if self.dragging {
            return;
        }
        self.state_file.send(self.state_toml());
    }

    /// Outputs and windows, bottom of the stack first, as TOML.
    fn state_toml(&self) -> String {
        let mut table = Table::new();
        table.insert("format".into(), Value::Integer(statefile::FORMAT));
        table.insert("policy".into(), Value::String(self.workspace.name().into()));
        let outputs = self
            .space
            .outputs()
            .filter_map(|output| {
                let area = self.space.output_geometry(output)?;
                let mut t = Table::new();
                t.insert("name".into(), Value::String(output.name()));
                t.insert(
                    "scale".into(),
                    Value::Float(output.current_scale().fractional_scale()),
                );
                insert_rect(&mut t, area);
                Some(Value::Table(t))
            })
            .collect();
        table.insert("outputs".into(), Value::Array(outputs));
        let focused = self.seat.get_keyboard().and_then(|k| k.current_focus());
        let windows = self
            .space
            .elements()
            .filter_map(|window| {
                let toplevel = window.toplevel()?;
                let place = self.space.element_geometry(window)?;
                let (title, app_id) = with_states(toplevel.wl_surface(), |states| {
                    let data = states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()?
                        .lock()
                        .ok()?;
                    Some((data.title.clone(), data.app_id.clone()))
                })
                .unwrap_or_default();
                let mut t = Table::new();
                t.insert("title".into(), Value::String(title.unwrap_or_default()));
                t.insert("app_id".into(), Value::String(app_id.unwrap_or_default()));
                insert_rect(&mut t, place);
                let is_focused = focused.as_ref() == Some(toplevel.wl_surface());
                t.insert("focused".into(), Value::Boolean(is_focused));
                t.insert(
                    "title_bar".into(),
                    Value::Boolean(self.insets(window).top > 0),
                );
                t.insert(
                    "maximized".into(),
                    Value::Boolean(self.is_maximized(window)),
                );
                Some(Value::Table(t))
            })
            .collect();
        table.insert("windows".into(), Value::Array(windows));
        toml::to_string(&table).unwrap_or_default()
    }

    pub fn window_of(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .chain(&self.unplaced)
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .cloned()
    }

    /// Starts a move or resize the window asked for, if the button that
    /// asked is still down on it.
    fn grab_for(&mut self, surface: &ToplevelSurface, serial: Serial, kind: Kind) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !pointer.has_grab(serial) {
            return;
        }
        let Some(start) = pointer.grab_start_data() else {
            return;
        };
        let asked = start
            .focus
            .as_ref()
            .is_some_and(|(focus, _)| focus.id().same_client_as(&surface.wl_surface().id()));
        let Some(window) = self.window_of(surface.wl_surface()) else {
            return;
        };
        let Some(place) = self.space.element_geometry(&window) else {
            return;
        };
        // A maximized window moves once dragged far enough (`grabs.rs`),
        // but keeps its size.
        if !asked || (kind != Kind::Move && self.is_maximized(&window)) {
            return;
        }
        let grab = WindowGrab {
            start,
            window,
            kind,
            initial: place,
            current: place,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
        self.dragging = true;
    }
}

/// Whether `surface` has a buffer to show.
fn has_buffer(surface: &WlSurface) -> bool {
    smithay::backend::renderer::utils::with_renderer_surface_state(surface, |s| {
        s.buffer().is_some()
    })
    .unwrap_or(false)
}

fn insert_rect(table: &mut Table, rect: Rectangle<i32, Logical>) {
    for (key, value) in [
        ("x", rect.loc.x),
        ("y", rect.loc.y),
        ("width", rect.size.w),
        ("height", rect.size.h),
    ] {
        table.insert(key.into(), Value::Integer(value.into()));
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
                // New subsurfaces and popups hear the scale too.
                self.send_scale(&window);
                // A shown window that drops its buffer hides itself; it
                // shows again, placed anew, once it draws again.
                let shown = self.space.element_geometry(&window).is_some();
                if shown && surface == &root && !has_buffer(surface) {
                    self.unmap(&window);
                    self.unplaced.push(window);
                    return;
                }
                self.reshaped(&window);
            }
        }
        self.dirty = true;
        let Some(i) = self
            .unplaced
            .iter()
            .position(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
        else {
            return;
        };
        let Some(toplevel) = self.unplaced[i].toplevel().cloned() else {
            return;
        };
        // The first commit asks for the first configure; the window shows
        // once a buffer comes, at the size it drew.
        if !toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
            return;
        }
        let drawn = has_buffer(surface);
        let Some(area) = self.output_area() else {
            return;
        };
        if drawn {
            let window = self.unplaced.remove(i);
            let insets = self.insets(&window);
            let frame = self.workspace.open(
                window.clone(),
                insets.frame_size(window.geometry().size),
                area,
            );
            let place = insets.window(frame);
            self.put(&window, frame);
            data(&window).borrow_mut().shape = Some((window.geometry().size, server_side(&window)));
            eprintln!(
                "edel-compositor: mapped window {} at {},{} {}x{}",
                title(&window),
                place.loc.x,
                place.loc.y,
                place.size.w,
                place.size.h
            );
            self.focus(&window);
            let maximize = std::mem::take(&mut data(&window).borrow_mut().maximize_when_placed);
            if maximize {
                self.maximize(&window);
            }
            if self.workspace.rearranges() {
                self.relayout();
            }
            self.state_changed();
        }
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

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.unplaced.push(Window::new_wayland_window(surface));
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

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        match self.window_of(surface.wl_surface()) {
            Some(window) if self.space.element_geometry(&window).is_some() => {
                self.maximize(&window);
            }
            Some(window) => {
                data(&window).borrow_mut().maximize_when_placed = true;
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
            None => {}
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_of(surface.wl_surface()) else {
            return;
        };
        data(&window).borrow_mut().maximize_when_placed = false;
        if self.is_maximized(&window) {
            self.unmaximize(&window);
        } else if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn title_changed(&mut self, _surface: ToplevelSurface) {
        // Bars are drawn again when what they show changes.
        self.dirty = true;
        self.state_changed();
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        self.grab_for(&surface, serial, Kind::Move);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        serial: Serial,
        edges: ResizeEdge,
    ) {
        self.grab_for(&surface, serial, Kind::Resize(edges));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.unplaced
            .retain(|w| w.toplevel().is_none_or(|t| t != &surface));
        if let Some(window) = self.window_of(surface.wl_surface()) {
            self.unmap(&window);
        }
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

    /// The window under the pointer set its cursor (`pointer.rs`).
    fn cursor_image(&mut self, _seat: &Seat<Edel>, image: CursorImageStatus) {
        self.cursors.status = image;
        self.dirty = true;
    }
}

impl TabletSeatHandler for Edel {}

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
delegate_xdg_decoration!(Edel);
delegate_fractional_scale!(Edel);
delegate_viewporter!(Edel);
delegate_tablet_manager!(Edel);
delegate_cursor_shape!(Edel);
delegate_seat!(Edel);
delegate_data_device!(Edel);
delegate_output!(Edel);
delegate_presentation!(Edel);
