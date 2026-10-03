//! Keyboard and pointer for both backends (M4.2b, M4.3, M4.4): keys go to
//! the focused window; the pointer moves, a click focuses and raises the
//! window under it. A window's title bar moves it when dragged and
//! maximizes it on a double click; its buttons act when released over
//! them; its edges resize it. Super with the left button moves any window,
//! with the right button resizes it from the nearest corner, Super+Q
//! closes the focused one and Super+T switches the workspace between
//! floating and tiling (M4.5). Wheels and touchpads scroll the window
//! under the pointer, and pens reach windows through `zwp_tablet_v2`
//! (M4.6b). Ctrl+Alt+F1 to F12 ask for a virtual terminal,
//! which only a real seat can switch to, so a person can always reach a
//! text console.

use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Device, DeviceCapability, Event,
    InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    PointerMotionEvent, ProximityState, TabletToolButtonEvent, TabletToolEvent,
    TabletToolProximityEvent, TabletToolTipEvent, TabletToolTipState,
};
use smithay::desktop::{LayerSurface, Window, WindowSurfaceType};
use smithay::input::keyboard::{FilterResult, xkb};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, Focus, GrabStartData, MotionEvent, RelativeMotionEvent,
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial};
use smithay::wayland::tablet_manager::{TabletDescriptor, TabletSeatTrait};

use edel_compositor::frame::{self, Button, Hit};

use crate::grabs::{Kind, WindowGrab, nearest_corner};
use crate::state::Edel;

/// Linux's codes for the left and right mouse buttons (`BTN_LEFT`,
/// `BTN_RIGHT`).
const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;

/// Two clicks on a title bar this close together, in milliseconds, are a
/// double click.
const DOUBLE_CLICK: u32 = 400;

/// What a key the compositor keeps from the windows does.
enum Action {
    Terminal(i32),
    Close,
    ToggleTiling,
}

/// What is under the pointer.
pub enum Under {
    /// A window's surface, at the given place on screen.
    Surface(Window, WlSurface, Point<f64, Logical>),
    /// A panel's or background's surface (M5.1a).
    Layer(LayerSurface, WlSurface, Point<f64, Logical>),
    /// A part of the frame the compositor draws round a window.
    Frame(Window, Hit),
    Nothing,
}

impl Under {
    fn window(&self) -> Option<&Window> {
        match self {
            Under::Surface(window, ..) | Under::Frame(window, _) => Some(window),
            Under::Layer(..) | Under::Nothing => None,
        }
    }

    /// The surface that gets the pointer's events, if any.
    fn focus(self) -> Option<(WlSurface, Point<f64, Logical>)> {
        match self {
            Under::Surface(_, surface, at) | Under::Layer(_, surface, at) => Some((surface, at)),
            _ => None,
        }
    }
}

