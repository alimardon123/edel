//! `--taps "X,Y X,Y ..."` (roadmap M9.5): a touch screen on a uinput
//! device as large as the VM's screen, 1280 by 800, that taps each point
//! in turn, a finger down for a tenth of a second and a second between
//! taps, printing `tapped X,Y` after each. The compositor reads it
//! through libinput as a laptop's touch screen, so the desktop test can
//! tap without one. Run as root, as the device is `/dev/uinput`.

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::time::Duration;

use crate::uinput;

const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const BTN_TOUCH: u16 = 0x14a;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
/// A touch screen, whose points lie on the screen (libinput's direct
/// device), not a touchpad's.
const INPUT_PROP_DIRECT: u16 = 0x01;

/// The VM's screen, which the device spans one unit a pixel.
const WIDTH: i32 = 1280;
const HEIGHT: i32 = 800;
/// Units a millimetre: a 13 inch screen's, about 5 pixels a millimetre.
const RESOLUTION: i32 = 5;

/// The points `--taps` was given, each `X,Y` on the screen.
pub fn points(text: &str) -> Result<Vec<(i32, i32)>> {
    let points = text
        .split_whitespace()
        .map(|point| {
            let (x, y) = point
                .split_once(',')
                .with_context(|| format!("--taps takes points as X,Y, not {point}"))?;
            let (x, y): (i32, i32) = (
                x.parse()
                    .with_context(|| format!("{x} in {point} is no number"))?,
                y.parse()
                    .with_context(|| format!("{y} in {point} is no number"))?,
            );
            if !(0..WIDTH).contains(&x) || !(0..HEIGHT).contains(&y) {
                bail!("{point} is off the {WIDTH}x{HEIGHT} screen");
            }
            Ok((x, y))
        })
        .collect::<Result<Vec<_>>>()?;
    if points.is_empty() {
        bail!("--taps takes at least one point, as X,Y");
    }
    Ok(points)
}

/// Taps each of `text`'s points, then takes the touch screen away.
pub fn run(text: &str) -> Result<()> {
    let points = points(text)?;
    let mut device = uinput::make(b"edel test touch screen", |fd| {
        uinput::ioctl(fd, uinput::UI_SET_EVBIT, EV_KEY as libc::c_long)?;
        uinput::ioctl(fd, uinput::UI_SET_KEYBIT, BTN_TOUCH as libc::c_long)?;
        uinput::ioctl(fd, uinput::UI_SET_EVBIT, EV_ABS as libc::c_long)?;
        uinput::ioctl(
            fd,
            uinput::UI_SET_PROPBIT,
            INPUT_PROP_DIRECT as libc::c_long,
        )?;
        uinput::axis(fd, ABS_X, WIDTH - 1, RESOLUTION)?;
        uinput::axis(fd, ABS_Y, HEIGHT - 1, RESOLUTION)?;
        uinput::axis(fd, ABS_MT_SLOT, 1, 0)?;
        uinput::axis(fd, ABS_MT_TRACKING_ID, 65535, 0)?;
        uinput::axis(fd, ABS_MT_POSITION_X, WIDTH - 1, RESOLUTION)?;
        uinput::axis(fd, ABS_MT_POSITION_Y, HEIGHT - 1, RESOLUTION)
    })?;
    for (n, (x, y)) in points.into_iter().enumerate() {
        tap(&mut device, n as i32, x, y)?;
        println!("tapped {x},{y}");
        std::thread::sleep(Duration::from_secs(1));
    }
    uinput::unmake(&device)
}

/// One finger down at `x`,`y` for a tenth of a second and up, as the
/// kernel's multitouch protocol B reports it, with the single-touch axes
/// beside it.
fn tap(device: &mut File, id: i32, x: i32, y: i32) -> Result<()> {
    for (kind, code, value) in [
        (EV_ABS, ABS_MT_SLOT, 0),
        (EV_ABS, ABS_MT_TRACKING_ID, id),
        (EV_ABS, ABS_MT_POSITION_X, x),
        (EV_ABS, ABS_MT_POSITION_Y, y),
        (EV_KEY, BTN_TOUCH, 1),
        (EV_ABS, ABS_X, x),
        (EV_ABS, ABS_Y, y),
        (uinput::EV_SYN, uinput::SYN_REPORT, 0),
    ] {
        uinput::event(device, kind, code, value)?;
    }
    std::thread::sleep(Duration::from_millis(100));
    for (kind, code, value) in [
        (EV_ABS, ABS_MT_TRACKING_ID, -1),
        (EV_KEY, BTN_TOUCH, 0),
        (uinput::EV_SYN, uinput::SYN_REPORT, 0),
    ] {
        uinput::event(device, kind, code, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_are_read_as_x_comma_y() {
        assert_eq!(points("10,20 1279,799").unwrap(), [(10, 20), (1279, 799)]);
    }

    #[test]
    fn points_off_the_screen_or_garbled_are_refused() {
        assert!(points("1280,10").is_err());
        assert!(points("10").is_err());
        assert!(points("a,b").is_err());
        assert!(points("").is_err());
    }
}
