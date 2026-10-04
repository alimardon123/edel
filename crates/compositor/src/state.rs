//! The compositor's state and the Wayland protocols it serves: surfaces,
//! shared memory, xdg-shell windows, the seat (keyboard and pointer), the
//! clipboard, outputs, presentation time and server-side decorations.
//! The shown workspace's windows live in one smithay `Space`
//! (`workspaces.rs`, M5.2a); its active policy,
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
use smithay::utils::{Clock, Logical, Monotonic, Point, Rectangle, SERIAL_COUNTER, Serial};
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
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
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
    delegate_layer_shell, delegate_output, delegate_presentation, delegate_seat, delegate_shm,
    delegate_tablet_manager, delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
};
use toml::{Table, Value};

use edel::system::MACHINE_FILE;
use edel_compositor::desks::Desks;
use edel_compositor::frame::{Button, Text};
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
    /// The background's colour changed (M5.5c), so every screen's next
    /// frame is drawn whole, not only where something moved.
    pub repaint: bool,
    pub telemetry: Telemetry,
    /// A timer will log the telemetry once the screen is still (`drm.rs`).
    pub report_armed: bool,
    /// A timer will mark the screens dirty for an animation's next step
    /// (`drm.rs`, M5.11b).
    pub animation_armed: bool,
    /// When a screen last drew a frame, so an animation adds no frame of
    /// its own while clients' frames already step it.
    pub last_frame: Option<std::time::Instant>,
    /// A window is being moved or resized (`grabs.rs`). Kept here because
    /// asking the pointer from inside its grab would wait on its own lock.
    pub dragging: bool,
    /// The title font, once its thread has loaded it (`decoration.rs`).
    pub text: Option<Text>,
    /// The cursor and its images (`pointer.rs`).
    pub cursors: Cursors,
    /// The title bar button under the pointer.
    pub hover: Option<(Window, Button)>,
    /// A dock that hides while covered (M5.4f) is shown anyway: the
    /// pointer reached its edge and has not left it since.
    pub dock_shown: bool,
    /// The title bar button the left button went down on.
    pub pressed: Option<(Window, Button)>,
    /// The last click on a title bar, and when, for double clicks.
    pub last_title_click: Option<(Window, u32)>,
    /// The modifier pressed alone while nothing else was, until another
    /// key or a button comes (M5.3b).
    pub tap: Option<smithay::input::keyboard::Keysym>,
    /// The layer that asked for the keyboard alone and was given it when
    /// it showed, such as shell-ui's launcher (M5.3b).
    pub keyboard_layer: Option<WlSurface>,
    /// The window switcher while its keys are held (M5.3c).
    pub switcher: Option<crate::switcher::Switcher>,
    /// What the system file says (`watch.rs`).
    pub settings: Settings,
    /// `[outputs]` changed: the backend scans and places its screens again
    /// (`drm.rs`).
    pub screens_changed: bool,
    /// The effect tier and its frame-deadline monitor (`tiers.rs`).
    pub deadline: edel_compositor::effects::Deadline,
    /// What is opening, closing and sliding (`animate.rs`).
    pub animations: crate::animate::Animations,
    /// The X11 display for X11 apps (`xwayland.rs`).
    pub x11: Option<crate::xwayland::X11Display>,
    /// shell-ui, started and started again (`shellui.rs`, M5.1b).
    pub shell_ui: crate::shellui::ShellUi,
    /// The Wayland socket's name, for programs the compositor starts.
    pub socket: String,
    /// The keyboard shortcuts that act (`shortcuts.rs`, M5.13a).
    pub bindings: Vec<crate::shortcuts::Binding>,
    /// The program the session runs, if it was given one (`program.rs`).
    pub program: Option<crate::program::Program>,
    /// The workspaces and where window frames go on each (M5.2a).
    pub desks: Desks<Window>,
    /// Clients that follow the workspaces (`extworkspace.rs`, M5.2b).
    pub ext_workspaces: crate::extworkspace::Managers,
    /// Clients that follow the windows (`toplevels.rs`, M5.2d); a cell, as
    /// they hear of changes wherever the state file is written.
    pub toplevels: std::cell::RefCell<crate::toplevels::Toplevels>,
    /// shell-ui's private link (`edelshell.rs`, M5.3).
    pub links: crate::edelshell::Links,
    /// Windows whose first buffer has not come yet, so their size is not
    /// known and they are not placed or shown.
    unplaced: Vec<Window>,
    state_file: StateFile,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    /// Panels, docks and backgrounds (`layers.rs`).
    pub layer_shell: WlrLayerShellState,
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
        crate::extworkspace::create_global(&display);
        crate::toplevels::create_global(&display);
        crate::edelshell::create_global(&display);
        Ok(Edel {
            compositor: CompositorState::new::<Edel>(&display),
            // Minimize since the window list brings a window back (M5.2h);
            // fullscreen and the window menu wait for their own steps, and
            // apps that draw their own bars leave out what is missing.
            xdg_shell: XdgShellState::new_with_capabilities::<Edel>(
                &display,
                [WmCapabilities::Maximize, WmCapabilities::Minimize],
            ),
            shm: ShmState::new::<Edel>(&display, Vec::new()),
            layer_shell: WlrLayerShellState::new::<Edel>(&display),
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
            repaint: false,
            telemetry: Telemetry::default(),
            report_armed: false,
            animation_armed: false,
            last_frame: None,
            dragging: false,
            text: None,
            cursors: Cursors::new(),
            hover: None,
            dock_shown: false,
            pressed: None,
            last_title_click: None,
            tap: None,
            keyboard_layer: None,
            switcher: None,
            settings: Settings::default(),
            screens_changed: false,
            deadline: edel_compositor::effects::Deadline::new(edel_compositor::effects::Tier::Lite),
            animations: crate::animate::Animations::new(edel_compositor::animation::style(
                edel_compositor::effects::Tier::Lite,
                edel_compositor::animation::Motion::Full,
            )),
            x11: None,
            shell_ui: Default::default(),
            program: None,
            socket: String::new(),
            bindings: crate::shortcuts::bind(&Default::default()).0,
            desks: Desks::new(Settings::default().workspaces(), tokens.gap),
            ext_workspaces: Default::default(),
            toplevels: Default::default(),
            links: Default::default(),
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
        self.sync_toplevels();
    }

    /// `window` was put at `place` by a person, or back from maximized:
    /// the policy decides where its frame stays (floating keeps it, tiling
    /// puts it back in its tile), and the state file says so.
    pub fn placed(&mut self, window: &Window, place: Rectangle<i32, Logical>) {
        let frame = self.insets(window).frame(place);
        // A floating window put on another screen moves to it; one whose
        // middle is off every screen stays on its own.
        let middle = frame.loc.to_f64() + frame.size.to_f64().downscale(2.0).to_point();
        let on_a_screen = self.space.output_under(middle).next().is_some();
        let found = if on_a_screen {
            self.screen_at(middle)
        } else {
            self.home(window)
        };
        let Some((screen, area)) = found else {
            return;
        };
        let frame = self.desks.layout_mut().moved(window, frame, &screen, area);
        self.put(window, frame);
        self.dirty = true;
        self.state_changed();
    }

    /// Puts `window`'s frame at `frame`: the window hears its new size,
    /// and whether it is tiled, so an app that draws its own frame leaves
    /// out its shadow and rounded corners there.
    /// A window on a hidden workspace stays hidden.
    fn put(&mut self, window: &Window, frame: Rectangle<i32, Logical>) {
        if self.desks.hidden_on(window).is_some() {
            return;
        }
        let place = self.insets(window).window(frame);
        let tiled = self.desks.layout().rearranges();
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
        self.slide(window, place.loc);
        self.space.map_element(window.clone(), place.loc, false);
        self.send_scale(window);
    }

    /// The layout is about to move `window` to `to`: if it is on screen
    /// somewhere else, it slides there (M5.11b).
    pub fn slide(&mut self, window: &Window, to: Point<i32, Logical>) {
        if let Some(from) = self.space.element_location(window) {
            self.animations.moved(window, from, to);
        }
    }

    /// Every window where the active policy puts it, stacked as they
    /// were; maximized windows fill the screen again, and the pointer
    /// stays on the screen.
    pub fn relayout(&mut self) {
        self.keep_pointer_on_screen();
        let areas = self.window_areas();
        if !areas.is_empty() {
            let stack: Vec<Window> = self.space.elements().cloned().collect();
            for (window, frame) in self.desks.layout_mut().arrange(&areas) {
                if self.is_maximized(&window) {
                    self.maximize(&window);
                } else {
                    self.put(&window, frame);
                }
            }
            for window in &stack {
                self.space.raise_element(window, false);
            }
        }
        self.dirty = true;
        self.state_changed();
    }

    /// `shell.tiling` or the preset changed: `name` is every workspace's
    /// policy, and the shown one's windows are laid out again.
    pub fn switch_policy(&mut self, name: &str) {
        let shown = self.desks.layout().name();
        for layout in self.desks.layouts_mut() {
            layout.switch(name);
        }
        if shown != name {
            eprintln!("edel-compositor: windows now {name}");
            self.relayout();
        }
    }

    /// Super+T: the other policy, for this workspace only; the system file
    /// is left as it is.
    pub fn toggle_tiling(&mut self) {
        let next = self.desks.layout().next();
        if self.desks.layout_mut().switch(next) {
            eprintln!("edel-compositor: windows now {next}");
            self.relayout();
        }
    }

    /// Reads both system files again and applies what changed.
    pub fn reload_settings(&mut self) {
        let person = settings::person_file();
        let (new, notes) = settings::load(std::path::Path::new(MACHINE_FILE), person.as_deref());
        for note in notes {
            eprintln!("edel-compositor: {note}");
        }
        let old = std::mem::replace(&mut self.settings, new.clone());
        let (bindings, notes) = crate::shortcuts::bind(&new.shortcuts);
        for line in notes
            .iter()
            .chain(&crate::shortcuts::changes(&self.bindings, &bindings))
        {
            eprintln!("edel-compositor: {line}");
        }
        self.bindings = bindings;
        if old.outputs != new.outputs {
            self.screens_changed = true;
        }
        self.restyle();
        let rescaled = self.apply_scales();
        if self.desks.count() != new.workspaces() {
            self.set_workspace_count(new.workspaces());
        }
        if old.color_scheme != new.color_scheme {
            // Light or dark (M5.5c): the tokens' other colours, the title
            // bars drawn again with them, and shell-ui started again.
            let (tokens, notes) = edel::tokens::load(new.color_scheme);
            for note in notes {
                eprintln!("edel-compositor: {}: {note}", edel::tokens::PATH);
            }
            self.tokens = tokens;
            self.repaint = true;
            self.dirty = true;
            eprintln!("edel-compositor: colour scheme {}", new.color_scheme.name());
        }
        if old.button_side() != new.button_side() {
            eprintln!(
                "edel-compositor: window buttons on the {}",
                new.button_side().name()
            );
            self.dirty = true;
        }
        if old.preset_differs(&new) {
            let name = new.preset.as_deref().unwrap_or(edel::presets::DEFAULT);
            crate::shellui::restart(self, &format!("the preset is now {name}"));
        } else if old.panels_differ(&new) {
            crate::shellui::restart(self, "the panels changed");
        } else if old.color_scheme != new.color_scheme {
            let name = new.color_scheme.name();
            crate::shellui::restart(self, &format!("the colour scheme is now {name}"));
        }
        if old.policy() != new.policy() {
            self.switch_policy(new.policy());
        } else if old.title_bars != new.title_bars || rescaled {
            self.relayout();
        }
    }

    /// Whether the compositor draws title bars under the active policy.
    pub fn bars_shown(&self) -> bool {
        self.settings.bars_in(self.desks.layout().name())
    }

    /// `window` leaves the screen, closed or hidden: the policy forgets its
    /// place and the keyboard goes to the window now on top.
    fn unmap(&mut self, window: &Window) {
        eprintln!("edel-compositor: unmapped window {}", title(window));
        self.forget(window);
        self.animations.forget(window);
        let mut frame = data(window).borrow_mut();
        frame.restore = None;
        frame.shape = None;
        drop(frame);
        // The keyboard moves only if this window had it, or nothing has
        // it: never away from a layer such as the launcher.
        let had_keyboard = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .is_none_or(|focus| window.toplevel().is_some_and(|t| t.wl_surface() == &focus));
        self.desks.close(window);
        self.space.unmap_elem(window);
        if had_keyboard {
            if let Some(top) = self.space.elements().last().cloned() {
                self.focus(&top);
            }
        }
        if self.desks.layout().rearranges() {
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
        let frame = self.insets(window).frame(place);
        if let Some((screen, area)) = self.home(window) {
            self.desks.layout_mut().moved(window, frame, &screen, area);
        }
        self.state_changed();
    }

    /// The outputs changed: the policy fits every frame to the new area,
    /// and maximized windows fill it.
    pub fn outputs_changed(&mut self) {
        self.screens_changed_for_layers();
        self.relayout();
        self.announce_workspaces();
    }

    /// Sends the state file what is on screen now. Never during a drag:
    /// the grab writes it once, when it ends.
    pub fn state_changed(&self) {
        if self.dragging {
            return;
        }
        self.state_file.send(self.state_toml());
        self.sync_toplevels();
        self.sync_shell();
    }

    /// Outputs and windows, bottom of the stack first, as TOML: the shown
    /// workspace's windows, then the hidden ones by workspace, each with
    /// its workspace, counted from 1 as Super+1 to Super+9 are, and
    /// whether it is minimized (M5.2h).
    fn state_toml(&self) -> String {
        let mut table = Table::new();
        table.insert("format".into(), Value::Integer(statefile::FORMAT));
        table.insert(
            "policy".into(),
            Value::String(self.desks.layout().name().into()),
        );
        let shown = self.desks.active();
        table.insert("workspace".into(), Value::Integer(shown as i64 + 1));
        table.insert(
            "workspaces".into(),
            Value::Integer(self.desks.count() as i64),
        );
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
        table.insert(
            "tier".into(),
            Value::String(self.deadline.tier().name().into()),
        );
        table.insert("outputs".into(), Value::Array(outputs));
        let focused = self.seat.get_keyboard().and_then(|k| k.current_focus());
        let hidden = self
            .desks
            .hidden()
            .map(|(desk, window, frame)| (desk, window, self.insets(window).window(frame)));
        let windows = self
            .space
            .elements()
            .filter_map(|window| Some((shown, window, self.space.element_geometry(window)?)))
            .chain(hidden)
            .filter_map(|(desk, window, place)| {
                let toplevel = window.toplevel()?;
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
                t.insert("workspace".into(), Value::Integer(desk as i64 + 1));
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
                t.insert(
                    "minimized".into(),
                    Value::Boolean(self.desks.minimized(window).is_some()),
                );
                Some(Value::Table(t))
            })
            .collect();
        table.insert("windows".into(), Value::Array(windows));
        table.insert("layers".into(), Value::Array(self.layers_toml()));
        toml::to_string(&table).unwrap_or_default()
    }

    /// The window `surface` is, shown, on a hidden workspace or not yet
    /// placed.
    pub fn window_of(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .chain(&self.unplaced)
            .chain(self.desks.hidden().map(|(_, window, _)| window))
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
pub fn has_buffer(surface: &WlSurface) -> bool {
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
            if self.layer_commit(&root) {
                return;
            }
            if let Some(window) = self.window_of(&root) {
                window.on_commit();
                // New subsurfaces and popups hear the scale too.
                self.send_scale(&window);
                // A window that drops its buffer hides itself; it shows
                // again, placed anew on the shown workspace, once it draws
                // again.
                if surface == &root && !has_buffer(surface) {
                    if self.space.element_geometry(&window).is_some() {
                        self.unmap(&window);
                        self.unplaced.push(window);
                        return;
                    }
                    if self.desks.hidden_on(&window).is_some() {
                        self.forget_hidden(&window);
                        self.unplaced.push(window);
                        return;
                    }
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
        // On the screen the pointer is on (M5.2g).
        let Some((screen, area)) = self.pointer_screen() else {
            return;
        };
        if drawn {
            let window = self.unplaced.remove(i);
            let insets = self.insets(&window);
            let frame = self.desks.layout_mut().open(
                window.clone(),
                insets.frame_size(window.geometry().size),
                &screen,
                area,
            );
            let place = insets.window(frame);
            self.put(&window, frame);
            self.animations.opened(&window);
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
            if self.desks.layout().rearranges() {
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

    /// A window that draws its own bar asks to be minimized, as ours does
    /// from its button.
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_of(surface.wl_surface()) {
            if self.space.element_geometry(&window).is_some() {
                self.minimize(&window);
            }
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
        let Some(window) = self.window_of(surface.wl_surface()) else {
            return;
        };
        if self.desks.hidden_on(&window).is_some() {
            self.forget_hidden(&window);
        } else {
            self.snapshot_closing(&window);
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

impl OutputHandler for Edel {
    /// A client's new `wl_output` joins the workspace group it sees.
    fn output_bound(
        &mut self,
        _output: smithay::output::Output,
        _wl_output: smithay::reexports::wayland_server::protocol::wl_output::WlOutput,
    ) {
        self.announce_workspaces();
        self.sync_toplevels();
    }
}

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
delegate_layer_shell!(Edel);
