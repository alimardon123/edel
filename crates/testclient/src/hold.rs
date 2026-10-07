//! `--lock-pointer` and `--inhibit-idle` (roadmap M5.23): the window locks
//! the pointer where it is and prints the mouse's motion as it comes
//! (`locked`, then `relative DX DY`), or asks to keep the screen on, as
//! games and video players do.

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_pointer, wl_seat, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
};
use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::{
    zwp_locked_pointer_v1::{self, ZwpLockedPointerV1},
    zwp_pointer_constraints_v1::{Lifetime, ZwpPointerConstraintsV1},
};
use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};

use crate::Client;

/// Locks the pointer over `surface` and asks for its relative motion.
pub fn lock(
    globals: &GlobalList,
    qh: &QueueHandle<Client>,
    surface: &wl_surface::WlSurface,
) -> Result<()> {
    let seat: wl_seat::WlSeat = globals.bind(qh, 1..=7, ()).context("no wl_seat")?;
    let pointer = seat.get_pointer(qh, ());
    let constraints: ZwpPointerConstraintsV1 = globals
        .bind(qh, 1..=1, ())
        .context("no zwp_pointer_constraints_v1")?;
    let relative: ZwpRelativePointerManagerV1 = globals
        .bind(qh, 1..=1, ())
        .context("no zwp_relative_pointer_manager_v1")?;
    constraints.lock_pointer(surface, &pointer, None, Lifetime::Persistent, qh, ());
    relative.get_relative_pointer(&pointer, qh, ());
    Ok(())
}

/// Asks to keep the screen on while `surface` is shown.
pub fn inhibit(
    globals: &GlobalList,
    qh: &QueueHandle<Client>,
    surface: &wl_surface::WlSurface,
) -> Result<()> {
    let manager: ZwpIdleInhibitManagerV1 = globals
        .bind(qh, 1..=1, ())
        .context("no zwp_idle_inhibit_manager_v1")?;
    manager.create_inhibitor(surface, qh, ());
    Ok(())
}

impl Dispatch<ZwpLockedPointerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_locked_pointer_v1::Event::Locked => println!("locked"),
            zwp_locked_pointer_v1::Event::Unlocked => println!("unlocked"),
            _ => {}
        }
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event {
            println!("relative {dx} {dy}");
        }
    }
}

/// The objects that send nothing the client needs.
macro_rules! quiet {
    ($($t:ty),*) => {
        $(impl Dispatch<$t, ()> for Client {
            fn event(
                _: &mut Self,
                _: &$t,
                _: <$t as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        })*
    };
}

quiet!(
    wl_seat::WlSeat,
    wl_pointer::WlPointer,
    ZwpPointerConstraintsV1,
    ZwpRelativePointerManagerV1,
    ZwpIdleInhibitManagerV1,
    ZwpIdleInhibitorV1
);
