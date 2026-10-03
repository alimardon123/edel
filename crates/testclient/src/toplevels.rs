//! `edel-testclient --toplevels [TITLE]` (roadmap M5.2d): lists the windows
//! over wlr-foreign-toplevel-management, as the panel's window list does,
//! by title, the focused one starred and one on no screen (a hidden
//! workspace's) marked `-`: `toplevels one* away-`. Given a title, it then
//! activates that window and lists them again once it is focused.

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::reexports::client::globals::{GlobalListContents, registry_queue_init};
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_registry, wl_seat};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle, event_created_child,
};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

/// The protocol's activated state.
const ACTIVATED: u32 = 2;

struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    screens: usize,
    activated: bool,
}

#[derive(Default)]
struct Seen {
    list: Vec<Toplevel>,
}

impl Seen {
    fn line(&self) -> String {
        let names: Vec<String> = self
            .list
            .iter()
            .map(|t| {
                let mark = match (t.activated, t.screens) {
                    (true, _) => "*",
                    (false, 0) => "-",
                    _ => "",
                };
                format!("{}{mark}", t.title.replace(' ', "_"))
            })
            .collect();
        format!("toplevels {}", names.join(" "))
    }

    fn find(&mut self, handle: &ZwlrForeignToplevelHandleV1) -> Option<&mut Toplevel> {
        self.list.iter_mut().find(|t| &t.handle == handle)
    }
}

pub fn run(activate: Option<&str>) -> Result<()> {
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, mut queue) =
        registry_queue_init::<Seen>(&connection).context("reading the globals")?;
    let qh = queue.handle();
    // The screens first, so the compositor can say which each window is on.
    let outputs: Vec<u32> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == "wl_output")
            .map(|g| g.name)
            .collect()
    });
    for name in outputs {
        globals
            .registry()
            .bind::<wl_output::WlOutput, _, _>(name, 1, &qh, ());
    }
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).context("no wl_seat")?;
    let _manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .context("the compositor offers no zwlr_foreign_toplevel_manager_v1")?;
    let mut seen = Seen::default();
    queue.roundtrip(&mut seen)?;
    queue.roundtrip(&mut seen)?;
    println!("{}", seen.line());
    let Some(title) = activate else {
        return Ok(());
    };
    let Some(target) = seen.list.iter().find(|t| t.title == title) else {
        bail!("no window is called {title}");
    };
    let handle = target.handle.clone();
    handle.activate(&seat);
    while !seen.list.iter().any(|t| t.handle == handle && t.activated) {
        queue.blocking_dispatch(&mut seen)?;
    }
    queue.roundtrip(&mut seen)?;
    println!("{}", seen.line());
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

impl Dispatch<wl_output::WlOutput, ()> for Seen {
    fn event(
        _: &mut Seen,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Seen {
    fn event(
        _: &mut Seen,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Seen {
    fn event(
        seen: &mut Seen,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            seen.list.push(Toplevel {
                handle: toplevel,
                title: String::new(),
                screens: 0,
                activated: false,
            });
        }
    }

    event_created_child!(Seen, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Seen {
    fn event(
        seen: &mut Seen,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Seen>,
    ) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                seen.list.retain(|t| &t.handle != handle);
            }
            event => {
                let Some(t) = seen.find(handle) else {
                    return;
                };
                match event {
                    zwlr_foreign_toplevel_handle_v1::Event::Title { title } => t.title = title,
                    zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { .. } => t.screens += 1,
                    zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { .. } => {
                        t.screens = t.screens.saturating_sub(1)
                    }
                    zwlr_foreign_toplevel_handle_v1::Event::State { state } => {
                        t.activated = state
                            .chunks_exact(4)
                            .any(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) == ACTIVATED);
                    }
                    _ => {}
                }
            }
        }
    }
}
