//! `edel report` (roadmap M3.3b): what a person pastes into an issue when
//! Edel OS misbehaves on their hardware. The release and kernel, the
//! features the image is made of (M4.0), the boot time and memory in use
//! when the slot was confirmed (the line `edel-boot-ok` printed), the
//! memory in use now, every PCI device with its driver, the kernel log, and
//! the last lines of the system log and of each desktop session's log
//! (M5.28a), the firmware the kernel asked for and this system lacks and the
//! language's script whose fonts it lacks (M5.30c), as TOML on standard
//! output. Everything comes from `/proc`, `/sys`, `/usr/share/edel/features`,
//! `/usr/share/fonts` and busybox's `dmesg`, so no tool is added.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use edel::i18n::trf;
use edel::{features, places, scripts, session_log, settings};
use serde::Serialize;

use crate::release::os_release_value;

/// Where `edel-boot-ok` keeps the line it printed when the slot was
/// confirmed.
pub const STARTED: &str = edel::places::STARTED;

/// The report's format, for the reader of a pasted report.
const FORMAT: u32 = 1;

/// Where the system's fonts are, which the missing script is looked for in
/// (M5.30c).
const FONTS: &str = "/usr/share/fonts";

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
    /// The system log's last lines, busybox syslogd's (M5.28a).
    system_log: String,
    pci: Vec<Pci>,
    /// The firmware files the kernel asked for and this system does not
    /// carry, from `dmesg` (M5.30c).
    missing_firmware: Vec<Missing>,
    /// The language's script whose fonts this system lacks, when one is
    /// (M5.30c).
    missing_script: Option<MissingScript>,
    /// The last lines of each person's desktop session logs (M5.28a): the
    /// one running it, or for root everyone's.
    sessions: Vec<Session>,
}

/// One person's session log, its last lines.
#[derive(Debug, PartialEq, Serialize)]
struct Session {
    person: String,
    path: String,
    /// Whether that session ended as it should; one still running has not
    ended: bool,
    last_lines: String,
}

/// The session logs `people` have, the session before's first.
fn sessions(people: &[session_log::Person]) -> Vec<Session> {
    let mut found = Vec::new();
    for person in people {
        for name in [places::SESSION_LOG_BEFORE, places::SESSION_LOG] {
            let path = person.dir.join(name);
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            found.push(Session {
                person: person.name.clone(),
                path: path.display().to_string(),
                ended: session_log::ended(&text),
                last_lines: session_log::tail(&text, session_log::LAST_LINES),
            });
        }
    }
    found
}

/// One firmware file the kernel asked for and this system does not carry
/// (M5.30c): the device that asked and the file.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct Missing {
    pub driver: String,
    pub device: String,
    pub file: String,
}

/// The language whose script this system has no font for (M5.30c), as the
/// language is set.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct MissingScript {
    pub language: String,
    pub script: String,
}

