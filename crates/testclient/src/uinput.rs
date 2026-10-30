//! The uinput devices the test client makes (`lid.rs`, `touch.rs`): the
//! kernel's ioctls and structs, and writing one event, written once.

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
pub const UI_SET_EVBIT: u32 = 0x4004_5564;
pub const UI_SET_SWBIT: u32 = 0x4004_556d;
pub const UI_SET_KEYBIT: u32 = 0x4004_5565;
pub const UI_SET_ABSBIT: u32 = 0x4004_5567;
pub const UI_SET_PROPBIT: u32 = 0x4004_556e;
const UI_ABS_SETUP: u32 = 0x401c_5504;

pub const EV_SYN: u16 = 0;
pub const SYN_REPORT: u16 = 0;

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

/// The kernel's `struct uinput_abs_setup`, 28 bytes: an axis and its
/// range, from 0 to `max`, at `resolution` units a millimetre.
#[repr(C)]
struct AbsSetup {
    code: u16,
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// Gives the device's axis `code` the range 0 to `max`, `resolution`
/// units a millimetre, before the device is made.
pub fn axis(fd: RawFd, code: u16, max: i32, resolution: i32) -> Result<()> {
    ioctl(fd, UI_SET_ABSBIT, code as libc::c_long)?;
    let setup = AbsSetup {
        code,
        value: 0,
        minimum: 0,
        maximum: max,
        fuzz: 0,
        flat: 0,
        resolution,
    };
    ioctl(fd, UI_ABS_SETUP, &setup as *const AbsSetup as libc::c_long)
}

/// Opens /dev/uinput, lets `kinds` set its event bits (and whatever else
/// the device has), names it `device_name` and makes it; the device is
/// ready once this returns.
pub fn make(device_name: &[u8], kinds: impl FnOnce(RawFd) -> Result<()>) -> Result<File> {
    let device = OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .context("opening /dev/uinput (run as root, with the uinput module loaded)")?;
    let fd = device.as_raw_fd();
    kinds(fd)?;
    let mut name = [0u8; 80];
    name[..device_name.len()].copy_from_slice(device_name);
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
    Ok(device)
}

/// Takes the device away again, a second after its last event so the
/// compositor has read them.
pub fn unmake(device: &File) -> Result<()> {
    std::thread::sleep(Duration::from_secs(1));
    ioctl(device.as_raw_fd(), UI_DEV_DESTROY, 0)
}

/// Writes one input event, as the kernel's `struct input_event` is laid
/// out, with its time left at zero for the kernel to fill in.
pub fn event(device: &mut File, kind: u16, code: u16, value: i32) -> Result<()> {
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
pub fn ioctl(fd: RawFd, request: u32, arg: libc::c_long) -> Result<()> {
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
    fn abs_setup_is_the_kernels_28_bytes() {
        assert_eq!(std::mem::size_of::<AbsSetup>(), 28);
    }
}
