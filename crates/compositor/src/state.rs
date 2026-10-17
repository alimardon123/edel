//! The compositor's state and the Wayland protocols it serves: surfaces,
//! shared memory, xdg-shell windows, the seat (keyboard and pointer), the
//! clipboard, outputs, presentation time and server-side decorations.
//! The shown workspace's windows live in one smithay `Space`
//! (`workspaces.rs`, M5.2a); its active policy,
//! floating (M4.3) or tiling (M4.5), decides where each frame goes, the
//! window sits inside its frame (M4.4), the settings from the settings file
//! apply live (M4.5), and every change of what is on screen is written to
//! the state file.

use std::sync::Arc;

use anyhow::{Context, Result};
use edel_compositor::layout::{Direction, Gaps};
use smithay::desktop::{PopupManager, Space, Window, find_popup_root_surface};
use smithay::input::pointer::{CursorImageStatus, Focus};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::Output;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{
    Interest, LoopHandle, LoopSignal, Mode as TriggerMode, PostAction,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::{
    ResizeEdge, State, WmCapabilities,
};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
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
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as KdeDefaultMode;
use smithay::wayland::shell::kde::decoration::KdeDecorationState;
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
    delegate_kde_decoration,
    delegate_layer_shell, delegate_output, delegate_presentation, delegate_seat, delegate_shm,
    delegate_tablet_manager, delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
};
use toml::{Table, Value};

use edel_compositor::desks::Desks;
use edel_compositor::frame::{Button, Insets, Text};
use edel_compositor::settings::{self, Settings};
use edel_compositor::telemetry::Telemetry;
use edel_compositor::tokens::Tokens;

use crate::decoration::{app_id, data, server_side, title};
use crate::grabs::{Kind, WindowGrab};
use crate::pointer::Cursors;
use crate::statefile::{self, StateFile};

