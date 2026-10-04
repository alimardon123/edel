//! `edel-shell-v1` (roadmap M5.3), our private protocol between the
//! compositor and shell-ui, from `protocols/edel-shell-v1.xml`: what the
//! standard protocols do not carry. Today the shown workspace's policy,
//! sent when shell-ui binds and whenever it changes (`sync_shell`, run
//! with the state file), for the panel's layout toggle; and
//! `toggle_policy`, which switches it as Super+T does. Any client may
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

impl GlobalDispatch<EdelShellV1, ()> for Edel {
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
