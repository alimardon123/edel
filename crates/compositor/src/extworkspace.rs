//! ext-workspace-v1 (roadmap M5.2b): the workspaces as panels and docks
//! see them, shell-ui's switcher first. One group holds every workspace,
//! as workspaces span every screen (M5.2a), and enters every screen the
//! client knows; with workspaces on each screen (M5.2k, M5.2o) each screen
//! has a group of its own, entering that screen alone, its active
//! workspace the one that screen shows, and an activate in it switches
//! that screen. Each workspace's id and name are its number from 1, its
//! coordinates its place in a row, and it can only be activated, since
//! the preset makes and removes workspaces. An activate takes effect at
//! the manager's commit, as the protocol asks, and each change ends with
//! `done`. A workspace is called by its name when it has one, else by its
//! number (M5.2i); a workspace closed in the middle renames the ones after
//! it and drops the last, as the protocol has no other way to say so.
//! When the key changes, the groups go and the new ones come.

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

/// What one manager was told: its groups, and the workspace an activate
/// asked for, by its group's screen (none for the group of every screen)
/// and its place, shown at the commit.
struct Manager {
    manager: ExtWorkspaceManagerV1,
    groups: Vec<Group>,
    pending: Option<(Option<String>, usize)>,
}

/// One group: the screen it stands for (none: every screen), its
/// workspaces in order, the name each was last told, and the screens it
/// entered.
struct Group {
    screen: Option<String>,
    group: ExtWorkspaceGroupHandleV1,
    workspaces: Vec<ExtWorkspaceHandleV1>,
    names: Vec<String>,
    outputs: Vec<WlOutput>,
}

/// Offers `ext_workspace_manager_v1` to every client.
pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, ExtWorkspaceManagerV1, ()>(1, ());
}

impl Edel {
    /// Tells every client the workspaces as they are now: after a switch,
    /// a change of their number or name, a screen change, the key of
    /// workspaces on each screen, or a new `wl_output`.
    pub fn announce_workspaces(&mut self) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let labels = self.desks.labels();
        // Each group's screen and the workspace it shows.
        let groups: Vec<(Option<String>, usize)> = if self.desks.per_screen() {
            outputs
                .iter()
                .map(|o| (Some(o.name()), self.desks.shown_on(&o.name())))
                .collect()
        } else {
            vec![(None, self.desks.active())]
        };
        let display = self.display.clone();
        let managers = &mut self.ext_workspaces.0;
        managers.retain(|m| m.manager.is_alive());
        for manager in managers {
            manager.sync(&display, &outputs, &labels, &groups);
        }
    }
}

impl Manager {
    /// Sends what changed since the last `done`, and `done`: the groups
    /// `groups` names, made or taken away as they differ, each with one
    /// workspace for each of `labels`, named by its label, the one it
    /// shows active.
    fn sync(
        &mut self,
        display: &DisplayHandle,
        outputs: &[Output],
        labels: &[String],
        groups: &[(Option<String>, usize)],
    ) {
        let Some(client) = self.manager.client() else {
            return;
        };
        let wanted: Vec<&Option<String>> = groups.iter().map(|(screen, _)| screen).collect();
        let had: Vec<&Option<String>> = self.groups.iter().map(|g| &g.screen).collect();
        if wanted != had {
            for old in self.groups.drain(..) {
                for workspace in &old.workspaces {
                    old.group.workspace_leave(workspace);
                    workspace.removed();
                }
                old.group.removed();
            }
            for (screen, _) in groups {
                let Ok(group) = client.create_resource::<ExtWorkspaceGroupHandleV1, (), Edel>(
                    display,
                    self.manager.version(),
                    (),
                ) else {
                    return;
                };
                self.manager.workspace_group(&group);
                group.capabilities(GroupCapabilities::empty());
                self.groups.push(Group {
                    screen: screen.clone(),
                    group,
                    workspaces: Vec::new(),
                    names: Vec::new(),
                    outputs: Vec::new(),
                });
            }
        }
        for (group, (_, shown)) in self.groups.iter_mut().zip(groups) {
            let now: Vec<WlOutput> = outputs
                .iter()
                .filter(|o| group.screen.as_ref().is_none_or(|name| *name == o.name()))
                .flat_map(|output| output.client_outputs(&client))
                .collect();
            group.sync(display, &self.manager, &client, now, labels, *shown);
        }
        self.manager.done();
    }
}

impl Group {
    /// Brings the group's screens, workspaces, names and active one to
    /// `outputs`, `labels` and `shown`.
    fn sync(
        &mut self,
        display: &DisplayHandle,
        manager: &ExtWorkspaceManagerV1,
        client: &Client,
        outputs: Vec<WlOutput>,
        labels: &[String],
        shown: usize,
    ) {
        for output in &outputs {
            if !self.outputs.contains(output) {
                self.group.output_enter(output);
            }
        }
        for output in &self.outputs {
            if !outputs.contains(output) && output.is_alive() {
                self.group.output_leave(output);
            }
        }
        self.outputs = outputs;
        let count = labels.len();
        while self.workspaces.len() < count {
            let number = self.workspaces.len();
            let Ok(workspace) = client.create_resource::<ExtWorkspaceHandleV1, (), Edel>(
                display,
                manager.version(),
                (),
            ) else {
                return;
            };
            manager.workspace(&workspace);
            workspace.id((number + 1).to_string());
            workspace.name(labels[number].clone());
            self.names.push(labels[number].clone());
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
            self.names.pop();
        }
        for (number, workspace) in self.workspaces.iter().enumerate() {
            if self.names[number] != labels[number] {
                workspace.name(labels[number].clone());
                self.names[number].clone_from(&labels[number]);
            }
            let state = if number == shown {
                State::Active
            } else {
                State::empty()
            };
            workspace.state(state);
        }
    }
}

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for Edel {
    /// Hidden from sandboxed apps: only the shell's (M5.22).
    fn can_view(client: Client, _: &()) -> bool {
        crate::sandbox::shell_may(&client)
    }

    fn bind(
        state: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let manager = data_init.init(resource, ());
        state.ext_workspaces.0.push(Manager {
            manager,
            groups: Vec::new(),
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
            ext_workspace_manager_v1::Request::Commit => match managers[i].pending.take() {
                // A screen's own group switches that screen alone.
                Some((Some(screen), to)) if state.desks.per_screen() => {
                    state.switch_screen(to, &screen);
                }
                Some((_, to)) => state.switch_workspace(to),
                None => {}
            },
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

/// A workspace: only `activate` does anything. It asks for the workspace
/// at the handle's place now in its group, as a workspace closed before
/// it moves the ones after it down (M5.2i).
impl Dispatch<ExtWorkspaceHandleV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        if let ext_workspace_handle_v1::Request::Activate = request {
            // A workspace already removed is inert.
            for manager in &mut state.ext_workspaces.0 {
                let found = manager.groups.iter().find_map(|g| {
                    let at = g.workspaces.iter().position(|w| w == resource)?;
                    Some((g.screen.clone(), at))
                });
                if found.is_some() {
                    manager.pending = found;
                    return;
                }
            }
        }
    }
}
