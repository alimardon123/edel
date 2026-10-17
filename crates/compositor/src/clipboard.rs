//! Clipboard history's side here (M7.13): the compositor offers
//! ext-data-control-v1, through which shell-ui sees each new selection
//! and sets an older one again, hidden from sandboxed apps as every shell
//! protocol is (`sandbox::shell_may`), so no Flatpak app reads what is
//! copied in another. shell-ui keeps the history; nothing is kept here.
//! Super+V (`Act::ClipboardHistory`) asks shell-ui for its list, and its
//! `paste` request over `edel-shell-v1` sends Ctrl+V to the window that
//! has the keyboard once the list is gone, as a person would press it.

use smithay::backend::input::KeyState;
use smithay::delegate_ext_data_control;
use smithay::input::keyboard::{FilterResult, Keycode, xkb};
use smithay::utils::{Clock, Monotonic, SERIAL_COUNTER};
use smithay::wayland::selection::ext_data_control::{DataControlHandler, DataControlState};

use crate::state::Edel;

/// Control and V on a PC keyboard, as xkb numbers them, for a keymap
/// that has no `v` at all.
const CONTROL: u32 = 37;
const V: u32 = 55;

impl DataControlHandler for Edel {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control
    }
}

delegate_ext_data_control!(Edel);

/// The key that gives `wanted` in the first layout that has it at its
/// first level, the way a Latin shortcut is found on any layout, among
/// `syms` (each keycode's symbols in each layout).
pub fn key_for(
    wanted: xkb::Keysym,
    syms: impl IntoIterator<Item = (u32, Vec<xkb::Keysym>)>,
) -> Option<u32> {
    syms.into_iter()
        .find(|(_, syms)| syms.contains(&wanted))
        .map(|(code, _)| code)
}

impl Edel {
    /// shell-ui set a chosen item as the selection: the window under the
    /// list gets the keyboard back, if a layer still holds it, and Ctrl+V,
    /// so the item is pasted there. Nothing while the screen is locked.
    pub fn paste(&mut self) {
        if self.is_locked() {
            return;
        }
        self.take_keyboard_back();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let window = keyboard.current_focus().and_then(|surface| {
            self.space
                .elements()
                .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == &surface))
                .cloned()
        });
        let Some(window) = window else {
            eprintln!("edel-compositor: nothing to paste into, no window has the keyboard");
            return;
        };
        let (control, v) = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            let find = |wanted| {
                xkb.layouts().find_map(|layout| {
                    let syms = (8..256).map(|code: u32| {
                        let syms = xkb.raw_syms_for_key_in_layout(code.into(), layout);
                        (code, syms.to_vec())
                    });
                    key_for(wanted, syms)
                })
            };
            (
                find(xkb::Keysym::Control_L).unwrap_or(CONTROL),
                find(xkb::Keysym::v).unwrap_or(V),
            )
        });
        let time = Clock::<Monotonic>::new().now().as_millis();
        for (code, state) in [
            (control, KeyState::Pressed),
            (v, KeyState::Pressed),
            (v, KeyState::Released),
            (control, KeyState::Released),
        ] {
            keyboard.input::<(), _>(
                self,
                Keycode::new(code),
                state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        }
        eprintln!(
            "edel-compositor: pasted into window {}",
            crate::decoration::title(&window)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_layout_with_the_symbol_gives_the_key() {
        let us = vec![(54, vec![xkb::Keysym::c]), (55, vec![xkb::Keysym::v])];
        assert_eq!(key_for(xkb::Keysym::v, us), Some(55));
        let dvorak = vec![(55, vec![xkb::Keysym::k]), (60, vec![xkb::Keysym::v])];
        assert_eq!(key_for(xkb::Keysym::v, dvorak), Some(60));
        let none = vec![(55, vec![xkb::Keysym::Cyrillic_em])];
        assert_eq!(key_for(xkb::Keysym::v, none), None);
    }
}
