//! Which hardware the machine has, as the kernel names it, and which
//! hardware packs serve it (M7.11, ADR-012). Matching is a pure function
//! and nothing is sent anywhere (ADR-012 section 6). Std only, not behind
//! `os`, since shell-ui reads the packs file this module reads.

use std::fs;
use std::path::Path;

/// Where the kernel lists the devices of every bus.
pub const BUS: &str = "/sys/bus";
/// The machine's own DMI identity as one modalias string.
pub const DMI_MODALIAS: &str = "/sys/class/dmi/id/modalias";
/// Where the kernel loads firmware from.
pub const FIRMWARE_DIR: &str = "/usr/lib/firmware";

/// One device of the machine, with the modalias the kernel gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The bus (`pci`, `usb`), or `dmi` for the machine itself.
    pub bus: String,
    /// The device's name on its bus (`0000:01:00.0`), or `id` for DMI.
    pub name: String,
    pub modalias: String,
}

/// A pack this machine's hardware matches and the machine has not chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub name: String,
    pub summary: String,
}

/// Whether `name` matches `pattern`: `*` is any run of characters, `?` one
/// character. With `slash_stops`, neither crosses a `/` (paths, as `keep`
/// matches them); modalias strings are matched without that, as a DMI
/// field can hold one (a date such as `03/25/2021`).
pub fn glob(pattern: &str, name: &str, slash_stops: bool) -> bool {
    fn go(p: &[u8], n: &[u8], slash_stops: bool) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => (0..=n.len())
                .take_while(|&i| i == 0 || !(slash_stops && n[i - 1] == b'/'))
                .any(|i| go(rest, &n[i..], slash_stops)),
            Some((b'?', rest)) => {
                n.first().is_some_and(|&c| !(slash_stops && c == b'/'))
                    && go(rest, &n[1..], slash_stops)
            }
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..], slash_stops),
        }
    }
    go(pattern.as_bytes(), name.as_bytes(), slash_stops)
}

/// Whether any device's modalias matches any of the packs' `patterns`.
pub fn serves(patterns: &[String], devices: &[Device]) -> bool {
    devices
        .iter()
        .any(|d| patterns.iter().any(|p| glob(p, &d.modalias, false)))
}

/// Whether the firmware file `file` is under `dir`, as itself or
/// compressed (`.zst` or `.xz`), which the kernel loads either way.
pub fn has_firmware(dir: &Path, file: &str) -> bool {
    [
        file.to_string(),
        format!("{file}.zst"),
        format!("{file}.xz"),
    ]
    .iter()
    .any(|name| dir.join(name).is_file())
}

/// The names in `dir`, sorted; none when it cannot be read.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A file's text without its surrounding white space; None when it cannot
/// be read or is empty.
fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Every device under `bus` (`BUS/*/devices/*`, its `modalias`) and the
/// machine's DMI identity at `dmi`, sorted by bus and name. Unreadable
/// entries are skipped.
pub fn devices_in(bus: &Path, dmi: &Path) -> Vec<Device> {
    let mut found = Vec::new();
    for bus_name in names(bus) {
        let devices = bus.join(&bus_name).join("devices");
        for name in names(&devices) {
            if let Some(modalias) = read_trimmed(&devices.join(&name).join("modalias")) {
                found.push(Device {
                    bus: bus_name.clone(),
                    name,
                    modalias,
                });
            }
        }
    }
    if let Some(modalias) = read_trimmed(dmi) {
        found.push(Device {
            bus: "dmi".into(),
            name: "id".into(),
            modalias,
        });
    }
    found.sort_by(|a, b| (&a.bus, &a.name).cmp(&(&b.bus, &b.name)));
    found
}

/// The machine's devices now, as the kernel names them.
pub fn devices() -> Vec<Device> {
    devices_in(Path::new(BUS), Path::new(DMI_MODALIAS))
}

