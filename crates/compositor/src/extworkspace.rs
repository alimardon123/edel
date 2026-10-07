//! ext-workspace-v1 (roadmap M5.2b): the workspaces as panels and docks
//! see them, shell-ui's switcher first. One group holds every workspace,
//! as workspaces span every screen (M5.2a), and enters every screen the
//! client knows; each workspace's id and name are its number from 1, its
//! coordinates its place in a row, and it can only be activated, since
//! the preset makes and removes workspaces. An activate takes effect at
//! the manager's commit, as the protocol asks, and each change ends with
//! `done`.

use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::ext_workspace_group_handle_v1::{
    self, ExtWorkspaceGroupHandleV1, GroupCapabilities,
};
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State, WorkspaceCapabilities,
};
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::state::Edel;

/// Every client's manager.
#[derive(Default)]
pub struct Managers(Vec<Manager>);

/// What one manager was told: its one group, the workspaces by number
/// from 0, and the screens the group entered.
struct Manager {
    manager: ExtWorkspaceManagerV1,
    group: ExtWorkspaceGroupHandleV1,
    workspaces: Vec<ExtWorkspaceHandleV1>,
    outputs: Vec<WlOutput>,
    /// The workspace an activate asked for, shown at the commit.
    pending: Option<usize>,
}

/// Offers `ext_workspace_manager_v1` to every client.
pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, ExtWorkspaceManagerV1, ()>(1, ());
}

impl Edel {
    /// Tells every client the workspaces as they are now: after a switch,
    /// a change of their number, a screen change or a new `wl_output`.
    pub fn announce_workspaces(&mut self) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let (count, shown) = (self.desks.count(), self.desks.active());
        let display = self.display.clone();
        let managers = &mut self.ext_workspaces.0;
        managers.retain(|m| m.manager.is_alive());
        for manager in managers {
            manager.sync(&display, &outputs, count, shown);
        }
    }
}

impl Manager {
    /// Sends what changed since the last `done`, and `done`.
    fn sync(&mut self, display: &DisplayHandle, outputs: &[Output], count: usize, shown: usize) {
        let Some(client) = self.manager.client() else {
            return;
        };
        let now: Vec<WlOutput> = outputs
            .iter()
            .flat_map(|output| output.client_outputs(&client))
            .collect();
        for output in &now {
            if !self.outputs.contains(output) {
                self.group.output_enter(output);
            }
        }
        for output in &self.outputs {
            if !now.contains(output) && output.is_alive() {
                self.group.output_leave(output);
            }
        }
        self.outputs = now;
        while self.workspaces.len() < count {
            let number = self.workspaces.len();
            let Ok(workspace) = client.create_resource::<ExtWorkspaceHandleV1, usize, Edel>(
                display,
                self.manager.version(),
                number,
            ) else {
                return;
            };
            self.manager.workspace(&workspace);
            let name = (number + 1).to_string();
            workspace.id(name.clone());
            workspace.name(name);
            workspace.coordinates((number as u32).to_ne_bytes().to_vec());
            workspace.capabilities(WorkspaceCapabilities::Activate);
            self.group.workspace_enter(&workspace);
            self.workspaces.push(workspace);
        }
        while self.workspaces.len() > count {
            if let Some(workspace) = self.workspaces.pop() {
                self.group.workspace_leave(&workspace);
                workspace.removed();
            }
        }
        for (number, workspace) in self.workspaces.iter().enumerate() {
            let state = if number == shown {
                State::Active
            } else {
                State::empty()
            };
            workspace.state(state);
        }
        self.manager.done();
    }
}

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for Edel {
    /// Hidden from sandboxed apps: only the shell's (M5.22).
    fn can_view(client: Client, _: &()) -> bool {
        crate::sandbox::shell_may(&client)
    }

    fn bind(
        state: &mut Edel,
        display: &DisplayHandle,
        client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let manager = data_init.init(resource, ());
        let Ok(group) = client.create_resource::<ExtWorkspaceGroupHandleV1, (), Edel>(
            display,
            manager.version(),
            (),
        ) else {
            return;
        };
        manager.workspace_group(&group);
        group.capabilities(GroupCapabilities::empty());
        state.ext_workspaces.0.push(Manager {
            manager,
            group,
            workspaces: Vec::new(),
            outputs: Vec::new(),
            pending: None,
        });
        state.announce_workspaces();
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ExtWorkspaceManagerV1,
        request: ext_workspace_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        let managers = &mut state.ext_workspaces.0;
        let Some(i) = managers.iter().position(|m| &m.manager == resource) else {
            return;
        };
        match request {
            ext_workspace_manager_v1::Request::Commit => {
                if let Some(to) = managers[i].pending.take() {
                    state.switch_workspace(to);
                }
            }
            ext_workspace_manager_v1::Request::Stop => {
                managers.remove(i).manager.finished();
            }
            _ => {}
        }
    }
}

/// The group offers no requests but `destroy`: workspaces come from the
/// preset.
impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Edel {
    fn request(
        _: &mut Edel,
        _: &Client,
        _: &ExtWorkspaceGroupHandleV1,
        _: ext_workspace_group_handle_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
    }
}

/// A workspace, by its number from 0: only `activate` does anything.
impl Dispatch<ExtWorkspaceHandleV1, usize> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        number: &usize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        if let ext_workspace_handle_v1::Request::Activate = request {
            // A workspace already removed is inert.
            if let Some(manager) = state
                .ext_workspaces
                .0
                .iter_mut()
                .find(|m| m.workspaces.contains(resource))
            {
                manager.pending = Some(*number);
            }
        }
    }
}
