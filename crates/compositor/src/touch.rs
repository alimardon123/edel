//! Touch screens (roadmap M9.5): the first finger down is the pointer. It
//! moves the pointer where it touches and presses the left button, a drag
//! moves it with the button held, and lifting it releases the button, so
//! a tap is a click wherever a click works (apps, the panels, the title
//! bars and their buttons, the overview, the lock screen) and a drag on a
//! title bar moves the window. Other fingers wait while the first is
//! down. Apps get no wl_touch of their own yet, so a drag in an app selects
//! as a mouse does rather than scrolling; gestures come with the phone's
//! (M9.4c).

use smithay::backend::input::{ButtonState, Event, InputBackend, TouchEvent};
use smithay::utils::Serial;

use crate::state::Edel;

/// The button a finger presses.
const BUTTON_LEFT: u32 = 0x110;

impl Edel {
    /// A finger touched the screen: the first one down becomes the
    /// pointer and presses.
    pub(crate) fn touch_down<B: InputBackend>(
        &mut self,
        event: &B::TouchDownEvent,
        serial: Serial,
    ) -> Option<()> {
        if self.touching.is_some() {
            return Some(());
        }
        let at = self.on_screen(event)?;
        self.touching = Some(event.slot());
        self.move_pointer(at, serial, event.time_msec());
        // A window tapped may show the keyboard on the screen (M9.5).
        if let crate::input::Under::Surface(_, surface, _) = self.under(at) {
            self.screen_keyboard.tapped(surface);
        }
        let pressed = self.button(BUTTON_LEFT, ButtonState::Pressed, serial, event.time_msec());
        // The tap may have moved the focus to a field already asking for
        // text, which says nothing more, so the keyboard follows now.
        if let Some(handle) = self.handle.clone() {
            self.follow_screen_keyboard(&handle);
        }
        pressed
    }

    /// The pointer's finger moved: the pointer follows it, button held.
    pub(crate) fn touch_motion<B: InputBackend>(
        &mut self,
        event: &B::TouchMotionEvent,
        serial: Serial,
    ) -> Option<()> {
        if self.touching != Some(event.slot()) {
            return Some(());
        }
        let at = self.on_screen(event)?;
        self.move_pointer(at, serial, event.time_msec());
        Some(())
    }

    /// The pointer's finger lifted: the button goes up where it was.
    pub(crate) fn touch_up<B: InputBackend>(
        &mut self,
        event: &B::TouchUpEvent,
        serial: Serial,
    ) -> Option<()> {
        if self.touching != Some(event.slot()) {
            return Some(());
        }
        self.touching = None;
        self.button(
            BUTTON_LEFT,
            ButtonState::Released,
            serial,
            event.time_msec(),
        )
    }

    /// The device took its touches back (a palm, a device gone): the
    /// button goes up, as nothing holds it any more.
    pub(crate) fn touch_cancel<B: InputBackend>(
        &mut self,
        event: &B::TouchCancelEvent,
        serial: Serial,
    ) -> Option<()> {
        self.touching.take()?;
        self.button(
            BUTTON_LEFT,
            ButtonState::Released,
            serial,
            event.time_msec(),
        )
    }
}
