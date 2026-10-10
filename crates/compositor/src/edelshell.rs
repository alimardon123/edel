//! `edel-shell-v1` (roadmap M5.3), our private protocol between the
//! compositor and shell-ui, from `protocols/edel-shell-v1.xml`: what the
//! standard protocols do not carry. Today the shown workspace's policy,
//! sent when shell-ui binds and whenever it changes (`sync_shell`, run
//! with the state file), for the panel's layout toggle; `toggle_policy`,
//! which switches it as Super+T does; the launcher's key (M5.3b),
//! which shows or hides shell-ui's launcher; and the window switcher's
//! titles and its end (M5.3c), which shell-ui draws. Any client may
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

/// Every bound link, with the policy it was last told.
#[derive(Default)]
pub struct Links(RefCell<Vec<(EdelShellV1, String)>>);

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
            if told != now {
                link.policy(now.to_string());
                *told = now.to_string();
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
        state.links.0.borrow_mut().push((link, String::new()));
        state.sync_shell();
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
        if let edel_shell_v1::Request::TogglePolicy = request {
            state.toggle_tiling();
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
