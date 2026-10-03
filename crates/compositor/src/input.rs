//! Keyboard and pointer for both backends (M4.2b, M4.3): keys go to the
//! focused window; the pointer moves, a click focuses and raises the
//! window under it, and Super with the left button moves a window, with
//! the right button resizes it from the nearest corner. Ctrl+Alt+F1 to
//! F12 ask for a virtual terminal, which only a real seat can switch to,
//! so a person can always reach a text console.

use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent, KeyState,
    KeyboardKeyEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::input::keyboard::{FilterResult, xkb};
use smithay::input::pointer::{
    ButtonEvent, Focus, GrabStartData, MotionEvent, RelativeMotionEvent,
};
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use crate::grabs::{Kind, WindowGrab, nearest_corner};
use crate::state::Edel;

/// Linux's codes for the left and right mouse buttons (`BTN_LEFT`,
/// `BTN_RIGHT`).
const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;

impl Edel {
    /// Handles one input event; returns the virtual terminal Ctrl+Alt+F1
    /// to F12 asked for.
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
                let location = clamp(pointer.current_location() + event.delta(), area);
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
                if event.state() == ButtonState::Pressed && !pointer.is_grabbed() {
                    let location = pointer.current_location();
                    let under = self.space.element_under(location).map(|(w, _)| w.clone());
                    if let Some(window) = under {
                        self.focus(&window);
                        let logo = self
                            .seat
                            .get_keyboard()
                            .is_some_and(|k| k.modifier_state().logo);
                        let place = self.space.element_geometry(&window);
                        let kind = match button {
                            BUTTON_LEFT => Some(Kind::Move),
                            BUTTON_RIGHT => {
                                place.map(|place| Kind::Resize(nearest_corner(place, location)))
                            }
                            _ => None,
                        };
                        if let (true, Some(kind), Some(place)) = (logo, kind, place) {
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
                            // The window never sees a click it did not get.
                            return None;
                        }
                    }
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

    /// The first output's area in the layout.
    pub fn output_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space.output_geometry(output)
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
