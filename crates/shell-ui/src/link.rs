//! shell-ui's end of `edel-shell-v1` (roadmap M5.3), the compositor's
//! private protocol (`crates/compositor/protocols/edel-shell-v1.xml`):
//! the shown workspace's policy, for the layout toggle, and the request
//! that switches it; the launcher's key (M5.3b); and the window
//! switcher's titles and its end (M5.3c). Without it (another
//! compositor) the toggle shows nothing and only the menu button opens
//! the launcher.

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};

use crate::Shell;

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    missing_docs,
    unused_imports
)]
pub mod protocol {
    use smithay_client_toolkit::reexports::client as wayland_client;
    use wayland_client::protocol::*;
    pub mod __interfaces {
        use smithay_client_toolkit::reexports::client::backend as wayland_backend;
        wayland_scanner::generate_interfaces!("../compositor/protocols/edel-shell-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("../compositor/protocols/edel-shell-v1.xml");
}

use protocol::edel_shell_v1::{self, EdelShellV1};

pub struct Link(Option<EdelShellV1>);

impl Link {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Link {
        let link = globals.bind(qh, 1..=1, Events).ok();
        if link.is_none() {
            eprintln!("edel-shell-ui: the compositor offers no edel_shell_v1, so no layout toggle");
        }
        Link(link)
    }

    /// Asks for the shown workspace's other policy, as Super+T.
    pub fn toggle_policy(&self) {
        if let Some(link) = &self.0 {
            link.toggle_policy();
        }
    }
}

/// The link's user data: its events change what the panel shows.
pub struct Events;

impl Dispatch2<EdelShellV1, Shell> for Events {
    fn event(
        &self,
        shell: &mut Shell,
        _: &EdelShellV1,
        event: edel_shell_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        match event {
            edel_shell_v1::Event::Policy { name } => {
                shell.live.policy = name;
                shell.draw_all();
            }
            edel_shell_v1::Event::Launcher => shell.toggle_launcher(),
            edel_shell_v1::Event::Switcher { titles, chosen } => {
                shell.show_switcher(crate::switcher::View::from_event(&titles, chosen));
            }
            edel_shell_v1::Event::SwitcherHide => shell.hide_switcher(),
        }
    }
}
