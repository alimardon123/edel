//! The workspaces as the compositor tells them over ext-workspace-v1
//! (M5.2b), for the panel's switcher (M5.2c): each by name, in the order
//! the compositor made them, with the shown one marked, and the request
//! that shows one. Without the protocol (another compositor) the list
//! stays empty and the switcher shows nothing.

use std::sync::Arc;

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::backend::ObjectData;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle, WEnum};
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_group_handle_v1::{
    self, ExtWorkspaceGroupHandleV1,
};
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State,
};
use smithay_client_toolkit::reexports::protocols::ext::workspace::v1::client::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};

use crate::Shell;

pub struct Workspaces {
    manager: Option<ExtWorkspaceManagerV1>,
    list: Vec<Entry>,
}

struct Entry {
    handle: ExtWorkspaceHandleV1,
    name: String,
    active: bool,
}

impl Workspaces {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Workspaces {
        let manager = globals.bind(qh, 1..=1, Manager).ok();
        if manager.is_none() {
            eprintln!(
                "edel-shell-ui: the compositor offers no ext_workspace_manager_v1, so no workspace switcher"
            );
        }
        Workspaces {
            manager,
            list: Vec::new(),
        }
    }

    /// Every workspace by name, the shown one marked.
    pub fn names(&self) -> Vec<(String, bool)> {
        self.list
            .iter()
            .map(|e| (e.name.clone(), e.active))
            .collect()
    }

    /// Asks the compositor to show the workspace called `name`.
    pub fn show(&self, name: &str) {
        let (Some(manager), Some(entry)) =
            (&self.manager, self.list.iter().find(|e| e.name == name))
        else {
            return;
        };
        entry.handle.activate();
        manager.commit();
    }

    fn entry(&mut self, handle: &ExtWorkspaceHandleV1) -> Option<&mut Entry> {
        self.list.iter_mut().find(|e| &e.handle == handle)
    }
}

/// The objects' user data: each says what its events do to the shell.
pub struct Manager;
pub struct Group;
pub struct Handle;

impl Dispatch2<ExtWorkspaceManagerV1, Shell> for Manager {
    fn event(
        &self,
        shell: &mut Shell,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                shell.workspaces.list.push(Entry {
                    handle: workspace,
                    name: String::new(),
                    active: false,
                });
            }
            ext_workspace_manager_v1::Event::Done => shell.workspaces_changed(),
            ext_workspace_manager_v1::Event::Finished => {
                shell.workspaces.manager = None;
                shell.workspaces.list.clear();
                shell.workspaces_changed();
            }
            _ => {}
        }
    }

    /// The manager's events make groups and workspaces.
    fn event_created_child(opcode: u16, qh: &QueueHandle<Shell>) -> Arc<dyn ObjectData> {
        if opcode == ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE {
            qh.make_data::<ExtWorkspaceGroupHandleV1, Group>(Group)
        } else {
            qh.make_data::<ExtWorkspaceHandleV1, Handle>(Handle)
        }
    }
}

/// One group holds every workspace (M5.2b), so its events say nothing the
/// switcher needs; a group removed is destroyed, as the protocol asks.
impl Dispatch2<ExtWorkspaceGroupHandleV1, Shell> for Group {
    fn event(
        &self,
        _: &mut Shell,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        if let ext_workspace_group_handle_v1::Event::Removed = event {
            group.destroy();
        }
    }
}

impl Dispatch2<ExtWorkspaceHandleV1, Shell> for Handle {
    fn event(
        &self,
        shell: &mut Shell,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::Name { name } => {
                if let Some(entry) = shell.workspaces.entry(handle) {
                    entry.name = name;
                }
            }
            ext_workspace_handle_v1::Event::State { state } => {
                if let (Some(entry), WEnum::Value(state)) = (shell.workspaces.entry(handle), state)
                {
                    entry.active = state.contains(State::Active);
                }
            }
            ext_workspace_handle_v1::Event::Removed => {
                shell.workspaces.list.retain(|e| &e.handle != handle);
                handle.destroy();
            }
            _ => {}
        }
    }
}