impl Edel {
    /// Handles one input event; returns the virtual terminal Ctrl+Alt+F1
    /// to F12 asked for.
    pub fn input<B: InputBackend>(&mut self, event: InputEvent<B>) -> Option<i32> {
        let serial = SERIAL_COUNTER.next_serial();
        match event {
            InputEvent::Keyboard { event } => {
                let pressed = event.state() == KeyState::Pressed;
                let keyboard = self.seat.get_keyboard()?;
                let action = keyboard.input(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    event.time_msec(),
                    |_, modifiers, keysym| {
                        let sym = keysym.modified_sym().raw();
                        let vts =
                            xkb::keysyms::KEY_XF86Switch_VT_1..=xkb::keysyms::KEY_XF86Switch_VT_12;
                        // Q and T where a Latin layout has them, whatever
                        // the layout.
                        let latin = keysym.raw_latin_sym_or_raw_current_sym().map(|s| s.raw());
                        let q = latin == Some(xkb::keysyms::KEY_q);
                        let t = latin == Some(xkb::keysyms::KEY_t);
                        if pressed && vts.contains(&sym) {
                            FilterResult::Intercept(Action::Terminal(
                                (sym - xkb::keysyms::KEY_XF86Switch_VT_1 + 1) as i32,
                            ))
                        } else if pressed && modifiers.logo && q {
                            FilterResult::Intercept(Action::Close)
                        } else if pressed && modifiers.logo && t {
                            FilterResult::Intercept(Action::ToggleTiling)
                        } else {
                            FilterResult::Forward
                        }
                    },
                );
                match action? {
                    Action::Terminal(vt) => return Some(vt),
                    Action::Close => {
                        if let Some(window) = self.focused_window() {
                            self.close(&window);
                        }
                    }
                    Action::ToggleTiling => self.toggle_tiling(),
                }
            }
            InputEvent::PointerMotion { event } => {
                let pointer = self.seat.get_pointer()?;
                let from = pointer.current_location();
                let location = self.on_screens(from, from + event.delta())?;
                let focus = self.pointer_over(location);
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
                let focus = self.pointer_over(location);
                let pointer = self.seat.get_pointer()?;
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
                self.dirty = true;
            }
            InputEvent::PointerButton { event } => {
                let pointer = self.seat.get_pointer()?;
                let button = event.button_code();
                let location = pointer.current_location();
                let pressed = event.state() == ButtonState::Pressed;
                if pressed
                    && !pointer.is_grabbed()
                    && self.press(button, location, serial, event.time_msec())
                {
                    // The window never sees a click it did not get.
                    return None;
                }
                if !pressed && button == BUTTON_LEFT {
                    self.release(location);
                }
                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: event.state(),
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event } => self.scroll::<B>(event),
            InputEvent::DeviceAdded { device } => {
                if device.has_capability(DeviceCapability::TabletTool) {
                    let tablets = self.seat.tablet_seat();
                    tablets.add_tablet::<Edel>(&self.display, &TabletDescriptor::from(&device));
                }
            }
            InputEvent::DeviceRemoved { device } => {
                if device.has_capability(DeviceCapability::TabletTool) {
                    let tablets = self.seat.tablet_seat();
                    tablets.remove_tablet(&TabletDescriptor::from(&device));
                    if tablets.count_tablets() == 0 {
                        tablets.clear_tools();
                    }
                }
            }
            InputEvent::TabletToolAxis { event } => {
                let area = self.output_area()?;
                let at = event.position_transformed(area.size) + area.loc.to_f64();
                let focus = self.pointer_over(at);
                let tablets = self.seat.tablet_seat();
                let tablet = tablets.get_tablet(&TabletDescriptor::from(&event.device()));
                let tool = tablets.get_tool(&event.tool());
                if let (Some(tablet), Some(tool)) = (tablet, tool) {
                    if event.pressure_has_changed() {
                        tool.pressure(event.pressure());
                    }
                    if event.distance_has_changed() {
                        tool.distance(event.distance());
                    }
                    if event.tilt_has_changed() {
                        tool.tilt(event.tilt());
                    }
                    if event.slider_has_changed() {
                        tool.slider_position(event.slider_position());
                    }
                    if event.rotation_has_changed() {
                        tool.rotation(event.rotation());
                    }
                    if event.wheel_has_changed() {
                        tool.wheel(event.wheel_delta(), event.wheel_delta_discrete());
                    }
                    tool.motion(at, focus, &tablet, serial, event.time_msec());
                }
                self.move_pointer(at, serial, event.time_msec());
            }
            InputEvent::TabletToolProximity { event } => {
                let area = self.output_area()?;
                let at = event.position_transformed(area.size) + area.loc.to_f64();
                let tablets = self.seat.tablet_seat();
                let display = self.display.clone();
                let tool = tablets.add_tool::<Edel>(self, &display, &event.tool());
                let tablet = tablets.get_tablet(&TabletDescriptor::from(&event.device()));
                match (event.state(), tablet) {
                    (ProximityState::In, Some(tablet)) => {
                        if let Some(focus) = self.pointer_over(at) {
                            tool.proximity_in(at, focus, &tablet, serial, event.time_msec());
                        }
                    }
                    (ProximityState::Out, _) => tool.proximity_out(event.time_msec()),
                    _ => {}
                }
                self.move_pointer(at, serial, event.time_msec());
            }
            InputEvent::TabletToolTip { event } => {
                let tool = self.seat.tablet_seat().get_tool(&event.tool())?;
                match event.tip_state() {
                    TabletToolTipState::Down => {
                        // A pen touching a window focuses it, as a click does.
                        let at = self.seat.get_pointer()?.current_location();
                        if let Some(window) = self.under(at).window().cloned() {
                            self.focus(&window);
                        }
                        tool.tip_down(serial, event.time_msec());
                    }
                    TabletToolTipState::Up => tool.tip_up(event.time_msec()),
                }
            }
            InputEvent::TabletToolButton { event } => {
                let tool = self.seat.tablet_seat().get_tool(&event.tool())?;
                tool.button(
                    event.button(),
                    event.button_state(),
                    serial,
                    event.time_msec(),
                );
            }
            _ => {}
        }
        None
    }

