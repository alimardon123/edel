//! `--lid SECONDS` (roadmap M7.8): a lid switch on a uinput device, shut
//! for SECONDS and opened again, as a laptop's lid reports itself. The
//! compositor reads it through libinput like any switch, so the desktop
//! test can close the lid of its VM without a lid. Run as root, as the
//! device is `/dev/uinput`.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;

// The uinput ioctls and the kernel's input_event come from libc: the
// client has no input library, and these are the kernel's own numbers.
const UI_DEV_CREATE: u32 = 0x5501;
const UI_DEV_DESTROY: u32 = 0x5502;
const UI_DEV_SETUP: u32 = 0x405c_5503;
const UI_SET_EVBIT: u32 = 0x4004_5564;
const UI_SET_SWBIT: u32 = 0x4004_556d;

const EV_SYN: u16 = 0;
const SYN_REPORT: u16 = 0;
const EV_SW: u16 = 5;
const SW_LID: u16 = 0;

/// The kernel's `struct uinput_setup`, 92 bytes.
#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

/// The kernel's `struct input_id`. BUS_VIRTUAL, and ids of the test's own.
#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

/// Shuts the lid for `seconds` (a number, as `--lid` takes it), then opens
/// it, printing what it did, which the test service's log keeps.
pub fn run(seconds: &str) -> Result<()> {
    let seconds: u64 = seconds.parse().context("--lid is a number of seconds")?;
    let mut device = OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .context("opening /dev/uinput (run as root, with the uinput module loaded)")?;
    let fd = device.as_raw_fd();
    ioctl(fd, UI_SET_EVBIT, EV_SW as libc::c_long)?;
    ioctl(fd, UI_SET_SWBIT, SW_LID as libc::c_long)?;
    let mut name = [0u8; 80];
    name[..b"edel test lid".len()].copy_from_slice(b"edel test lid");
    let setup = UinputSetup {
        id: InputId {
            bustype: 0x06, // BUS_VIRTUAL
            vendor: 0xed0e,
            product: 0x0001,
            version: 1,
        },
        name,
        ff_effects_max: 0,
    };
    ioctl(
        fd,
        UI_DEV_SETUP,
        &setup as *const UinputSetup as libc::c_long,
    )?;
    ioctl(fd, UI_DEV_CREATE, 0)?;
    // udev and libinput take a new device a moment to notice.
    std::thread::sleep(Duration::from_secs(1));
    switch(&mut device, 1)?;
    println!("lid closed");
    std::thread::sleep(Duration::from_secs(seconds));
    switch(&mut device, 0)?;
    println!("lid opened");
    std::thread::sleep(Duration::from_secs(1));
    ioctl(fd, UI_DEV_DESTROY, 0)?;
    Ok(())
}

/// Sends the lid's state (1 shut, 0 open) and the report that ends it.
fn switch(device: &mut File, value: i32) -> Result<()> {
    event(device, EV_SW, SW_LID, value)?;
    event(device, EV_SYN, SYN_REPORT, 0)
}

/// Writes one input event, as the kernel's `struct input_event` is laid
/// out, with its time left at zero for the kernel to fill in.
fn event(device: &mut File, kind: u16, code: u16, value: i32) -> Result<()> {
    let event = libc::input_event {
        time: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        type_: kind,
        code,
        value,
    };
    // SAFETY: the event is plain old data with no padding that matters to
    // the kernel, and the slice covers exactly its bytes, for the write
    // that follows while the event is still alive.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &event as *const libc::input_event as *const u8,
            std::mem::size_of::<libc::input_event>(),
        )
    };
    device
        .write_all(bytes)
        .context("writing an event to /dev/uinput")
}

/// One ioctl on the uinput device; `arg` is the number or the address the
/// request takes.
fn ioctl(fd: RawFd, request: u32, arg: libc::c_long) -> Result<()> {
    // SAFETY: fd is the open uinput device, and every request here reads
    // an integer or one struct that the caller keeps alive for this call.
    let rc = unsafe { libc::ioctl(fd, request as _, arg) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("ioctl {request:#x} on /dev/uinput"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_is_the_kernels_92_bytes() {
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
    }

    #[test]
    fn seconds_must_be_a_number() {
        assert!(run("soon").is_err());
    }
}
