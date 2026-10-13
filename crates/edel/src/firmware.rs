//! The firmware the kernel asked for and could not find, read from the kernel
//! log (M5.30c), shared by `edel report`, `edel status` and Settings' Sound
//! page (M6.15).

use std::time::Duration;

use serde::Serialize;

use crate::tool;

/// How long reading the kernel log may take before it is given up.
const READ_TIME: Duration = Duration::from_secs(5);

/// The start of the path of each firmware file a sound card needs: Intel's
/// sound processors above all (Sound Open Firmware).
const SOUND_FILES: [&str; 5] = [
    "intel/sof",
    "intel/avs",
    "intel/dsp_fw",
    "intel/fw_sst",
    "intel/IntcSST",
];

/// The start of the name of each driver of a sound card.
const SOUND_DRIVERS: [&str; 2] = ["sof", "snd"];

/// One firmware file the kernel asked for and this system does not carry
/// (M5.30c): the device that asked and the file.
#[derive(Debug, PartialEq, Serialize)]
pub struct Missing {
    pub driver: String,
    pub device: String,
    pub file: String,
}

/// The firmware the kernel asked for and could not find, from `dmesg`'s
/// lines `DRIVER DEVICE: Direct firmware load for FILE failed with error -2`
/// and `DRIVER DEVICE: firmware: failed to load FILE (-2)` (M5.30c). Each
/// file once, in the order first seen; a file that exists but failed with
/// another error is not missing, so it is left out.
pub fn missing_firmware(dmesg: &str) -> Vec<Missing> {
    let mut found: Vec<Missing> = Vec::new();
    for missing in dmesg.lines().filter_map(missing_line) {
        if !found.iter().any(|f| f.file == missing.file) {
            found.push(missing);
        }
    }
    found
}

/// The missing firmware one kernel log line names, when it is a load that
/// failed for want of the file (error -2); a timestamp in front is skipped.
fn missing_line(line: &str) -> Option<Missing> {
    let line = match line.strip_prefix('[') {
        Some(rest) => rest.split_once("] ")?.1,
        None => line,
    };
    let (who, what) = line.split_once(": ")?;
    let mut words = who.split_whitespace();
    let (driver, device) = (words.next()?, words.next()?);
    let file = match what.strip_prefix("Direct firmware load for ") {
        Some(rest) => rest.strip_suffix(" failed with error -2")?,
        None => what
            .strip_prefix("firmware: failed to load ")?
            .strip_suffix(" (-2)")?,
    };
    Some(Missing {
        driver: driver.into(),
        device: device.into(),
        file: file.into(),
    })
}

/// The missing firmware a sound card needs, Intel's sound processors above
/// all (Sound Open Firmware): the files under `intel/sof` and the other Intel
/// sound paths, and whatever a sound driver (`sof`, `snd`) asked for.
pub fn for_sound(missing: &[Missing]) -> Vec<&Missing> {
    missing
        .iter()
        .filter(|m| {
            SOUND_FILES.iter().any(|p| m.file.starts_with(p))
                || SOUND_DRIVERS.iter().any(|p| m.driver.starts_with(p))
        })
        .collect()
}

/// The kernel log, from busybox's `dmesg`, within five seconds; None when it
/// cannot run or exits non-zero, as it does for a person the kernel keeps the
/// log from (such as a person who is not root).
pub fn read_kernel_log() -> Option<String> {
    let done = tool::run("dmesg", &[], READ_TIME).ok()?;
    if done.ok() { Some(done.out) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_missing_firmware_from_both_kernel_messages_once_each() {
        let dmesg = "\
[    0.000000] Linux version 6.18.54
[    1.234567] i915 0000:00:02.0: Direct firmware load for i915/kbl_dmc_ver1_04.bin failed with error -2
i915 0000:00:02.0: firmware: failed to load i915/kbl_dmc_ver1_04.bin (-2)
[    2.000000] iwlwifi 0000:00:14.3: Direct firmware load for iwlwifi-so-a0-gf-a0-90.ucode failed with error -2
[    2.000001] iwlwifi 0000:00:14.3: Direct firmware load for iwlwifi-so-a0-gf-a0-90.ucode failed with error -12
[    3.000000] ath10k_pci 0000:02:00.0: Direct firmware load for ath10k/board.bin failed with error -12
i915 0000:00:02.0: firmware: failed to load i915/tgl_dmc_ver2_12.bin (-2)
[    4.000000] usb 1-1: new high-speed USB device
";
        let found = missing_firmware(dmesg);
        assert_eq!(
            found,
            [
                Missing {
                    driver: "i915".into(),
                    device: "0000:00:02.0".into(),
                    file: "i915/kbl_dmc_ver1_04.bin".into(),
                },
                Missing {
                    driver: "iwlwifi".into(),
                    device: "0000:00:14.3".into(),
                    file: "iwlwifi-so-a0-gf-a0-90.ucode".into(),
                },
                Missing {
                    driver: "i915".into(),
                    device: "0000:00:02.0".into(),
                    file: "i915/tgl_dmc_ver2_12.bin".into(),
                },
            ]
        );
        assert!(missing_firmware("[    0.1] Linux version 6.18.54\n").is_empty());
    }

    #[test]
    fn for_sound_keeps_the_sound_firmware_only() {
        let dmesg = "\
sof-audio-pci-intel-tgl 0000:00:1f.3: Direct firmware load for intel/sof/sof-tgl.ri failed with error -2
i915 0000:00:02.0: Direct firmware load for i915/tgl_dmc.bin failed with error -2
";
        let missing = missing_firmware(dmesg);
        let sound = for_sound(&missing);
        assert_eq!(sound.len(), 1);
        assert_eq!(sound[0].file, "intel/sof/sof-tgl.ri");
        assert_eq!(sound[0].driver, "sof-audio-pci-intel-tgl");
        assert!(for_sound(&[]).is_empty());
    }
}
