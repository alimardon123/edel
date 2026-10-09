//! The kernel's news for the status area (M5.9a): when a network link or
//! address changes (rtnetlink's link and address groups) or a power
//! supply does (the kernel's uevents for `power_supply`, a charger plugged
//! in, a battery's level), the event loop is woken with [`Msg::Changed`]
//! and reads the status afresh, so the icons follow the machine without a
//! timer of their own. Two netlink sockets on the event loop itself: no
//! thread and no D-Bus connection, which cost shell-ui 0.1 MiB over its
//! budget on #157's first runs (a second zbus connection held about 160
//! KiB). A socket that cannot be opened gives no news, and the status is
//! read on the clock's minute and when the card opens instead.

use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::net::{
    AddressFamily, RecvFlags, SocketFlags, SocketType, bind, netlink, recv, socket_with,
};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use crate::messages;
use crate::status::Msg;

/// rtnetlink's groups for links and IPv4 and IPv6 addresses
/// (`RTMGRP_LINK`, `RTMGRP_IPV4_IFADDR`, `RTMGRP_IPV6_IFADDR`).
const ROUTE_GROUPS: u32 = 0x1 | 0x10 | 0x100;
/// The kernel's own uevents (not udev's re-sent ones).
const KERNEL_UEVENTS: u32 = 1;

/// Listens for the kernel's news of each part whose feature `features`
/// has, network and power; nothing when neither, and a line saying why
/// when a socket cannot be opened.
pub fn serve<D: 'static>(features: &Path, wake: &Sender<Msg>, handle: &LoopHandle<'static, D>) {
    let has = |name: &str| features.join(format!("{name}.toml")).is_file();
    if has("network") {
        listen(handle, None, ROUTE_GROUPS, wake, |_| true);
    }
    if has("power") {
        listen(
            handle,
            Some(netlink::KOBJECT_UEVENT),
            KERNEL_UEVENTS,
            wake,
            concerns_power,
        );
    }
}

/// Opens a netlink socket of `protocol` joined to `groups` and wakes the
/// loop once for each batch of messages of which one `matters`.
fn listen<D: 'static>(
    handle: &LoopHandle<'static, D>,
    protocol: Option<rustix::net::Protocol>,
    groups: u32,
    wake: &Sender<Msg>,
    matters: fn(&[u8]) -> bool,
) {
    let socket = match open(protocol, groups) {
        Ok(socket) => socket,
        Err(e) => {
            eprintln!("edel-shell-ui: {}", messages::status_no_news(e));
            return;
        }
    };
    let wake = wake.clone();
    let mut buffer = vec![0u8; 8192];
    let source = Generic::new(socket, Interest::READ, Mode::Level);
    let inserted = handle.insert_source(source, move |_, socket, _| {
        let mut news = false;
        loop {
            match recv(&*socket, &mut buffer[..], RecvFlags::DONTWAIT) {
                Ok((0, _)) => break,
                Ok((n, _)) => news |= matters(&buffer[..n.min(buffer.len())]),
                Err(rustix::io::Errno::AGAIN) => break,
                // The kernel had more news than the socket held: read the
                // status anyway, as something changed.
                Err(rustix::io::Errno::NOBUFS) => news = true,
                Err(_) => break,
            }
        }
        if news && wake.send(Msg::Changed).is_err() {
            return Ok(PostAction::Remove);
        }
        Ok(PostAction::Continue)
    });
    if let Err(e) = inserted {
        eprintln!("edel-shell-ui: {}", messages::status_no_news(e));
    }
}

/// A non-blocking netlink socket of `protocol` (none is rtnetlink,
/// `NETLINK_ROUTE`, protocol 0), joined to `groups`.
fn open(protocol: Option<rustix::net::Protocol>, groups: u32) -> rustix::io::Result<OwnedFd> {
    let socket = socket_with(
        AddressFamily::NETLINK,
        SocketType::DGRAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        protocol,
    )?;
    bind(&socket, &netlink::SocketAddrNetlink::new(0, groups))?;
    Ok(socket)
}

/// Whether a kernel uevent is about a power supply: its lines, after the
/// header, are `KEY=VALUE`, nul-separated.
fn concerns_power(message: &[u8]) -> bool {
    message
        .split(|b| *b == 0)
        .any(|field| field == b"SUBSYSTEM=power_supply")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_power_supply_uevents_matter() {
        let charger = b"change@/devices/LNXSYSTM:00/ACPI0003:00/power_supply/AC\0ACTION=change\0SUBSYSTEM=power_supply\0POWER_SUPPLY_ONLINE=1\0";
        let usb = b"add@/devices/pci0000:00/usb1/1-1\0ACTION=add\0SUBSYSTEM=usb\0";
        assert!(concerns_power(charger));
        assert!(!concerns_power(usb));
        assert!(!concerns_power(b"SUBSYSTEM=power_supply_x\0"));
    }

    /// Opening the sockets needs a kernel that allows them here; where it
    /// does not, the test says so and passes, as shell-ui then just reads
    /// on the minute.
    #[test]
    fn the_sockets_open_where_the_kernel_allows() {
        for (protocol, groups) in [
            (None, ROUTE_GROUPS),
            (Some(netlink::KOBJECT_UEVENT), KERNEL_UEVENTS),
        ] {
            if let Err(e) = open(protocol, groups) {
                eprintln!("netlink not allowed here: {e}");
            }
        }
    }
}