    /// A wheel turned or fingers slid on a touchpad: the window under the
    /// pointer scrolls, by wheel clicks where the device counts them.
    fn scroll<B: InputBackend>(&mut self, event: B::PointerAxisEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let source = event.source();
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let v120 = event.amount_v120(axis);
            // A wheel click is 15 units of scrolling where only clicks come.
            let amount = event
                .amount(axis)
                .or_else(|| v120.map(|v| v * 15.0 / 120.0))
                .unwrap_or(0.0);
            if amount != 0.0 {
                frame = frame
                    .relative_direction(axis, event.relative_direction(axis))
                    .value(axis, amount);
                if let Some(v120) = v120 {
                    frame = frame.v120(axis, v120 as i32);
                }
            } else if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                // Fingers lifted: kinetic scrolling may start now.
                frame = frame.stop(axis);
            }
        }
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// After the screen shrank or rescaled: the pointer moves just enough
    /// to be on it again, so its cursor never goes out of sight.
    pub fn keep_pointer_on_screen(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        let Some(kept) = self.on_screens(at, at) else {
            return;
        };
        if kept != at {
            self.move_pointer(kept, SERIAL_COUNTER.next_serial(), 0);
        }
    }

    /// Moves the pointer to `at`, as a pen does on its way.
    fn move_pointer(&mut self, at: Point<f64, Logical>, serial: Serial, time: u32) {
        let focus = self.pointer_over(at);
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.motion(
                self,
                focus,
                &MotionEvent {
                    location: at,
                    serial,
                    time,
                },
            );
            pointer.frame(self);
        }
        self.dirty = true;
    }

    /// What is at `point`: panels over the windows, then the windows from
    /// the top, each window's own surfaces (its popups may lie over its
    /// bar) before its frame, then backgrounds.
    pub fn under(&self, point: Point<f64, Logical>) -> Under {
        if let Some((layer, surface, at)) = self.layer_under(&crate::layers::ABOVE, point) {
            return Under::Layer(layer, surface, at);
        }
        for window in self.space.elements().rev() {
            let Some(place) = self.space.element_geometry(window) else {
                continue;
            };
            let origin = place.loc - window.geometry().loc;
            if let Some((surface, offset)) =
                window.surface_under(point - origin.to_f64(), WindowSurfaceType::ALL)
            {
                return Under::Surface(window.clone(), surface, (offset + origin).to_f64());
            }
            let insets = self.insets(window);
            if insets != frame::Insets::default() {
                let outer = insets.frame(place);
                let resizable = !self.is_maximized(window);
                let at = point - outer.loc.to_f64();
                if let Some(hit) = frame::hit(outer.size, insets, at, resizable) {
                    return Under::Frame(window.clone(), hit);
                }
            }
        }
        if let Some((layer, surface, at)) = self.layer_under(&crate::layers::BELOW, point) {
            return Under::Layer(layer, surface, at);
        }
        Under::Nothing
    }

    /// The surface the pointer is over, after noting which title bar
    /// button it is over, if any.
    fn pointer_over(
        &mut self,
        point: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let under = self.under(point);
        let hover = match &under {
            Under::Frame(window, Hit::Button(button)) => Some((window.clone(), *button)),
            _ => None,
        };
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
        under.focus()
    }

    /// A button went down where no grab is; returns true when the
    /// compositor took the click for itself.
    fn press(
        &mut self,
        button: u32,
        location: Point<f64, Logical>,
        serial: Serial,
        time: u32,
    ) -> bool {
        let under = self.under(location);
        if let Under::Layer(layer, ..) = &under {
            self.focus_layer(layer);
            return false;
        }
        let Some(window) = under.window().cloned() else {
            return false;
        };
        self.focus(&window);
        let logo = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().logo);
        let Some(place) = self.space.element_geometry(&window) else {
            return false;
        };
        let kind = match (&under, button, logo) {
            (_, BUTTON_LEFT, true) => Kind::Move,
            (_, BUTTON_RIGHT, true) if !self.is_maximized(&window) => {
                Kind::Resize(nearest_corner(place, location))
            }
            (Under::Frame(_, Hit::Title), BUTTON_LEFT, false) => {
                let double = self
                    .last_title_click
                    .take()
                    .is_some_and(|(w, t)| w == window && time.wrapping_sub(t) <= DOUBLE_CLICK);
                if double {
                    self.toggle_maximized(&window);
                    return true;
                }
                self.last_title_click = Some((window.clone(), time));
                Kind::Move
            }
            (Under::Frame(_, Hit::Edge(edges)), BUTTON_LEFT, false) => Kind::Resize(*edges),
            (Under::Frame(_, Hit::Button(b)), BUTTON_LEFT, false) => {
                self.pressed = Some((window, *b));
                return false;
            }
            _ => return false,
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let grab = WindowGrab {
            start: GrabStartData {
                focus: None,
                button,
                location,
            },
            window,
            kind,
            initial: place,
            current: place,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
        self.dragging = true;
        true
    }

    /// The left button went up: a title bar button pressed and released
    /// over the same button acts.
    fn release(&mut self, location: Point<f64, Logical>) {
        let Some((window, button)) = self.pressed.take() else {
            return;
        };
        let still = matches!(
            self.under(location),
            Under::Frame(w, Hit::Button(b)) if w == window && b == button
        );
        if !still {
            return;
        }
        match button {
            Button::Close => self.close(&window),
            Button::Maximize => self.toggle_maximized(&window),
        }
    }

    /// `to` if it is on a screen, else the nearest point to it on the
    /// screen `from` is on (else the first): the pointer crosses where
    /// screens touch and stops at the outer edges.
    fn on_screens(
        &self,
        from: Point<f64, Logical>,
        to: Point<f64, Logical>,
    ) -> Option<Point<f64, Logical>> {
        let screens: Vec<Rectangle<i32, Logical>> = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .collect();
        if screens.iter().any(|s| s.to_f64().contains(to)) {
            return Some(to);
        }
        let home = screens
            .iter()
            .find(|s| s.to_f64().contains(from))
            .or(screens.first())?;
        Some(clamp(to, *home))
    }

    /// The first output's area in the layout: where windows open, and what
    /// absolute pointers such as tablets map to.
    pub fn output_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space.output_geometry(output)
    }

    /// Forgets `window` wherever input remembers it, once it is gone.
    pub fn forget(&mut self, window: &Window) {
        if self.hover.as_ref().is_some_and(|(w, _)| w == window) {
            self.hover = None;
        }
        if self.pressed.as_ref().is_some_and(|(w, _)| w == window) {
            self.pressed = None;
        }
        if self
            .last_title_click
            .as_ref()
            .is_some_and(|(w, _)| w == window)
        {
            self.last_title_click = None;
        }
    }
}

/// `location` kept on the screen `area`.
fn clamp(mut location: Point<f64, Logical>, area: Rectangle<i32, Logical>) -> Point<f64, Logical> {
    location.x = location.x.clamp(
        f64::from(area.loc.x),
        f64::from(area.loc.x + area.size.w - 1),
    );
    location.y = location.y.clamp(
        f64::from(area.loc.y),
        f64::from(area.loc.y + area.size.h - 1),
    );
    location
}
