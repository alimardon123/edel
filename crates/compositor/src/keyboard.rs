//! Keyboard layouts (roadmap M5.21): `region.keyboard` from the settings
//! file, handed to xkb at start and whenever the file changes, and
//! Super+Space (`next_keyboard_layout`) to go to the next one. A layout
//! xkb cannot load leaves the one before in place and says so. Shortcuts
//! match keys on the Latin layout, whatever the layout (`shortcuts.rs`).

use smithay::input::keyboard::XkbConfig;

use edel_compositor::messages;

use crate::state::Edel;

impl Edel {
    /// Gives xkb the layouts `region.keyboard` names, or its default.
    pub fn apply_keyboard(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let written = self.settings.keyboard.clone().unwrap_or_default();
        let layouts = if written.is_empty() {
            Vec::new()
        } else {
            match edel::keyboard::parse(&written) {
                Ok(layouts) => layouts,
                Err(e) => {
                    eprintln!("edel-compositor: region.keyboard: {e}; keeping the layout in use");
                    return;
                }
            }
        };
        let (layout, variant) = edel::keyboard::xkb_names(&layouts);
        let config = XkbConfig {
            layout: &layout,
            variant: &variant,
            ..Default::default()
        };
        match keyboard.set_xkb_config(self, config) {
            Ok(()) if layouts.is_empty() => {
                eprintln!("edel-compositor: keyboard layout us, xkb's default");
            }
            Ok(()) => {
                let names: Vec<String> = layouts.iter().map(ToString::to_string).collect();
                let which = if names.len() == 1 {
                    "layout"
                } else {
                    "layouts"
                };
                eprintln!("edel-compositor: keyboard {which} {}", names.join(", "));
            }
            Err(e) => eprintln!(
                "edel-compositor: {}",
                messages::keyboard_not_loaded(&written, format!("{e:?}"))
            ),
        }
    }

    /// Goes to the next of the keyboard's layouts, and says which.
    pub fn next_keyboard_layout(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let name = keyboard.with_xkb_state(self, |mut context| {
            context.cycle_next_layout();
            let xkb = context.xkb().lock().ok()?;
            Some(xkb.layout_name(xkb.active_layout()).to_string())
        });
        if let Some(name) = name {
            eprintln!("edel-compositor: keyboard layout now {name}");
        }
    }
}
