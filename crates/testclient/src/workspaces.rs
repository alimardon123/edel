//! `edel-testclient --workspace N` (roadmap M5.2b): shows workspace N
//! through ext-workspace-v1, as a panel's switcher does, and prints the
//! workspaces by name before and after, the shown one starred:
//!
//!     workspaces 1* 2 3 4
//!     workspaces 1 2 3* 4

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::reexports::client::globals::{GlobalListContents, registry_queue_init};
use smithay_client_toolkit::reexports::client::protocol::wl_registry;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
};
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1;
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State,
};
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};

/// One workspace as the compositor last described it.
struct Workspace {
    handle: ExtWorkspaceHandleV1,
    name: String,
    active: bool,
}

#[derive(Default)]
struct Seen {
    workspaces: Vec<Workspace>,
    /// `done` came: the list is whole.
    done: bool,
}

impl Seen {
    fn line(&self) -> String {
        let names: Vec<String> = self
            .workspaces
            .iter()
            .map(|w| format!("{}{}", w.name, if w.active { "*" } else { "" }))
            .collect();
        format!("workspaces {}", names.join(" "))
    }

    fn find(&mut self, handle: &ExtWorkspaceHandleV1) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| &w.handle == handle)
    }
}

pub fn run(wanted: &str) -> Result<()> {
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, mut queue) =
        registry_queue_init::<Seen>(&connection).context("reading the globals")?;
    let qh = queue.handle();
    let manager: ExtWorkspaceManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .context("the compositor offers no ext_workspace_manager_v1")?;
    let mut seen = Seen::default();
    while !seen.done {
        queue.blocking_dispatch(&mut seen)?;
    }
    println!("{}", seen.line());
    let Some(target) = seen.workspaces.iter().find(|w| w.name == wanted) else {
        bail!("no workspace is called {wanted}");
    };
    target.handle.activate();
    manager.commit();
    seen.done = false;
    while !(seen.done && seen.workspaces.iter().any(|w| w.name == wanted && w.active)) {
        seen.done = false;
        queue.blocking_dispatch(&mut seen)?;
    }
    println!("{}", seen.line());
    manager.stop();
    connection.flush()?;
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Seen {
    fn event(
        _: &mut Seen,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Seen {
    fn event(
        seen: &mut Seen,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                seen.workspaces.push(Workspace {
                    handle: workspace,
                    name: String::new(),
                    active: false,
                });
            }
            ext_workspace_manager_v1::Event::Done => seen.done = true,
            _ => {}
        }
    }

    event_created_child!(Seen, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Seen {
    fn event(
        _: &mut Seen,
        _: &ExtWorkspaceGroupHandleV1,
        _: <ExtWorkspaceGroupHandleV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for Seen {
    fn event(
        seen: &mut Seen,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::Name { name } => {
                if let Some(w) = seen.find(handle) {
                    w.name = name;
                }
            }
            ext_workspace_handle_v1::Event::State { state } => {
                if let (Some(w), WEnum::Value(state)) = (seen.find(handle), state) {
                    w.active = state.contains(State::Active);
                }
            }
            ext_workspace_handle_v1::Event::Removed => {
                seen.workspaces.retain(|w| &w.handle != handle);
            }
            _ => {}
        }
    }
}
