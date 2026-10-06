//! `edel report` (roadmap M3.3b): what a person pastes into an issue when
//! Edel OS misbehaves on their hardware. The release and kernel, the
//! features the image is made of (M4.0), the boot time and memory in use
//! when the slot was confirmed (the line `edel-boot-ok` printed), the
//! memory in use now, every PCI device with its driver, and the kernel log,
//! as TOML on standard output. Everything comes from `/proc`, `/sys`,
//! `/usr/share/edel/features` and busybox's `dmesg`, so no tool is added.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use edel::features;
use serde::Serialize;

use crate::release::os_release_value;

/// Where `edel-boot-ok` keeps the line it printed when the slot was
/// confirmed.
pub const STARTED: &str = edel::places::STARTED;

/// The report's format, for the reader of a pasted report.
const FORMAT: u32 = 1;

#[derive(Serialize)]
struct Report {
    format: u32,
    version: String,
    kernel: String,
    /// The features in `/usr/share/edel/features/`, by name.
    features: Vec<String>,
    /// What reading them skipped, such as a field this edel does not know.
    feature_notes: Vec<String>,
    /// `Started in N s, M MiB of memory in use, R MiB used on the root`
    started: String,
    memory_in_use_mib: u64,
    dmesg: String,
    pci: Vec<Pci>,
}

/// One PCI device as sysfs shows it.
#[derive(Debug, PartialEq, Serialize)]
struct Pci {
    address: String,
    vendor: String,
    device: String,
    class: String,
    /// The bound driver, or `none`
    driver: String,
}

fn read_trimmed(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Every device under `devices` (`/sys/bus/pci/devices`), by address.
fn pci_devices(devices: &Path) -> Vec<Pci> {
    let Ok(entries) = fs::read_dir(devices) else {
        return Vec::new();
    };
    let mut found: Vec<Pci> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let dir = entry.path();
            let driver = fs::read_link(dir.join("driver"))
                .ok()
                .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "none".into());
            Pci {
                address: entry.file_name().to_string_lossy().into_owned(),
                vendor: read_trimmed(&dir.join("vendor")),
                device: read_trimmed(&dir.join("device")),
                class: read_trimmed(&dir.join("class")),
                driver,
            }
        })
        .collect();
    found.sort_by(|a, b| a.address.cmp(&b.address));
    found
}

/// The features in `dir`, read the way a machine reads them (leniently),
/// and a note for each thing skipped.
fn installed_features(dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return (names, notes);
    };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    for path in files {
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".toml"))
        else {
            continue;
        };
        match fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| features::read(&text))
        {
            Ok((_, skipped)) => {
                names.push(name.to_string());
                notes.extend(skipped.into_iter().map(|n| format!("{name}: {n}")));
            }
            Err(err) => notes.push(format!("{name}: not read: {err:#}")),
        }
    }
    (names, notes)
}

/// Whether the disk `name` is a stick or another removable disk: on USB,
/// or marked removable by the kernel. `sys` is `/sys`.
pub(crate) fn is_removable(sys: &Path, name: &str) -> bool {
    let block = sys.join("block").join(name);
    let removable = fs::read_to_string(block.join("removable")).is_ok_and(|r| r.trim() == "1");
    let on_usb = fs::canonicalize(&block).is_ok_and(|path| {
        path.components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with("usb"))
    });
    removable || on_usb
}

/// MemTotal minus MemAvailable from `/proc/meminfo`, in MiB.
fn memory_in_use_mib(meminfo: &str) -> Option<u64> {
    let field = |name: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    Some(field("MemTotal:")?.saturating_sub(field("MemAvailable:")?) / 1024)
}

fn render(report: &Report) -> Result<String> {
    Ok(toml::to_string(report)?)
}

