//! The windows as the compositor tells them over
//! wlr-foreign-toplevel-management (M5.2d), for the panel's window list
//! (M5.2h): each with its title, whether it is on a screen (one on a
//! hidden workspace is on none), focused or minimized, in the order the
//! compositor first told of it; and the requests that bring one forward
//! and minimize it. Without the protocol (another compositor) the list
//! stays empty and the window list shows nothing.

use std::sync::Arc;

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::backend::ObjectData;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_seat};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

use crate::Shell;
use crate::widgets::Task;

/// The protocol's states, as its enum numbers them.
const MINIMIZED: u32 = 1;
const ACTIVATED: u32 = 2;

pub struct Toplevels {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    list: Vec<Entry>,
}

struct Entry {
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    screens: Vec<wl_output::WlOutput>,
    focused: bool,
    minimized: bool,
}

impl Toplevels {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Toplevels {
        let manager = globals.bind(qh, 1..=3, Manager).ok();
        if manager.is_none() {
            eprintln!(
                "edel-shell-ui: the compositor offers no zwlr_foreign_toplevel_manager_v1, so no window list"
            );
        }
        Toplevels {
            manager,
            list: Vec::new(),
        }
    }

    /// The windows on a screen, as the window list shows them.
    fn shown(&self) -> impl Iterator<Item = &Entry> {
        self.list.iter().filter(|e| !e.screens.is_empty())
    }

    pub fn tasks(&self) -> Vec<Task> {
        self.shown()
            .map(|e| Task {
                title: e.title.clone(),
                focused: e.focused,
                minimized: e.minimized,
            })
            .collect()
    }

    /// Brings the `i`th window of [`Toplevels::tasks`] forward, back if it
    /// is minimized.
    pub fn activate(&self, i: usize, seat: Option<&wl_seat::WlSeat>) {
        if let (Some(entry), Some(seat)) = (self.shown().nth(i), seat) {
            entry.handle.activate(seat);
        }
    }

    /// Minimizes the `i`th window of [`Toplevels::tasks`].
    pub fn minimize(&self, i: usize) {
        if let Some(entry) = self.shown().nth(i) {
            entry.handle.set_minimized();
        }
    }

    fn entry(&mut self, handle: &ZwlrForeignToplevelHandleV1) -> Option<&mut Entry> {
        self.list.iter_mut().find(|e| &e.handle == handle)
    }
}

/// The objects' user data: each says what its events do to the shell.
pub struct Manager;
pub struct Handle;

impl Dispatch2<ZwlrForeignToplevelManagerV1, Shell> for Manager {
    fn event(
        &self,
        shell: &mut Shell,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                shell.toplevels.list.push(Entry {
                    handle: toplevel,
                    title: String::new(),
                    screens: Vec::new(),
                    focused: false,
                    minimized: false,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                shell.toplevels.manager = None;
                shell.toplevels.list.clear();
                shell.windows_changed();
            }
            _ => {}
        }
    }

    /// The manager's `toplevel` event makes a handle.
    fn event_created_child(_: u16, qh: &QueueHandle<Shell>) -> Arc<dyn ObjectData> {
        qh.make_data::<ZwlrForeignToplevelHandleV1, Handle>(Handle)
    }
}

impl Dispatch2<ZwlrForeignToplevelHandleV1, Shell> for Handle {
    fn event(
        &self,
        shell: &mut Shell,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        match event {
            Event::Title { title } => {
                if let Some(entry) = shell.toplevels.entry(handle) {
                    entry.title = title;
                }
            }
            Event::OutputEnter { output } => {
                if let Some(entry) = shell.toplevels.entry(handle) {
                    entry.screens.push(output);
                }
            }
            Event::OutputLeave { output } => {
                if let Some(entry) = shell.toplevels.entry(handle) {
                    entry.screens.retain(|o| *o != output);
                }
            }
            Event::State { state } => {
                if let Some(entry) = shell.toplevels.entry(handle) {
                    let states: Vec<u32> = state
                        .chunks_exact(4)
                        .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    entry.focused = states.contains(&ACTIVATED);
                    entry.minimized = states.contains(&MINIMIZED);
                }
            }
            // Each change ends with done, so the list is drawn once.
            Event::Done => shell.windows_changed(),
            Event::Closed => {
                shell.toplevels.list.retain(|e| &e.handle != handle);
                handle.destroy();
                shell.windows_changed();
            }
            _ => {}
        }
    }
}
