//! `edel-shell-v1` (roadmap M5.3), our private protocol between the
//! compositor and shell-ui, from `protocols/edel-shell-v1.xml`: what the
//! standard protocols do not carry. Today the shown workspace's policy,
//! sent when shell-ui binds and whenever it changes (`sync_shell`, run
//! with the state file), for the panel's layout toggle; `toggle_policy`,
//! which switches it as Super+T does; the launcher's key (M5.3b) and the
//! tray's key (M5.9h), which show or hide shell-ui's grids, and the
//! settings files' changes (M5.9h), which shell-ui reads again;
//! which shows or hides shell-ui's launcher; and the window switcher's
//! titles and its end (M5.3c), which shell-ui draws; and the keyboard's
//! layouts and the one in use (M5.9f), for the layout indicator, with the
//! request that goes to the next one. Any client may
//! bind it, as with the workspace protocol: a narrower rule waits for a
//! client the person did not start.

use std::cell::RefCell;

use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::state::Edel;

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    missing_docs,
    unused_imports
)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use wayland_server::protocol::*;
    pub mod __interfaces {
        use smithay::reexports::wayland_server::backend as wayland_backend;
        wayland_scanner::generate_interfaces!("protocols/edel-shell-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_server_code!("protocols/edel-shell-v1.xml");
}

use protocol::edel_shell_v1::{self, EdelShellV1};

/// Every bound link, with what it was last told.
#[derive(Default)]
pub struct Links(RefCell<Vec<(EdelShellV1, Told)>>);

/// What a link was last told: the policy, and the keyboard's layouts and
/// the one in use (M5.9f), `None` until it was told them.
#[derive(Default)]
pub struct Told {
    policy: String,
    keyboard: Option<(String, u32)>,
}

/// Offers `edel_shell_v1` to every client.
pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, EdelShellV1, ()>(1, ());
}

impl Edel {
    /// Tells each link the shown workspace's policy if it changed.
    pub fn sync_shell(&self) {
        let Ok(mut links) = self.links.0.try_borrow_mut() else {
            return;
        };
        links.retain(|(link, _)| link.is_alive());
        let now = self.desks.layout().name();
        for (link, told) in links.iter_mut() {
            if told.policy != now {
                link.policy(now.to_string());
                told.policy = now.to_string();
            }
        }
    }

    /// Tells each link the keyboard's layouts and the one in use, if they
    /// changed since it was last told (M5.9f). The layouts are xkb's
    /// names for `region.keyboard`, empty when nothing is set or it does
    /// not parse, as `apply_keyboard` keeps the layout in use then.
    pub fn tell_keyboard(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let written = self.settings.keyboard.as_deref().unwrap_or_default().trim();
        let layouts = match edel::keyboard::parse(written) {
            Ok(parsed) if !written.is_empty() => edel::keyboard::xkb_names(&parsed).0,
            _ => String::new(),
        };
        let active = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().ok()?;
            Some(xkb.active_layout().0)
        });
        let now = (layouts, active.unwrap_or(0));
        let Ok(mut links) = self.links.0.try_borrow_mut() else {
            return;
        };
        links.retain(|(link, _)| link.is_alive());
        for (link, told) in links.iter_mut() {
            if told.keyboard.as_ref() != Some(&now) {
                link.keyboard(now.0.clone(), now.1);
                told.keyboard = Some(now.clone());
            }
        }
    }
}

impl Edel {
    /// The switcher's titles, the chosen one marked (M5.3c).
    pub fn show_switcher(&self, titles: &str, chosen: u32) {
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.switcher(titles.to_string(), chosen);
            }
        }
    }

    pub fn hide_switcher(&self) {
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.switcher_hide();
            }
        }
    }

    /// A volume or brightness key (M5.9c): shell-ui changes the level and
    /// shows it.
    pub fn media_key(&self, name: &str) {
        eprintln!("edel-compositor: media key {name}");
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.media_key(name.to_string());
            }
        }
    }

    /// The launcher's key: shell-ui shows its launcher, or hides it.
    pub fn show_launcher(&self) {
        eprintln!("edel-compositor: launcher");
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.launcher();
            }
        }
    }

    /// The tray's key (M5.9h): shell-ui opens the grid of the apps behind
    /// the tray's arrow, or closes it.
    pub fn show_tray(&self) {
        eprintln!("edel-compositor: tray");
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.tray();
            }
        }
    }

    /// The settings files changed (M5.9h): every link reads them again, as
    /// the tray's list is read when the tray changes. Nothing is logged,
    /// as it happens at every change.
    pub fn tell_settings(&self) {
        for (link, _) in self.links.0.borrow().iter() {
            if link.is_alive() {
                link.settings();
            }
        }
    }
}

impl GlobalDispatch<EdelShellV1, ()> for Edel {
    /// Hidden from sandboxed apps: only the shell's (M5.22).
    fn can_view(client: Client, _: &()) -> bool {
        crate::sandbox::shell_may(&client)
    }

    fn bind(
        state: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<EdelShellV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let link = data_init.init(resource, ());
        state.links.0.borrow_mut().push((link, Told::default()));
        state.sync_shell();
        state.tell_keyboard();
    }
}

impl Dispatch<EdelShellV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        _: &EdelShellV1,
        request: edel_shell_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        match request {
            edel_shell_v1::Request::TogglePolicy => state.toggle_tiling(),
            edel_shell_v1::Request::NextKeyboardLayout => state.next_keyboard_layout(),
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Edel,
        _: smithay::reexports::wayland_server::backend::ClientId,
        resource: &EdelShellV1,
        _: &(),
    ) {
        state
            .links
            .0
            .borrow_mut()
            .retain(|(link, _)| link != resource);
    }
}