pub struct Edel {
    pub display: DisplayHandle,
    /// Apps' GPU buffers (M5.19), offered by the DRM backend.
    pub dmabuf: crate::dmabuf::Dmabufs,
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
    /// Counts each title font that loads or changes size (M5.12b5); the
    /// bars draw again when it moves.
    pub text_key: u32,
    /// The apps' icons for the title bars (M5.6a).
    pub app_icons: crate::decoration::AppIcons,
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
    /// A touchpad swipe under way: its fingers, how far it slid, and the
    /// workspaces three of them hold (M5.2j-b4, M5.14).
    pub swipe: Option<crate::workspaces::Swipe>,
    /// `EDEL_LOG_KEYS=1`: each key's code, press or release and the
    /// modifiers held are logged, for CI's desktop test alone; a session
    /// never sets it, as a log of keys holds what people type.
    pub log_keys: bool,
    /// The layer that asked for the keyboard alone and was given it when
    /// it showed, such as shell-ui's launcher (M5.3b).
    pub keyboard_layer: Option<WlSurface>,
    /// The window switcher while its keys are held (M5.3c).
    pub switcher: Option<crate::switcher::Switcher>,
    /// The overview while it is open (M5.2j).
    pub overview: Option<crate::glance::Overview>,
    /// What the overview keeps between openings (its backdrops).
    pub overview_kept: crate::glance::Kept,
    /// Each window's shadow, kept with what it shows and its margin in
    /// pixels, painted again only when its size or the shadow changes
    /// (`render.rs`, M5.12b3).
    /// The rounded corners' programs, once the renderer made them; none
    /// keeps the corners square (M5.14).
    pub rounded: Option<crate::rounded::Rounded>,
    /// The live blur behind popups, its programs and pictures (M5.14, `backdrop.rs`).
    pub backdrops: crate::backdrop::Backdrops,
    pub window_shadows: std::collections::HashMap<Window, (crate::glance::Painted, i32)>,
    /// What the settings file says (`watch.rs`).
    pub settings: Settings,
    /// The wallpaper and its picture for each screen (M5.10c4b).
    pub wallpaper: crate::wallpaper::Shown,
    /// `[displays]` changed: the backend scans and places its screens again
    /// (`drm.rs`).
    pub screens_changed: bool,
    /// The effect tier and its frame-deadline monitor (`tiers.rs`).
    pub deadline: edel_compositor::effects::Deadline,
    /// Game mode (M7.7, `games.rs`).
    pub game: crate::games::Game,
    /// What is opening, closing and sliding (`animate.rs`).
    pub animations: crate::animate::Animations,
    /// The X11 display for X11 apps (`xwayland.rs`).
    pub x11: Option<crate::xwayland::X11Display>,
    /// shell-ui, started and started again (`shellui.rs`, M5.1b).
    pub shell_ui: crate::shellui::ShellUi,
    /// The programs the preset's `[session] start` lists (`session.rs`, M5.7b).
    pub services: crate::session::Services,
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
    /// ext-session-lock-v1 and the lock while the session is locked (M5.10).
    pub session_lock: smithay::wayland::session_lock::SessionLockManagerState,
    pub locked: Option<crate::lock::Locked>,
    shm: ShmState,
    seat_state: SeatState<Edel>,
    data_device: DataDeviceState,
    _decorations: XdgDecorationState,
    /// GTK's way to ask for our title bar (`decoration.rs`, M5.6a).
    pub kde_decorations: KdeDecorationState,
    _fractional_scale: FractionalScaleManagerState,
    _viewporter: ViewporterState,
    _tablets: TabletManagerState,
    _cursor_shapes: CursorShapeManagerState,
    _outputs: OutputManagerState,
    _presentation: PresentationState,
    _security_context: smithay::wayland::security_context::SecurityContextState,
    /// The event loop's handle, for the sockets sandboxes ask for (M5.22).
    pub handle: Option<LoopHandle<'static, Edel>>,
    /// The protocols everyday apps expect (`everyday.rs`, M5.23).
    pub primary_selection: smithay::wayland::selection::primary_selection::PrimarySelectionState,
    /// ext-data-control-v1, for shell-ui's clipboard history (M7.13).
    pub data_control: smithay::wayland::selection::ext_data_control::DataControlState,
    pub activation: smithay::wayland::xdg_activation::XdgActivationState,
    /// The surfaces that asked to keep the screen on.
    pub idle_inhibitors: Vec<WlSurface>,
    /// ext-idle-notify-v1: the clients told when the person is idle
    /// (`everyday.rs`, M7.8).
    pub idle_notifier: smithay::wayland::idle_notify::IdleNotifierState<Edel>,
    _pointer_constraints: smithay::wayland::pointer_constraints::PointerConstraintsState,
    _relative_pointer: smithay::wayland::relative_pointer::RelativePointerManagerState,
    _idle_inhibit: smithay::wayland::idle_inhibit::IdleInhibitManagerState,
    /// The frames clients asked to copy the screen into, until their
    /// buffers are filled (`screencopy.rs`, M6.3).
    pub pending_copies: Vec<crate::screencopy::Pending>,
    /// The clients whose first copy has been logged, once each.
    pub copy_clients: Vec<smithay::reexports::wayland_server::backend::ClientId>,
    /// The keyboard shortcuts inhibit global (M5.13b, `everyday.rs`).
    pub keyboard_shortcuts_inhibit:
        smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState,
    /// The text input, input method and virtual keyboard globals (`ime.rs`, M6.8).
    _text_input: smithay::wayland::text_input::TextInputManagerState,
    _input_method: smithay::wayland::input_method::InputMethodManagerState,
    _virtual_keyboard: smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState,
}