/// `edel report`, or with `esp` written to `/EFI/edel/report.toml` on the
/// EFI system partition, where the desktop stick leaves it at every boot.
pub fn report(esp: bool) -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let dmesg = Command::new("dmesg")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    let (names, notes) = installed_features(Path::new(features::DIR));
    let report = Report {
        format: FORMAT,
        version: os_release_value(&os_release, "VERSION_ID").unwrap_or_default(),
        kernel: read_trimmed(Path::new("/proc/sys/kernel/osrelease")),
        features: names,
        feature_notes: notes,
        started: read_trimmed(Path::new(STARTED)),
        memory_in_use_mib: memory_in_use_mib(
            &fs::read_to_string("/proc/meminfo").unwrap_or_default(),
        )
        .unwrap_or(0),
        dmesg,
        pci: pci_devices(Path::new("/sys/bus/pci/devices")),
    };
    let text = render(&report)?;
    if esp {
        // Only a stick needs the report on its partition: an installed
        // machine has a login to run `edel report`, and its ESP is its one
        // way to boot, so it is not written at every boot for nothing.
        let disk = crate::update::Disk::find()?;
        if !is_removable(Path::new("/sys"), &disk.name) {
            println!(
                "edel report: {} is not a removable disk, so the report stays off its EFI system partition",
                disk.name
            );
            return Ok(());
        }
        let path = crate::update::write_beside_grubenv("report.toml", &text)?;
        println!("edel report: wrote {path} on the EFI system partition");
    } else {
        print!("{text}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pci_devices_and_their_drivers() {
        let dir = std::env::temp_dir().join(format!("edel-report-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let gpu = dir.join("0000:00:02.0");
        let wifi = dir.join("0000:00:14.3");
        for (path, vendor, device, class) in [
            (&gpu, "0x8086", "0x9a49", "0x030000"),
            (&wifi, "0x8086", "0xa0f0", "0x028000"),
        ] {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join("vendor"), format!("{vendor}\n")).unwrap();
            fs::write(path.join("device"), format!("{device}\n")).unwrap();
            fs::write(path.join("class"), format!("{class}\n")).unwrap();
        }
        std::os::unix::fs::symlink("../../../bus/pci/drivers/i915", gpu.join("driver")).unwrap();
        let devices = pci_devices(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].driver, "i915");
        assert_eq!(devices[0].device, "0x9a49");
        assert_eq!(devices[1].driver, "none");
    }

    #[test]
    fn lists_features_and_notes_what_it_skipped() {
        let dir = std::env::temp_dir().join(format!("edel-report-features-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ssh = "format = 1\nsummary = \"ssh\"\nwhy = \"w\"\npackages = [\"openssh-server\"]\n";
        fs::write(dir.join("ssh.toml"), format!("{ssh}shiny = true\n")).unwrap();
        fs::write(dir.join("base.toml"), "format = 1\nsummary = \"base\"\n").unwrap();
        fs::write(dir.join("broken.toml"), "format = [").unwrap();
        let (names, notes) = installed_features(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["base", "ssh"]);
        assert_eq!(notes[1], "ssh: unknown field shiny ignored");
        assert!(notes[0].starts_with("broken: not read"), "{notes:?}");
    }

    #[test]
    fn writes_to_the_esp_of_removable_disks_only() {
        let sys = std::env::temp_dir().join(format!("edel-report-sys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&sys);
        let devices = sys.join("devices/pci0000:00");
        for (dir, removable) in [
            ("0000:00:14.0/usb2/2-1/2-1:1.0/host0/block/sda", "0"),
            ("0000:00:04.0/virtio1/block/vda", "0"),
            ("0000:00:1f.2/ata1/host1/block/sdb", "1"),
        ] {
            fs::create_dir_all(devices.join(dir)).unwrap();
            fs::write(devices.join(dir).join("removable"), removable).unwrap();
        }
        fs::create_dir_all(sys.join("block")).unwrap();
        for (name, dir) in [
            ("sda", "0000:00:14.0/usb2/2-1/2-1:1.0/host0/block/sda"),
            ("vda", "0000:00:04.0/virtio1/block/vda"),
            ("sdb", "0000:00:1f.2/ata1/host1/block/sdb"),
        ] {
            std::os::unix::fs::symlink(devices.join(dir), sys.join("block").join(name)).unwrap();
        }
        let found = [
            is_removable(&sys, "sda"),
            is_removable(&sys, "vda"),
            is_removable(&sys, "sdb"),
        ];
        fs::remove_dir_all(&sys).unwrap();
        assert_eq!(found, [true, false, true]);
    }

    #[test]
    fn counts_memory_in_use() {
        let meminfo = "MemTotal:  512000 kB\nMemFree: 100 kB\nMemAvailable:  450560 kB\n";
        assert_eq!(memory_in_use_mib(meminfo), Some(60));
        assert_eq!(memory_in_use_mib("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn writes_toml_a_reader_can_parse() {
        let report = Report {
            format: FORMAT,
            version: "2026.10.90".into(),
            kernel: "6.18.54-0-lts".into(),
            features: vec!["base".into(), "ssh".into()],
            feature_notes: vec![],
            started: "Started in 6.78 s, 60 MiB of memory in use, 91 MiB used on the root".into(),
            memory_in_use_mib: 61,
            dmesg: "[    0.000000] Linux version 6.18.54\n[    1.0] a \"quoted\" line\n".into(),
            pci: vec![Pci {
                address: "0000:00:02.0".into(),
                vendor: "0x8086".into(),
                device: "0x9a49".into(),
                class: "0x030000".into(),
                driver: "i915".into(),
            }],
        };
        let text = render(&report).unwrap();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["format"].as_integer(), Some(1));
        assert_eq!(parsed["version"].as_str(), Some("2026.10.90"));
        assert!(parsed["dmesg"].as_str().unwrap().contains("\"quoted\""));
        assert_eq!(parsed["pci"][0]["driver"].as_str(), Some("i915"));
    }
}
