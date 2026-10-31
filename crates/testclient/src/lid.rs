//! `--lid SECONDS` (roadmap M7.8): a lid switch on a uinput device, shut
//! for SECONDS and opened again, as a laptop's lid reports itself. The
//! compositor reads it through libinput like any switch, so the desktop
//! test can close the lid of its VM without a lid. `--tablet-mode SECONDS`
//! does the same with a 2-in-1's tablet-mode switch (M9.6), on for SECONDS
//! and off again. Run as root, as the device is `/dev/uinput`.

use anyhow::{Context, Result};
use std::fs::File;
use std::time::Duration;

use crate::uinput;

const EV_SW: u16 = 5;
const SW_LID: u16 = 0;
const SW_TABLET_MODE: u16 = 1;

/// Which switch the device has.
#[derive(Clone, Copy)]
pub enum Switch {
    Lid,
    TabletMode,
}

impl Switch {
    /// The kernel's code, the device's name, the flag's name, and what is
    /// printed when it goes on and off.
    fn of(self) -> (u16, &'static [u8], &'static str, &'static str, &'static str) {
        match self {
            Switch::Lid => (
                SW_LID,
                b"edel test lid",
                "--lid",
                "lid closed",
                "lid opened",
            ),
            Switch::TabletMode => (
                SW_TABLET_MODE,
                b"edel test tablet mode",
                "--tablet-mode",
                "tablet mode on",
                "tablet mode off",
            ),
        }
    }
}

/// Turns `which` on for `seconds` (a number, as `--lid` takes it), then
/// off, printing what it did, which the test service's log keeps.
pub fn run(seconds: &str, which: Switch) -> Result<()> {
    let (code, device_name, flag, on, off) = which.of();
    let seconds: u64 = seconds
        .parse()
        .with_context(|| format!("{flag} is a number of seconds"))?;
    let mut device = uinput::make(device_name, |fd| {
        uinput::ioctl(fd, uinput::UI_SET_EVBIT, EV_SW as libc::c_long)?;
        uinput::ioctl(fd, uinput::UI_SET_SWBIT, code as libc::c_long)
    })?;
    switch(&mut device, code, 1)?;
    println!("{on}");
    std::thread::sleep(Duration::from_secs(seconds));
    switch(&mut device, code, 0)?;
    println!("{off}");
    uinput::unmake(&device)
}

/// Sends the switch's state (1 on, 0 off) and the report that ends it.
fn switch(device: &mut File, code: u16, value: i32) -> Result<()> {
    uinput::event(device, EV_SW, code, value)?;
    uinput::event(device, uinput::EV_SYN, uinput::SYN_REPORT, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_must_be_a_number() {
        assert!(run("soon", Switch::Lid).is_err());
        assert!(run("soon", Switch::TabletMode).is_err());
    }
}