/// The packs file's text: one `NAME`, a tab and the summary per pack. A
/// tab or line break in a summary becomes a space, so a line is a pack.
pub fn packs_text(packs: &[Pack]) -> String {
    packs
        .iter()
        .map(|p| {
            let summary: String = p
                .summary
                .chars()
                .map(|c| {
                    if matches!(c, '\t' | '\n' | '\r') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            format!("{}\t{summary}\n", p.name)
        })
        .collect()
}

/// The packs [`packs_text`] wrote. Lenient: a line with no tab, or with no
/// name before it, is skipped.
pub fn read_packs(text: &str) -> Vec<Pack> {
    text.lines()
        .filter_map(|line| {
            let (name, summary) = line.split_once('\t')?;
            (!name.is_empty()).then(|| Pack {
                name: name.to_string(),
                summary: summary.to_string(),
            })
        })
        .collect()
}

/// Asks the kernel to start again each device under `bus` whose modalias
/// the mounted packs' `patterns` match and that has no driver, by writing
/// its name to the bus's `drivers_probe` (M7.11). A device with a driver is
/// left alone: a driver that started without its firmware keeps running as
/// it started, and a graphics card is never unbound, as the machine may
/// have no other screen. Returns one log line for each device it tried.
pub fn start_again_at(bus: &Path, patterns: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    if patterns.is_empty() {
        return lines;
    }
    for bus_name in names(bus) {
        let devices = bus.join(&bus_name).join("devices");
        for name in names(&devices) {
            let folder = devices.join(&name);
            let Some(modalias) = read_trimmed(&folder.join("modalias")) else {
                continue;
            };
            let matched = patterns.iter().any(|p| glob(p, &modalias, false));
            let bound = fs::symlink_metadata(folder.join("driver")).is_ok();
            if !matched || bound {
                continue;
            }
            match fs::write(bus.join(&bus_name).join("drivers_probe"), &name) {
                Ok(()) => lines.push(format!(
                    "asked the kernel to start {name} again, now a hardware pack is here"
                )),
                Err(err) => lines.push(format!("could not start {name} again: {err}")),
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn temp(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-hardware-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A PCI device of `bus` with its modalias; returns its folder.
    fn pci(bus: &Path, name: &str, modalias: &str) -> PathBuf {
        let folder = bus.join("pci/devices").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("modalias"), format!("{modalias}\n")).unwrap();
        folder
    }

    #[test]
    fn a_glob_matches_runs_and_single_characters() {
        assert!(glob(
            "pci:v000010DEd*bc03*",
            "pci:v000010DEd00001EB8sv0bc03sc00",
            false
        ));
        assert!(!glob(
            "pci:v000010DEd*bc03*",
            "pci:v00008086d00001234bc03",
            false
        ));
        assert!(glob(
            "iwlwifi-?u-*",
            "iwlwifi-Qu-b0-hr-b0-77.ucode.zst",
            true
        ));
        assert!(!glob("i915/*", "i915/sub/x.bin", true));
        assert!(glob("i915/*", "i915/sub/x.bin", false));
        assert!(glob(
            "dmi:*svnHP*",
            "dmi:bvnAMI:bvr1.0:bd03/25/2021:svnHP:pnHP250G8",
            false
        ));
        assert!(!glob(
            "dmi:*svnHP*",
            "dmi:bvnAMI:bd03/25/2021:svnDell",
            false
        ));
    }

    #[test]
    fn a_pack_serves_a_machine_with_a_matching_device() {
        let devices = vec![Device {
            bus: "pci".into(),
            name: "0000:01:00.0".into(),
            modalias: "pci:v000010DEd00001EB8sv0bc03sc00".into(),
        }];
        assert!(serves(&["pci:v000010DEd*bc03*".into()], &devices));
        assert!(!serves(&["pci:v00001AF4d*".into()], &devices));
        assert!(!serves(&[], &devices));
        assert!(!serves(&["pci:v000010DEd*".into()], &[]));
    }

    #[test]
    fn firmware_counts_as_present_compressed_or_not() {
        let dir = temp("firmware");
        fs::create_dir_all(dir.join("nvidia/ga10x")).unwrap();
        fs::write(dir.join("nvidia/ga10x/gsp.bin.zst"), "").unwrap();
        fs::write(dir.join("plain.bin"), "").unwrap();
        assert!(has_firmware(&dir, "nvidia/ga10x/gsp.bin"));
        assert!(has_firmware(&dir, "plain.bin"));
        assert!(!has_firmware(&dir, "nvidia/ga10x/other.bin"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn devices_are_read_from_each_bus_sorted_by_bus_then_name() {
        let dir = temp("devices");
        let bus = dir.join("bus");
        pci(&bus, "0000:00:02.0", "pci:v00008086d00009A49bc03");
        pci(&bus, "0000:00:1f.0", "");
        let dmi = dir.join("modalias");
        fs::write(&dmi, "dmi:bvnHP:svnHP\n").unwrap();
        assert_eq!(
            devices_in(&bus, &dmi),
            [
                Device {
                    bus: "dmi".into(),
                    name: "id".into(),
                    modalias: "dmi:bvnHP:svnHP".into(),
                },
                Device {
                    bus: "pci".into(),
                    name: "0000:00:02.0".into(),
                    modalias: "pci:v00008086d00009A49bc03".into(),
                },
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn packs_round_trip_and_a_summary_stays_on_one_line() {
        let packs = vec![
            Pack {
                name: "graphics-nvidia".into(),
                summary: "NVIDIA graphics:\tNVK\nand firmware".into(),
            },
            Pack {
                name: "ci-hang".into(),
                summary: "Hangs the boot".into(),
            },
        ];
        let text = packs_text(&packs);
        assert_eq!(text.lines().count(), 2);
        assert_eq!(
            read_packs(&text),
            [
                Pack {
                    name: "graphics-nvidia".into(),
                    summary: "NVIDIA graphics: NVK and firmware".into(),
                },
                Pack {
                    name: "ci-hang".into(),
                    summary: "Hangs the boot".into(),
                },
            ]
        );
        assert_eq!(packs_text(&[]), "");
        assert!(read_packs("no tab here\n\tno name\n").is_empty());
    }

    #[test]
    fn only_a_device_without_a_driver_is_probed_and_a_bound_one_is_left_alone() {
        let dir = temp("start");
        let bus = dir.join("bus");
        let patterns = ["pci:v00001AF4d*".to_string()];
        // Matches and has no driver: probed once.
        pci(&bus, "0000:00:03.0", "pci:v00001AF4d00001000sv00001AF4");
        // Matches and is bound to a driver: left alone.
        let bound = pci(&bus, "0000:00:04.0", "pci:v00001AF4d00001041sv00001AF4");
        symlink("../../../drivers/virtio-pci", bound.join("driver")).unwrap();
        // A graphics card, bound and matching: never unbound.
        let card = pci(&bus, "0000:00:05.0", "pci:v00001AF4d00001050sv00001AF4");
        symlink("../../../drivers/virtio-gpu", card.join("driver")).unwrap();
        fs::create_dir_all(card.join("drm")).unwrap();
        // Does not match: left alone.
        pci(&bus, "0000:00:01.0", "pci:v00008086d00009A49bc03");
        let probe = bus.join("pci/drivers_probe");
        fs::write(&probe, "").unwrap();
        assert_eq!(
            start_again_at(&bus, &patterns),
            ["asked the kernel to start 0000:00:03.0 again, now a hardware pack is here"]
        );
        assert_eq!(fs::read_to_string(&probe).unwrap(), "0000:00:03.0");
        assert!(start_again_at(&bus, &[]).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_probe_is_named_and_the_others_still_run() {
        let dir = temp("probe-fails");
        let bus = dir.join("bus");
        pci(&bus, "0000:00:03.0", "pci:v00001AF4d00001000sv00001AF4");
        // A folder where the kernel's file is: the write to it fails.
        fs::create_dir_all(bus.join("pci/drivers_probe")).unwrap();
        let lines = start_again_at(&bus, &["pci:v00001AF4d*".to_string()]);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("could not start 0000:00:03.0 again: "),
            "{}",
            lines[0]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_modalias_glob_crosses_a_slash_but_a_path_glob_does_not() {
        assert!(glob("dmi:*bd*/*", "dmi:bd03/25/2021", false));
        assert!(!glob("dmi:*bd*/*", "dmi:bd03/25/2021", true));
    }
}
