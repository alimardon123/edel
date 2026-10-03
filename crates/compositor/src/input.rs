//! Keyboard and pointer for both backends (M4.2b, M4.3, M4.4): keys go to
//! the focused window; the pointer moves, a click focuses and raises the
//! window under it. A window's title bar moves it when dragged and
//! maximizes it on a double click; its buttons act when released over
//! them; its edges resize it. Super with the left button moves any window,
//! with the right button resizes it from the nearest corner, and Super+Q
//! closes the focused one. Ctrl+Alt+F1 to F12 ask for a virtual terminal,
//! which only a real seat can switch to, so a person can always reach a
//! text console.

use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent, KeyState,
    KeyboardKeyEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::desktop::{Window, WindowSurfaceType};
use smithay::input::keyboard::{FilterResult, xkb};
use smithay::input::pointer::{
    ButtonEvent, Focus, GrabStartData, MotionEvent, RelativeMotionEvent,
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial};

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
}

/// What is under the pointer.
pub enum Under {
    /// A window's surface, at the given place on screen.
    Surface(Window, WlSurface, Point<f64, Logical>),
    /// A part of the frame the compositor draws round a window.
    Frame(Window, Hit),
    Nothing,
}

impl Under {
    fn window(&self) -> Option<&Window> {
        match self {
            Under::Surface(window, ..) | Under::Frame(window, _) => Some(window),
            Under::Nothing => None,
        }
    }

    /// The surface that gets the pointer's events, if any.
    fn focus(self) -> Option<(WlSurface, Point<f64, Logical>)> {
        match self {
            Under::Surface(_, surface, at) => Some((surface, at)),
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
                        // Q where a Latin layout has it, whatever the layout.
                        let q = keysym
                            .raw_latin_sym_or_raw_current_sym()
                            .is_some_and(|s| s.raw() == xkb::keysyms::KEY_q);
                        if pressed && vts.contains(&sym) {
                            FilterResult::Intercept(Action::Terminal(
                                (sym - xkb::keysyms::KEY_XF86Switch_VT_1 + 1) as i32,
                            ))
                        } else if pressed && modifiers.logo && q {
                            FilterResult::Intercept(Action::Close)
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
                }
            }
            InputEvent::PointerMotion { event } => {
                let pointer = self.seat.get_pointer()?;
                let area = self.output_area()?;
                let location = clamp(pointer.current_location() + event.delta(), area);
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
            _ => {}
        }
        None
    }

    /// What is at `point`: the windows from the top, each window's own
    /// surfaces (its popups may lie over its bar) before its frame.
    pub fn under(&self, point: Point<f64, Logical>) -> Under {
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

    /// The first output's area in the layout.
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
