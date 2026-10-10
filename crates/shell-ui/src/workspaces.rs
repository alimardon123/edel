//! The workspaces as the compositor tells them over ext-workspace-v1
//! (M5.2b), for the panel's switcher (M5.2c): each by name, in the order
//! the compositor made them, with the shown one marked, and the request
//! that shows one. Without the protocol (another compositor) the list
//! stays empty and the switcher shows nothing. With workspaces on each
//! screen (M5.2o) the compositor gives each screen a group of its own:
//! a panel shows, and switches, the group of the screen it is on.

use std::sync::Arc;

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::backend::ObjectData;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
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
    groups: Vec<Set>,
}

/// One group: the screens it is on and its workspaces.
struct Set {
    handle: ExtWorkspaceGroupHandleV1,
    outputs: Vec<WlOutput>,
    members: Vec<ExtWorkspaceHandleV1>,
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
            groups: Vec::new(),
        }
    }

    /// The workspaces of the group on `output`, else of the first group
    /// (the one of every screen), in the compositor's order.
    fn on(&self, output: Option<&WlOutput>) -> Vec<&Entry> {
        let set = output
            .and_then(|o| self.groups.iter().find(|g| g.outputs.contains(o)))
            .or(self.groups.first());
        self.list
            .iter()
            .filter(|e| set.is_none_or(|g| g.members.contains(&e.handle)))
            .collect()
    }

    /// Every workspace of the group on `output` by name, the shown one
    /// marked; `None` is the first group's.
    pub fn names(&self, output: Option<&WlOutput>) -> Vec<(String, bool)> {
        self.on(output)
            .into_iter()
            .map(|e| (e.name.clone(), e.active))
            .collect()
    }

    /// Asks the compositor to show the workspace at `index`, its place in
    /// [`Workspaces::names`] for the same `output` (M5.2m, M5.2o).
    pub fn show(&self, output: Option<&WlOutput>, index: usize) {
        let (Some(manager), Some(entry)) = (&self.manager, self.on(output).get(index).copied())
        else {
            return;
        };
        entry.handle.activate();
        manager.commit();
    }

    fn set(&mut self, handle: &ExtWorkspaceGroupHandleV1) -> Option<&mut Set> {
        self.groups.iter_mut().find(|g| &g.handle == handle)
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
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                shell.workspaces.groups.push(Set {
                    handle: workspace_group,
                    outputs: Vec::new(),
                    members: Vec::new(),
                });
            }
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
                shell.workspaces.groups.clear();
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

/// A group's screens and workspaces (M5.2o): every screen's group holds
/// every workspace, a screen's own group that screen's; a group removed
/// is destroyed, as the protocol asks.
impl Dispatch2<ExtWorkspaceGroupHandleV1, Shell> for Group {
    fn event(
        &self,
        shell: &mut Shell,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        let workspaces = &mut shell.workspaces;
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => {
                if let Some(set) = workspaces.set(group) {
                    set.outputs.push(output);
                }
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => {
                if let Some(set) = workspaces.set(group) {
                    set.outputs.retain(|o| *o != output);
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                if let Some(set) = workspaces.set(group) {
                    set.members.push(workspace);
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                if let Some(set) = workspaces.set(group) {
                    set.members.retain(|w| *w != workspace);
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                workspaces.groups.retain(|g| &g.handle != group);
                group.destroy();
            }
            _ => {}
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
                for set in &mut shell.workspaces.groups {
                    set.members.retain(|w| w != handle);
                }
                handle.destroy();
            }
            _ => {}
        }
    }
}