/// The firmware the kernel asked for and could not find, from `dmesg`'s
/// lines `DRIVER DEVICE: Direct firmware load for FILE failed with error -2`
/// and `DRIVER DEVICE: firmware: failed to load FILE (-2)` (M5.30c). Each
/// file once, in the order first seen; a file that exists but failed with
/// another error is not missing, so it is left out.
pub(crate) fn missing_firmware(dmesg: &str) -> Vec<Missing> {
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

/// The `logs:` lines `edel status` prints when something went wrong: a
/// boot that fell back (`fell_back`), each session in `badly` that ended
/// without closing, by person and log; then a `firmware:` line for the
/// firmware the kernel lacked (`firmware`) and a `fonts:` line for the
/// language's script with no font (`script`). None when all is well.
pub(crate) fn pointers(
    fell_back: bool,
    badly: &[(String, PathBuf)],
    firmware: &[Missing],
    script: Option<&MissingScript>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if fell_back {
        lines.push(format!(
            "logs: {}",
            trf(
                "the last update did not start, so the machine went back to the slot before; the system log is {log}, and edel report gathers it with the rest",
                &[("log", places::SYSTEM_LOG)]
            )
        ));
    }
    for (person, path) in badly {
        lines.push(format!(
            "logs: {}",
            trf(
                "{person}'s last desktop session ended without closing; its log is {log}, and edel report gathers it with the rest",
                &[("person", person), ("log", &path.display().to_string())]
            )
        ));
    }
    if let Some(first) = firmware.first() {
        lines.push(format!(
            "firmware: {}",
            trf(
                "the kernel asked for firmware this system does not carry; the first is {file} for {driver} ({count} missing in all); edel report lists them, and a later release brings the hardware packs that hold them",
                &[
                    ("file", &first.file),
                    ("driver", &first.driver),
                    ("count", &firmware.len().to_string()),
                ]
            )
        ));
    }
    if let Some(script) = script {
        lines.push(format!(
            "fonts: {}",
            trf(
                "the language {language} needs {script} fonts, which this system does not carry; text in it shows as boxes until a later release brings them",
                &[("language", &script.language), ("script", &script.script)]
            )
        ));
    }
    lines
}

/// [`pointers`] for this machine now.
pub(crate) fn pointers_now() -> Vec<String> {
    let badly: Vec<_> = session_log::people(Path::new(places::HOMES), as_root())
        .into_iter()
        .filter_map(|person| {
            let running =
                session_log::runs_as_owner_of(Path::new("/proc"), "edel-compositor", &person.dir);
            Some((
                person.name.clone(),
                session_log::ended_badly(&person, running)?,
            ))
        })
        .collect();
    pointers(
        Path::new(places::LAST_FALLBACK).exists(),
        &badly,
        &missing_firmware(&dmesg_now()),
        missing_script_now().as_ref(),
    )
}

/// The kernel log from busybox's `dmesg`; empty when it fails, as it does
/// when the kernel keeps the log from a person (M5.30c).
fn dmesg_now() -> String {
    Command::new("dmesg")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// The names of the files under `dir`, recursively and bare; symbolic links
/// are not followed into folders (M5.30c).
fn font_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            names.extend(font_files(&entry.path()));
        } else {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names
}

/// The language `region.language` names for this report: the person's over
/// the machine's, but only the machine's as root, whose home is not the
/// person's (M5.30c).
fn language_now() -> Option<String> {
    let read = |path: PathBuf| fs::read_to_string(places::found(&path)).ok();
    let machine = read(places::machine_settings());
    let person = if as_root() {
        None
    } else {
        places::person_settings().and_then(read)
    };
    settings::chosen("region.language", machine.as_deref(), person.as_deref())
}

/// The script `language` needs when `font_files` (bare names) hold no Noto
/// font of it; none when it needs no script beyond the three, or has one
/// (M5.30c).
fn missing_script(language: Option<&str>, font_files: &[String]) -> Option<MissingScript> {
    let language = language?;
    let script = scripts::script_of(language)?;
    (!scripts::has_script(script, font_files)).then(|| MissingScript {
        language: language.to_string(),
        script: script.to_string(),
    })
}

/// [`missing_script`] for this machine now: the person's language, and the
/// fonts under `/usr/share/fonts`.
fn missing_script_now() -> Option<MissingScript> {
    missing_script(language_now().as_deref(), &font_files(Path::new(FONTS)))
}

/// Whether this process runs as root, as `/proc/self`'s owner says.
pub(crate) fn as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
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
    let dmesg = dmesg_now();
    let missing = missing_firmware(&dmesg);
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
        system_log: session_log::tail(
            &fs::read_to_string(places::SYSTEM_LOG).unwrap_or_default(),
            session_log::LAST_LINES,
        ),
        pci: pci_devices(Path::new("/sys/bus/pci/devices")),
        missing_firmware: missing,
        missing_script: missing_script_now(),
        sessions: sessions(&session_log::people(Path::new(places::HOMES), as_root())),
    };
    let text = render(&report)?;
    if esp {
        // Only a stick needs the report on its partition: an installed
        // machine has a login to run `edel report`, and its ESP is its one
        // way to boot, so it is not written at every boot for nothing.
        let disk = crate::update::Disk::find()?;
        if !is_removable(Path::new("/sys"), &disk.name) {
            println!(
                "edel report: {}",
                trf(
                    "{disk} is not a removable disk, so the report stays off its EFI system partition",
                    &[("disk", &disk.name)]
                )
            );
            return Ok(());
        }
        let path = crate::update::write_beside_grubenv("report.toml", &text)?;
        println!(
            "edel report: {}",
            trf(
                "wrote {path} on the EFI system partition",
                &[("path", &path)]
            )
        );
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
            missing_firmware: vec![Missing {
                driver: "iwlwifi".into(),
                device: "0000:00:14.3".into(),
                file: "iwlwifi-so-a0-gf-a0-90.ucode".into(),
            }],
            missing_script: Some(MissingScript {
                language: "ja_JP.UTF-8".into(),
                script: "CJK".into(),
            }),
            system_log: "Oct  7 11:03:25 edel syslog.info syslogd started\n".into(),
            sessions: vec![Session {
                person: "ci".into(),
                path: "/home/ci/.local/state/edel/session.log".into(),
                ended: false,
                last_lines: "edel-compositor: output Virtual-1 ready\n".into(),
            }],
        };
        let text = render(&report).unwrap();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["format"].as_integer(), Some(1));
        assert_eq!(parsed["version"].as_str(), Some("2026.10.90"));
        assert!(parsed["dmesg"].as_str().unwrap().contains("\"quoted\""));
        assert_eq!(parsed["pci"][0]["driver"].as_str(), Some("i915"));
        assert_eq!(parsed["sessions"][0]["person"].as_str(), Some("ci"));
        assert!(parsed["system_log"].as_str().unwrap().contains("syslogd"));
        assert_eq!(
            parsed["missing_firmware"][0]["file"].as_str(),
            Some("iwlwifi-so-a0-gf-a0-90.ucode")
        );
        assert_eq!(parsed["missing_script"]["script"].as_str(), Some("CJK"));
    }

    #[test]
    fn status_points_to_the_logs_only_when_something_went_wrong() {
        assert!(pointers(false, &[], &[], None).is_empty());
        let lines = pointers(
            true,
            &[(
                "ci".into(),
                "/home/ci/.local/state/edel/session.old.log".into(),
            )],
            &[],
            None,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(places::SYSTEM_LOG), "{}", lines[0]);
        assert_eq!(
            lines[1],
            "logs: ci's last desktop session ended without closing; its log is /home/ci/.local/state/edel/session.old.log, and edel report gathers it with the rest"
        );
    }

    /// `edel status` names the firmware a device lacks and the language whose
    /// script has no font, with what brings them (M5.30c).
    #[test]
    fn status_names_missing_firmware_and_fonts() {
        let firmware = vec![Missing {
            driver: "iwlwifi".into(),
            device: "0000:00:14.3".into(),
            file: "iwlwifi-so-a0-gf-a0-90.ucode".into(),
        }];
        let script = MissingScript {
            language: "ja".into(),
            script: "CJK".into(),
        };
        assert_eq!(
            pointers(false, &[], &firmware, Some(&script)),
            [
                "firmware: the kernel asked for firmware this system does not carry; the first is iwlwifi-so-a0-gf-a0-90.ucode for iwlwifi (1 missing in all); edel report lists them, and a later release brings the hardware packs that hold them",
                "fonts: the language ja needs CJK fonts, which this system does not carry; text in it shows as boxes until a later release brings them",
            ]
        );
    }

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
    fn a_language_whose_script_has_no_font_is_named() {
        let dejavu = vec!["DejaVuSans.ttf".to_string()];
        assert_eq!(
            missing_script(Some("ja_JP.UTF-8"), &dejavu),
            Some(MissingScript {
                language: "ja_JP.UTF-8".into(),
                script: "CJK".into(),
            })
        );
        assert_eq!(missing_script(Some("de_DE.UTF-8"), &dejavu), None);
        assert_eq!(missing_script(None, &dejavu), None);
        let cjk = vec!["NotoSansCJK-Regular.ttc".to_string()];
        assert_eq!(missing_script(Some("ja"), &cjk), None);
    }

    #[test]
    fn lists_font_file_names_under_the_folder_recursively() {
        let dir = std::env::temp_dir().join(format!("edel-report-fonts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("noto")).unwrap();
        fs::write(dir.join("noto/NotoSansCJK-Regular.ttc"), "").unwrap();
        fs::write(dir.join("Inter.ttf"), "").unwrap();
        let mut names = font_files(&dir);
        fs::remove_dir_all(&dir).unwrap();
        names.sort();
        assert_eq!(names, ["Inter.ttf", "NotoSansCJK-Regular.ttc"]);
    }

    #[test]
    fn gathers_both_sessions_logs_the_one_before_first() {
        let dir = std::env::temp_dir().join(format!("edel-report-logs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(places::SESSION_LOG_BEFORE), "killed half way\n").unwrap();
        fs::write(
            dir.join(places::SESSION_LOG),
            format!("ready\n{}\n", session_log::ENDED),
        )
        .unwrap();
        let person = session_log::Person {
            name: "ci".into(),
            dir: dir.clone(),
        };
        let found = sessions(&[person]);
        assert_eq!(found.len(), 2);
        assert!(!found[0].ended);
        assert!(found[0].path.ends_with(places::SESSION_LOG_BEFORE));
        assert!(found[1].ended);
        assert!(found[1].last_lines.starts_with("ready\n"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