impl Edel {
    pub fn new(
        display: DisplayHandle,
        signal: LoopSignal,
        loop_handle: LoopHandle<'static, Edel>,
        tokens: Tokens,
    ) -> Result<Edel> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display, "seat0");
        seat.add_keyboard(Default::default(), 600, 25)?;
        seat.add_pointer();
        crate::extworkspace::create_global(&display);
        crate::toplevels::create_global(&display);
        crate::edelshell::create_global(&display);
        crate::screencopy::create_global(&display);
        crate::games::create_global(&display);
        crate::backdrop::create_global(&display);
        let primary_selection =
            smithay::wayland::selection::primary_selection::PrimarySelectionState::new::<Edel>(
                &display,
            );
        let data_control =
            smithay::wayland::selection::ext_data_control::DataControlState::new::<Edel, _>(
                &display,
                Some(&primary_selection),
                crate::sandbox::shell_may,
            );
        Ok(Edel {
            dmabuf: crate::dmabuf::Dmabufs::default(),
            compositor: CompositorState::new::<Edel>(&display),
            // Minimize since the window list brings a window back (M5.2h);
            // fullscreen and the window menu wait for their own steps, and
            // apps that draw their own bars leave out what is missing.
            xdg_shell: XdgShellState::new_with_capabilities::<Edel>(
                &display,
                [WmCapabilities::Maximize, WmCapabilities::Minimize],
            ),
            shm: ShmState::new::<Edel>(&display, Vec::new()),
            // Panels and docks, but not sandboxed apps (M5.22).
            layer_shell: WlrLayerShellState::new_with_filter::<Edel, _>(
                &display,
                crate::sandbox::shell_may,
            ),
            // The lock screen, the shell's alone (M5.10).
            session_lock: smithay::wayland::session_lock::SessionLockManagerState::new::<Edel, _>(
                &display,
                crate::sandbox::shell_may,
            ),
            locked: None,
            _security_context: smithay::wayland::security_context::SecurityContextState::new::<
                Edel,
                _,
            >(&display, crate::sandbox::shell_may),
            handle: None,
            primary_selection,
            // Clipboard history (M7.13), hidden from sandboxed apps.
            data_control,
            activation: smithay::wayland::xdg_activation::XdgActivationState::new::<Edel>(&display),
            idle_inhibitors: Vec::new(),
            idle_notifier: smithay::wayland::idle_notify::IdleNotifierState::new(
                &display,
                loop_handle,
            ),
            _pointer_constraints:
                smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Edel>(
                    &display,
                ),
            _relative_pointer: smithay::wayland::relative_pointer::RelativePointerManagerState::new::<
                Edel,
            >(&display),
            _idle_inhibit: smithay::wayland::idle_inhibit::IdleInhibitManagerState::new::<Edel>(
                &display,
            ),
            pending_copies: Vec::new(),
            copy_clients: Vec::new(),
            keyboard_shortcuts_inhibit:
                smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState::new::<
                    Edel,
                >(&display),
            _text_input: smithay::wayland::text_input::TextInputManagerState::new::<Edel>(&display),
            // Hidden from sandboxed apps: a Flatpak app never becomes the input method (M6.8).
            _input_method: smithay::wayland::input_method::InputMethodManagerState::new::<Edel, _>(
                &display,
                crate::sandbox::shell_may,
            ),
            // fcitx5 speaks input-method-v2 only beside a virtual keyboard,
            // through which it gives back the keys it does not take (M6.8);
            // hidden from sandboxed apps as the input method is.
            _virtual_keyboard: smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState::new::<
                Edel,
                _,
            >(&display, crate::sandbox::shell_may),
            data_device: DataDeviceState::new::<Edel>(&display),
            _decorations: XdgDecorationState::new::<Edel>(&display),
            // Server: GTK windows without a header bar of their own take
            // ours, with the buttons where the person put them.
            kde_decorations: KdeDecorationState::new::<Edel>(&display, KdeDefaultMode::Server),
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
            text_key: 0,
            cursors: Cursors::new(),
            hover: None,
            dock_shown: false,
            pressed: None,
            last_title_click: None,
            tap: None,
            swipe: None,
            log_keys: std::env::var_os("EDEL_LOG_KEYS").is_some_and(|v| v == "1"),
            keyboard_layer: None,
            switcher: None,
            overview: None,
            overview_kept: Default::default(),
            rounded: None,
            backdrops: Default::default(),
            window_shadows: std::collections::HashMap::new(),
            settings: Settings::default(),
            wallpaper: crate::wallpaper::Shown::new(edel::wallpapers::on_this_machine(None, false)),
            app_icons: crate::decoration::AppIcons::new(),
            screens_changed: false,
            deadline: edel_compositor::effects::Deadline::new(edel_compositor::effects::Tier::Lite),
            game: Default::default(),
            animations: crate::animate::Animations::new(edel_compositor::animation::style(
                edel_compositor::effects::Tier::Lite,
                edel_compositor::animation::Motion::Full,
                edel::settings::ANIMATION_SPEED_DEFAULT,
                edel_compositor::animation::Curve::Smooth,
            )),
            x11: None,
            shell_ui: Default::default(),
            services: Default::default(),
            program: None,
            socket: String::new(),
            bindings: crate::shortcuts::bind(&Default::default()).0,
            desks: Desks::new(Settings::default().workspaces(), Gaps::from_tokens(&tokens)),
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
        // Locked, the keyboard stays with the lock screen (M5.10).
        if self.is_locked() {
            self.space.raise_element(window, true);
            return;
        }
        // The scroll style brings the window's column into view.
        let scrolled = self.desks.layout_of_mut(window).focused(window);
        self.space.raise_element(window, true);
        let surface = window.toplevel().map(|t| t.wl_surface().clone());
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
        }
        self.dirty = true;
        self.sync_toplevels();
        if scrolled {
            self.relayout();
        }
    }

    /// Super+R: the focused column's next width, in the scroll style.
    pub fn widen_focused(&mut self) {
        if let Some(window) = self.focused_window() {
            if self.desks.layout_of_mut(&window).widen(&window) {
                self.relayout();
            }
        }
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
        let frame = self
            .desks
            .layout_of_mut(window)
            .moved(window, frame, &screen, area);
        self.put(window, frame);
        // With workspaces on each screen, put on another screen, it joins
        // the workspace shown there (M5.2k).
        if self.desks.per_screen() {
            self.settle_screens();
            self.note_screen();
            self.relayout();
        }
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
        let tiled = self.desks.layout_of(window).rearranges();
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
            for (window, frame) in self.desks.arrange(&areas) {
                if self.is_fullscreen(&window) {
                    self.fullscreen(&window, None);
                } else if self.is_maximized(&window) {
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

    /// `layout.tiling` or the preset changed: `name` is every workspace's
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

    /// `window` comes forward, as a click in a window list or an app's
    /// activation (M5.23) asks: its workspace first, then the window, and
    /// a minimized window comes back.
    pub fn bring_forward(&mut self, window: &Window) {
        if !self.restore(window) {
            if let Some(desk) = self.desks.hidden_on(window) {
                self.switch_for(desk, window);
            }
            if self.space.element_geometry(window).is_some() {
                self.focus(window);
                self.state_changed();
            }
        }
    }

    /// The focused window and the one shown `way` from it (M5.16a).
    fn toward(&self, way: Direction) -> Option<(Window, Window)> {
        let focused = self.focused_window()?;
        let from = self.space.element_geometry(&focused)?;
        let others: Vec<(Window, Rectangle<i32, Logical>)> = self
            .space
            .elements()
            .filter(|w| **w != focused)
            .filter_map(|w| Some((w.clone(), self.space.element_geometry(w)?)))
            .collect();
        let next = edel_compositor::layout::toward(from, &others, way)?;
        Some((focused, next))
    }

    /// Super with an arrow: the keyboard goes to the window that way.
    pub fn focus_toward(&mut self, way: Direction) {
        if let Some((_, next)) = self.toward(way) {
            self.focus(&next);
        }
    }

    /// Super+Shift with an arrow: the focused window trades places with
    /// the one that way, when the workspace tiles.
    pub fn swap_toward(&mut self, way: Direction) {
        let Some((focused, next)) = self.toward(way) else {
            return;
        };
        if self.desks.layout_mut().swap(&focused, &next) {
            eprintln!(
                "edel-compositor: swapped {} and {}",
                title(&focused),
                title(&next)
            );
            self.relayout();
        }
    }

    /// Super+Y: the other policy, for this workspace only; the settings file
    /// is left as it is.
    pub fn toggle_tiling(&mut self) {
        let next = self.desks.layout().next();
        if self.desks.layout_mut().switch(next) {
            eprintln!("edel-compositor: windows now {next}");
            self.relayout();
        }
    }

    /// Reads both settings files again and applies what changed.
    pub fn reload_settings(&mut self) {
        let person = settings::person_file();
        let (new, notes) = settings::load(&settings::machine_file(), person.as_deref());
        for note in notes {
            eprintln!("edel-compositor: {note}");
        }
        let old = std::mem::replace(&mut self.settings, new.clone());
        self.cursor_settings(new.cursor_theme.as_deref(), new.cursor_size);
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
        if old.keyboard != new.keyboard {
            self.apply_keyboard();
        }
        self.follow_game_mode(new.game_mode);
        self.restyle();
        let rescaled = self.apply_scales();
        if old.workspace_names != new.workspace_names {
            self.desks.set_names(new.names());
            self.announce_workspaces();
            self.state_changed();
        }
        if old.per_screen() != new.per_screen() {
            self.set_per_screen(new.per_screen());
        }
        if new.dynamic() {
            self.settle_dynamic();
        } else if self.desks.count() != new.workspaces() {
            self.set_workspace_count(new.workspaces());
        }
        // The tokens follow the scheme, the person's accent and colours and
        // a wallpaper's accent (M5.12b1), read as every reader on the
        // machine reads them. A change that leaves the tokens as they were
        // does nothing.
        let colours_changed = new.colours_changed(&old);
        // The title font and its size (M5.12b5), as they were.
        let old_text = (self.tokens.font.clone(), self.tokens.title_text_size);
        // The shape (M5.12b2b) is read with the tokens, and the windows'
        // frames are measured before the tokens change.
        let old_sizes = sizes_of(&self.tokens);
        // The windows' depth (M5.12b3) is logged when it changes.
        let old_depth = depth_of(&self.tokens);
        let old_insets: Vec<(Window, Insets)> = self
            .space
            .elements()
            .map(|window| (window.clone(), self.insets(window)))
            .collect();
        // High contrast (M6.9) is a change of the tokens, logged for CI.
        if old.contrast != new.contrast {
            let word = if new.contrast { "high" } else { "normal" };
            eprintln!("edel-compositor: contrast {word}");
        }
        if old.color_scheme != new.color_scheme || colours_changed {
            let (tokens, notes) = edel::tokens::on_this_machine(new.color_scheme);
            for note in notes {
                eprintln!("edel-compositor: {}: {note}", edel::tokens::PATH);
            }
            if tokens != self.tokens {
                // Light or dark (M5.5c): the tokens' other colours and the
                // title bars drawn again with them; shell-ui reads the tokens
                // itself on the `settings` event (M6.17). The accent is
                // logged unless only the windows' opacity changed (M5.12b3).
                let mut seen = tokens.clone();
                seen.window_opacity = self.tokens.window_opacity;
                seen.window_opacity_unfocused = self.tokens.window_opacity_unfocused;
                let more = seen != self.tokens;
                self.tokens = tokens;
                self.repaint = true;
                self.dirty = true;
                if more {
                    eprintln!("edel-compositor: accent {}", self.tokens.accent.hex());
                }
            }
            if old.color_scheme != new.color_scheme {
                eprintln!("edel-compositor: colour scheme {}", new.color_scheme.name());
            }
        }
        if sizes_of(&self.tokens) != old_sizes {
            self.shape_changed(old_insets);
        }
        if depth_of(&self.tokens) != old_depth {
            log_depth(&self.tokens);
        }
        if old.text != new.text
            || old_text != (self.tokens.font.clone(), self.tokens.title_text_size)
        {
            self.text_changed(old_text);
        }
        // The wallpaper (M5.10c4b): at start, and when it or the scheme (a
        // dark wallpaper's picture) changes, or the dark one (M5.10c4d), or
        // a workspace's own (M5.12b5).
        if !self.wallpaper.announced
            || old.wallpaper != new.wallpaper
            || old.wallpaper_dark != new.wallpaper_dark
            || old.color_scheme != new.color_scheme
            || old.workspace_wallpapers != new.workspace_wallpapers
        {
            self.apply_wallpaper(&new);
        }
        if old.buttons != new.buttons {
            eprintln!("edel-compositor: title bar buttons {}", new.buttons.names());
            self.dirty = true;
        }
        if old.button_side() != new.button_side() {
            eprintln!(
                "edel-compositor: window buttons on the {}",
                new.button_side().name()
            );
            self.dirty = true;
        }
        if old.tiling_style != new.tiling_style {
            eprintln!("edel-compositor: tiling style {}", new.tiling_style.name());
            self.desks.set_style(new.tiling_style);
            if self.desks.layout().rearranges() && old.policy() == new.policy() {
                self.relayout();
            }
        }
        if old.policy() != new.policy() {
            self.switch_policy(new.policy());
        } else if old.title_bars != new.title_bars || rescaled {
            self.relayout();
        }
        // The input method starts or stops as region.input_method says (M6.8).
        if old.input_method != new.input_method {
            if let Some(handle) = self.handle.clone() {
                crate::session::follow_input_method(&handle, self);
            }
        }
        // Shell-ui reads the files again too (M5.9h), as the tray's list.
        self.tell_settings();
    }

    /// The text changed (M5.12b5): the title bars load the family and size
    /// the tokens name, and draw again when they arrive. A family this system
    /// has no font for keeps the one the bars had, logged, as the release's
    /// fonts are the fallback at start.
    fn text_changed(&mut self, was: (String, u32)) {
        let (family, size) = was;
        if self.tokens.font != family
            && edel_compositor::frame::font_file(
                &self.tokens.font,
                &edel_compositor::frame::font_dirs(),
            )
            .is_none()
        {
            eprintln!(
                "edel-compositor: no font called {:?}; title bars keep {family:?}",
                self.tokens.font
            );
            self.tokens.font = family.clone();
        }
        if (self.tokens.font.clone(), self.tokens.title_text_size) == (family, size) {
            return;
        }
        eprintln!(
            "edel-compositor: text {} at {} px",
            self.tokens.font, self.tokens.title_text_size
        );
        self.text_key += 1;
        self.dirty = true;
        if let Some(handle) = self.handle.clone() {
            crate::decoration::load_text(&handle, &self.tokens.font, self.tokens.title_text_size);
        }
    }

    /// The shape changed (M5.12b2b): the gaps go to every workspace, and
    /// each window's frame takes its new bar and border. A floating window
    /// keeps its size and place, a saved frame (the one before maximized or
    /// fullscreen) keeps its window's size too, and then every window is
    /// laid out again, tiles with the new gaps.
    fn shape_changed(&mut self, old_insets: Vec<(Window, Insets)>) {
        let (gaps, _, _) = sizes_of(&self.tokens);
        log_shape(&self.tokens);
        self.desks.set_gaps(gaps);
        for (window, was) in old_insets {
            let insets = self.insets(&window);
            if insets == was {
                continue;
            }
            let mut frame = data(&window).borrow_mut();
            frame.restore = frame.restore.map(|r| insets.frame(was.window(r)));
            frame.before_fullscreen = frame.before_fullscreen.map(|r| insets.frame(was.window(r)));
            drop(frame);
            if self.is_maximized(&window) || self.is_fullscreen(&window) {
                continue;
            }
            if let Some(place) = self.space.element_geometry(&window) {
                self.placed(&window, place);
            }
        }
        self.relayout();
    }

    /// Whether the compositor draws title bars round `window` under its
    /// workspace's policy.
    pub fn bars_shown(&self, window: &Window) -> bool {
        self.settings.bars_in(self.desks.layout_of(window).name())
    }

    /// `window` leaves the screen, closed or hidden: the policy forgets its
    /// place and the keyboard goes to the window now on top.
    fn unmap(&mut self, window: &Window) {
        eprintln!("edel-compositor: unmapped window {}", title(window));
        self.forget(window);
        self.animations.forget(window);
        self.window_shadows.remove(window);
        let mut frame = data(window).borrow_mut();
        frame.restore = None;
        frame.before_fullscreen = None;
        frame.shape = None;
        drop(frame);
        // The keyboard moves only if this window had it, or nothing has
        // it: never away from a layer such as the launcher.
        let had_keyboard = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .is_none_or(|focus| window.toplevel().is_some_and(|t| t.wl_surface() == &focus));
        let tiled = self.desks.layout_of(window).rearranges();
        self.desks.close(window);
        self.space.unmap_elem(window);
        if had_keyboard {
            if let Some(top) = self.space.elements().last().cloned() {
                self.focus(&top);
            }
        }
        if tiled {
            self.relayout();
        }
        self.settle_dynamic();
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
        // A fullscreen window's place is its screen's, not the policy's.
        if self.is_fullscreen(window) {
            self.state_changed();
            return;
        }
        let frame = self.insets(window).frame(place);
        if let Some((screen, area)) = self.home(window) {
            self.desks
                .layout_of_mut(window)
                .moved(window, frame, &screen, area);
        }
        self.state_changed();
    }

    /// The outputs changed: the policy fits every frame to the new area,
    /// and maximized windows fill it.
    pub fn outputs_changed(&mut self) {
        self.screens_changed_for_layers();
        self.settle_screens();
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
    /// whether it is minimized (M5.2h). The top level says which workspace
    /// is shown, how many there are and each one's name, empty for none
    /// (M5.2i).
    fn state_toml(&self) -> String {
        let mut table = Table::new();
        table.insert("format".into(), Value::Integer(statefile::FORMAT));
        table.insert(
            "policy".into(),
            Value::String(self.desks.layout().name().into()),
        );
        let shown = self.desks.active();
        // The workspace in use: the pointer's screen's, with workspaces on
        // each screen (M5.2k); each screen's own is in its table.
        table.insert("workspace".into(), Value::Integer(shown as i64 + 1));
        // Whether game mode is on (M7.7).
        table.insert("game_mode".into(), Value::Boolean(self.game.on()));
        table.insert(
            "workspaces".into(),
            Value::Integer(self.desks.count() as i64),
        );
        // Each workspace's name, empty for none (M5.2i), in the same order.
        table.insert(
            "workspace_names".into(),
            Value::Array(self.desks.names().into_iter().map(Value::String).collect()),
        );
        let outputs = self
            .space
            .outputs()
            .filter_map(|output| {
                let area = self.space.output_geometry(output)?;
                let mut t = Table::new();
                t.insert("name".into(), Value::String(output.name()));
                let desk = self.desks.shown_on(&output.name());
                t.insert("workspace".into(), Value::Integer(desk as i64 + 1));
                t.insert(
                    "scale".into(),
                    Value::Float(output.current_scale().fractional_scale()),
                );
                insert_rect(&mut t, area);
                // What Settings' Displays page offers (M5.7a): the mode in
                // use, every size the screen has, its preferred one and the
                // scale it gets when the settings file names none.
                let text = |size: smithay::utils::Size<i32, smithay::utils::Physical>| {
                    format!("{}x{}", size.w, size.h)
                };
                if let Some(mode) = output.current_mode() {
                    t.insert("mode".into(), Value::String(text(mode.size)));
                }
                if let Some(mode) = output.preferred_mode() {
                    t.insert("preferred_mode".into(), Value::String(text(mode.size)));
                }
                let modes = edel_compositor::layout::mode_sizes(
                    output.modes().iter().map(|m| (m.size.w, m.size.h)),
                );
                t.insert(
                    "modes".into(),
                    Value::Array(modes.into_iter().map(Value::String).collect()),
                );
                t.insert(
                    "auto_scale".into(),
                    Value::Float(crate::outputs::auto_scale(output)),
                );
                Some(Value::Table(t))
            })
            .collect();
        table.insert(
            "tier".into(),
            Value::String(self.deadline.tier().name().into()),
        );
        // Whether the lock screen is up (M5.10).
        table.insert("locked".into(), Value::Boolean(self.is_locked()));
        // How many surfaces keep the screen on (M5.23).
        table.insert(
            "idle_inhibitors".into(),
            Value::Integer(self.idle_inhibited() as i64),
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
            .filter_map(|window| {
                let desk = self.desks.desk_of(window).unwrap_or(shown);
                Some((desk, window, self.space.element_geometry(window)?))
            })
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
                    "fullscreen".into(),
                    Value::Boolean(self.is_fullscreen(window)),
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
        // A fullscreen window stays where it is.
        if !asked
            || self.is_fullscreen(&window)
            || (kind != Kind::Move && self.is_maximized(&window))
        {
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

/// Per client: the compositor's bookkeeping for its surfaces, and the
/// sandbox it came from, if one marked it (M5.22, `sandbox.rs`).
#[derive(Default)]
pub struct ClientState {
    pub compositor: CompositorClientState,
    pub security_context: Option<smithay::wayland::security_context::SecurityContext>,
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
            data(&window).borrow_mut().shape = Some((window.geometry().size, server_side(&window)));
            let insets = self.insets(&window);
            let size = insets.frame_size(window.geometry().size);
            // An app with a rule (`workspaces.apps`, M5.2l) opens on
            // its workspace; that workspace not shown, the window waits there,
            // unmapped, and the person stays where they are.
            let ruled = self
                .settings
                .app_workspace(&app_id(&window))
                .and_then(|desk| {
                    let opened = self
                        .desks
                        .open_on(desk, window.clone(), size, &screen, area)?;
                    Some((desk, opened))
                });
            let frame = match ruled {
                Some((desk, (_, false))) => {
                    eprintln!(
                        "edel-compositor: window {} opened on workspace {} (workspaces.apps)",
                        title(&window),
                        desk + 1
                    );
                    self.settle_dynamic();
                    self.announce_workspaces();
                    self.state_changed();
                    return;
                }
                Some((_, (frame, true))) => frame,
                None => self
                    .desks
                    .layout_on_mut(&screen)
                    .open(window.clone(), size, &screen, area),
            };
            let place = insets.window(frame);
            self.put(&window, frame);
            self.animations.opened(&window);
            eprintln!(
                "edel-compositor: mapped window {} at {},{} {}x{}",
                title(&window),
                place.loc.x,
                place.loc.y,
                place.size.w,
                place.size.h
            );
            // Which app a window belongs to, by its app id (M6.4): a web app
            // is a browser window with an app id of its own.
            let app = app_id(&window);
            if !app.is_empty() {
                eprintln!(
                    "edel-compositor: window {} belongs to app {app}",
                    title(&window)
                );
            }
            self.focus(&window);
            let maximize = std::mem::take(&mut data(&window).borrow_mut().maximize_when_placed);
            if maximize {
                self.maximize(&window);
            }
            let fullscreen = std::mem::take(&mut data(&window).borrow_mut().fullscreen_when_placed);
            if fullscreen {
                self.fullscreen(&window, None);
            }
            if self.desks.layout_of(&window).rearranges() {
                self.relayout();
            }
            self.settle_dynamic();
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

    /// A window asks to fill a screen, its own unless it names one
    /// (M5.20).
    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        let output = output.as_ref().and_then(Output::from_resource);
        match self.window_of(surface.wl_surface()) {
            Some(window) if self.space.element_geometry(&window).is_some() => {
                self.fullscreen(&window, output);
            }
            Some(window) => {
                data(&window).borrow_mut().fullscreen_when_placed = true;
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
            None => {}
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_of(surface.wl_surface()) else {
            return;
        };
        data(&window).borrow_mut().fullscreen_when_placed = false;
        if self.is_fullscreen(&window) {
            self.unfullscreen(&window);
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
pub fn listen(
    handle: &LoopHandle<'static, Edel>,
    state: &mut Edel,
    display: Display<Edel>,
) -> Result<String> {
    state.handle = Some(handle.clone());
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
delegate_kde_decoration!(Edel);
delegate_fractional_scale!(Edel);
delegate_viewporter!(Edel);
delegate_tablet_manager!(Edel);
/// The sizes a shape sets (M5.12b2b): the gaps between and at the edge of
/// tiles, the border's width and the title bar's height.
fn sizes_of(tokens: &Tokens) -> (Gaps, u32, u32) {
    (
        Gaps::from_tokens(tokens),
        tokens.border,
        tokens.title_bar_height,
    )
}

/// Logs the shape the desktop draws with, at start and on each change
/// (M5.12b2b), for CI's desktop test.
pub fn log_shape(tokens: &Tokens) {
    let (gaps, border, title_bar) = sizes_of(tokens);
    eprintln!(
        "edel-compositor: shape gaps {}/{}, border {border}, title bar {title_bar}",
        gaps.inner, gaps.outer
    );
}

/// The windows' depth the desktop draws with (M5.12b3): the focused and the
/// other windows' opacity in percent, the shadow's blur in logical pixels
/// and the shadow's alpha in percent, rounded.
fn depth_of(tokens: &Tokens) -> (u32, u32, u32, u32) {
    let alpha = (tokens.shadow.a * 100.0).round() as u32;
    (
        tokens.window_opacity,
        tokens.window_opacity_unfocused,
        tokens.shadow_blur,
        alpha,
    )
}

/// The line `log_depth` writes for `tokens`, for CI's desktop test.
fn depth_line(tokens: &Tokens) -> String {
    let (focused, other, blur, alpha) = depth_of(tokens);
    format!("edel-compositor: depth windows {focused}/{other} %, shadow {blur} px at {alpha} %")
}

/// Logs the windows' depth at start and on each change (M5.12b3), for
/// CI's desktop test.
pub fn log_depth(tokens: &Tokens) {
    eprintln!("{}", depth_line(tokens));
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn the_depth_line_names_both_opacities_the_shadow_and_its_alpha() {
        let mut tokens = Tokens::built_in();
        tokens.window_opacity = 90;
        tokens.window_opacity_unfocused = 90;
        tokens.shadow_blur = 48;
        tokens.shadow.a = 0.9;
        assert_eq!(
            depth_line(&tokens),
            "edel-compositor: depth windows 90/90 %, shadow 48 px at 90 %"
        );
    }
}

delegate_cursor_shape!(Edel);
delegate_seat!(Edel);
delegate_data_device!(Edel);
delegate_output!(Edel);
delegate_presentation!(Edel);
delegate_layer_shell!(Edel);
