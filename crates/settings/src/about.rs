//! The About page (M5.6a): this release, read from `/usr/lib/os-release`
//! as `edel image build` writes it, and the last lines of the person's
//! session log (M5.28b), so a problem is found without a terminal.

//! The About page (M5.6a, made friendly in M5.8d at Alimardon's ask of
//! 2026-10-08): what a familiar "About this computer" says. The system's
//! name large, its version and channel under it, then Check for updates
//! (which opens the Updates page) and Copy system info, then plain rows
//! for this computer: its name, device, processor, memory, graphics and
//! storage, each read from `/proc` and `/sys` as it is, a row left out
//! when the machine does not say. The release comes from
//! `/usr/lib/os-release` as `edel image build` writes it. The last lines
//! of the person's session log (M5.28b) follow, so a problem is found
//! without a terminal.

use std::path::Path;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::{places, session_log};

use crate::{rows, widgets};

const OS_RELEASE: &str = "/usr/lib/os-release";

/// The action Check for updates calls: the window shows the page named.
const SHOW_PAGE: &str = "win.show-page";

pub fn page() -> gtk::Widget {
    let release = std::fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let (page, content, _) = widgets::page(tr("About"), "");
    let name = field(&release, "NAME").unwrap_or_else(|| tr("Edel OS").into());
    let version = field(&release, "VERSION_ID");
    let channel = field(&release, "EDEL_CHANNEL").map(|c| rows::label(&c));
    let line = rows::version_line(version.as_deref(), channel.as_deref());
    let computer = computer_rows(Path::new("/"));

    let buttons = widgets::banner(&content, &name, &line);
    let check = widgets::add_big(&buttons, tr("Check for updates"), true);
    check.connect_clicked(|button| {
        let _ = button.activate_action(SHOW_PAGE, Some(&"updates".to_variant()));
    });
    let copy = widgets::add_big(&buttons, tr("Copy system info"), false);
    let text = info_text(&name, &line, &computer);
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(&text);
        let label = button.label();
        button.set_label(tr("Copied"));
        let button = button.downgrade();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1400), move || {
            if let (Some(button), Some(label)) = (button.upgrade(), label) {
                button.set_label(&label);
            }
        });
    });

    widgets::heading(&content, tr("This computer"));
    let group = widgets::group(&content);
    for (title, value) in &computer {
        widgets::value_row(&group, title, value);
    }
    widgets::heading(&content, tr("Session log"));
    let group = widgets::group(&content);
    let dir = places::person_state_dir();
    match log_lines(dir.as_deref()) {
        Some(lines) => widgets::log_row(&group, tr("The last lines of this session"), &lines),
        None => widgets::text_row(&group, &nothing_yet(dir.as_deref())),
    }
    page
}

/// The rows about this computer, from the system's own files under `root`
/// (`/`, or a folder holding a copy in a test): its name, device,
/// processor, memory, graphics and the storage under the home folder.
fn computer_rows(root: &Path) -> Vec<(&'static str, String)> {
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let cpuinfo = read("proc/cpuinfo").unwrap_or_default();
    let dmi = |name: &str| {
        read(&format!("sys/class/dmi/id/{name}"))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let hostname = read("proc/sys/kernel/hostname").or_else(|| read("etc/hostname"));
    let (vendor, product) = (dmi("sys_vendor"), dmi("product_name"));
    let mut rows = vec![
        (
            tr("Name"),
            hostname.map(|h| h.trim().to_string()).unwrap_or_default(),
        ),
        (
            tr("Device"),
            device_name(vendor.as_deref(), product.as_deref())
                .or_else(|| board_model(&cpuinfo))
                .unwrap_or_default(),
        ),
        (tr("Processor"), cpu_text(&cpuinfo).unwrap_or_default()),
        (
            tr("Memory"),
            read("proc/meminfo")
                .and_then(|m| memory_text(&m))
                .unwrap_or_default(),
        ),
        (tr("Graphics"), graphics(root).unwrap_or_default()),
        (tr("Storage"), storage()),
    ];
    rows.retain(|(_, value)| !value.is_empty());
    rows
}

/// What the machine's maker calls it, `HP 250 G8 Notebook PC`, not the
/// placeholders firmware ships when nobody filled them in.
fn device_name(vendor: Option<&str>, product: Option<&str>) -> Option<String> {
    let junk = |v: &str| {
        let v = v.to_lowercase();
        [
            "to be filled",
            "system product",
            "default string",
            "not specified",
            "o.e.m.",
        ]
        .iter()
        .any(|j| v.contains(j))
    };
    let product = product.filter(|p| !junk(p))?;
    match vendor.filter(|v| !junk(v)) {
        Some(vendor) if !product.to_lowercase().starts_with(&vendor.to_lowercase()) => {
            Some(format!("{vendor} {product}"))
        }
        _ => Some(product.to_string()),
    }
}

/// The board's own name where there is no firmware table, as a Raspberry Pi's
/// `Model` line of `/proc/cpuinfo`.
fn board_model(cpuinfo: &str) -> Option<String> {
    cpuinfo_value(cpuinfo, "Model").filter(|m| !m.is_empty())
}

/// The first `key` line's value in `/proc/cpuinfo` text.
fn cpuinfo_value(cpuinfo: &str, key: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// The processor as `11th Gen Intel Core i7-1165G7 @ 2.80GHz × 8`: the
/// model's name without its trademark signs and the number of threads.
fn cpu_text(cpuinfo: &str) -> Option<String> {
    let name =
        cpuinfo_value(cpuinfo, "model name").or_else(|| cpuinfo_value(cpuinfo, "Hardware"))?;
    let name = name
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace(" CPU", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let threads = cpuinfo
        .lines()
        .filter(|l| {
            l.split_once(':')
                .is_some_and(|(k, _)| k.trim() == "processor")
        })
        .count();
    Some(if threads > 1 {
        format!("{name} × {threads}")
    } else {
        name
    })
}

/// The memory as the box says it, `16 GB`, from `/proc/meminfo`'s total,
/// which the firmware and the kernel have taken a little from: the
/// usual size just above it when that is within 12 percent, else the total.
fn memory_text(meminfo: &str) -> Option<String> {
    let kib: f64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let gib = kib / 1_048_576.0;
    const SIZES: [f64; 20] = [
        1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0, 192.0,
        256.0, 384.0, 512.0, 768.0, 1024.0,
    ];
    let shown = SIZES
        .iter()
        .copied()
        .find(|s| *s >= gib && *s <= gib * 1.12)
        .unwrap_or((gib * 10.0).round() / 10.0);
    let n = if shown.fract() == 0.0 {
        format!("{shown:.0}")
    } else {
        format!("{shown:.1}")
    };
    Some(trf("{n} GB", &[("n", &n)]))
}

/// The graphics: the first card's maker, from its PCI vendor, and its
/// driver as the last word, `Intel graphics`.
fn graphics(root: &Path) -> Option<String> {
    let mut cards: Vec<_> = std::fs::read_dir(root.join("sys/class/drm"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("card") && !name.contains('-')
        })
        .map(|e| e.path().join("device"))
        .collect();
    cards.sort();
    let card = cards.first()?;
    let vendor = std::fs::read_to_string(card.join("vendor")).ok();
    let driver = std::fs::read_link(card.join("driver"))
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()));
    graphics_name(vendor.as_deref(), driver.as_deref())
}

/// A graphics card's name from its PCI vendor id (`0x8086`) or, for a
/// maker not listed, its driver; none when neither is known.
fn graphics_name(vendor: Option<&str>, driver: Option<&str>) -> Option<String> {
    let maker = match vendor.map(|v| v.trim().to_lowercase()).as_deref() {
        Some("0x8086") => Some(tr("Intel graphics")),
        Some("0x1002") => Some(tr("AMD graphics")),
        Some("0x10de") => Some(tr("NVIDIA graphics")),
        Some("0x15ad") => Some(tr("VMware graphics")),
        Some("0x1af4") => Some(tr("Virtual graphics")),
        Some("0x1234") => Some(tr("Virtual graphics")),
        Some("0x106b") => Some(tr("Apple graphics")),
        Some("0x13b5") => Some(tr("Arm graphics")),
        _ => None,
    };
    match (maker, driver) {
        (Some(maker), _) => Some(maker.to_string()),
        (None, Some(driver)) => Some(trf("Graphics driver {driver}", &[("driver", driver)])),
        (None, None) => None,
    }
}

/// The storage under the home folder, `74 GB free of 128 GB`; empty when
/// the system does not say.
fn storage() -> String {
    let home = std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from);
    let info = gio::File::for_path(home)
        .query_filesystem_info("filesystem::size,filesystem::free", gio::Cancellable::NONE);
    let Ok(info) = info else {
        return String::new();
    };
    let (size, free) = (
        info.attribute_uint64("filesystem::size"),
        info.attribute_uint64("filesystem::free"),
    );
    storage_text(size, free)
}

fn storage_text(size: u64, free: u64) -> String {
    if size == 0 {
        return String::new();
    }
    trf(
        "{free} free of {size}",
        &[
            ("free", &rows::bytes_text(free)),
            ("size", &rows::bytes_text(size)),
        ],
    )
}

/// The rows as plain text for Copy system info: the name and version,
/// then a line for each row.
fn info_text(name: &str, version: &str, rows: &[(&str, String)]) -> String {
    let mut text = format!("{name}\n");
    if !version.is_empty() {
        text.push_str(&format!("{version}\n"));
    }
    for (title, value) in rows {
        text.push_str(&format!("{title}: {value}\n"));
    }
    text
}

/// The last [`session_log::LAST_LINES`] lines of the session log in `dir`
/// (a person's state folder), or none when there is no log or it is empty.
fn log_lines(dir: Option<&Path>) -> Option<String> {
    let bytes = std::fs::read(dir?.join(places::SESSION_LOG)).ok()?;
    let lines = session_log::tail(&String::from_utf8_lossy(&bytes), session_log::LAST_LINES);
    (!lines.trim().is_empty()).then_some(lines)
}

/// What the page says when there is no log to show, and where one will be.
fn nothing_yet(dir: Option<&Path>) -> String {
    match dir {
        Some(dir) => trf(
            "There is no session log yet. The desktop starts one when you log in, in {folder}.",
            &[(
                "folder",
                &dir.join(places::SESSION_LOG).display().to_string(),
            )],
        ),
        None => tr("There is no session log: neither XDG_STATE_HOME nor HOME is set.").into(),
    }
}

/// `key`'s value in this machine's os-release, its quotes taken off.
pub fn os_release_field(key: &str) -> Option<String> {
    field(
        &std::fs::read_to_string(OS_RELEASE).unwrap_or_default(),
        key,
    )
}

/// `key`'s value in os-release text, its quotes taken off.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_come_out_of_os_release_without_quotes() {
        let text = "NAME=\"Edel OS\"\nPRETTY_NAME=\"Edel OS 2026.10\"\nVERSION_ID=2026.10.1\nEDEL_CHANNEL=preview\n";
        assert_eq!(
            field(text, "PRETTY_NAME").as_deref(),
            Some("Edel OS 2026.10")
        );
        assert_eq!(field(text, "VERSION_ID").as_deref(), Some("2026.10.1"));
        assert_eq!(field(text, "NAME").as_deref(), Some("Edel OS"));
        assert_eq!(field(text, "BUILD_ID"), None);
    }

    const CPUINFO: &str = "processor\t: 0\nmodel name\t: 11th Gen Intel(R) Core(TM) i7-1165G7 @ 2.80GHz\n\
                           cpu MHz\t: 400.0\n\nprocessor\t: 1\nmodel name\t: 11th Gen Intel(R) Core(TM) i7-1165G7 @ 2.80GHz\n";

    #[test]
    fn the_processor_reads_without_trademark_signs_and_with_its_threads() {
        assert_eq!(
            cpu_text(CPUINFO).as_deref(),
            Some("11th Gen Intel Core i7-1165G7 @ 2.80GHz × 2")
        );
        assert_eq!(
            cpu_text("processor : 0\nmodel name : Intel(R) Xeon(R) CPU E5\n").as_deref(),
            Some("Intel Xeon E5")
        );
        assert_eq!(cpu_text("processor : 0\nCPU part : 0xd08\n"), None);
        assert_eq!(
            board_model("Hardware\t: BCM2711\nModel\t\t: Raspberry Pi 4 Model B\n").as_deref(),
            Some("Raspberry Pi 4 Model B")
        );
    }

    #[test]
    fn memory_reads_as_the_size_on_the_box() {
        let mem = |kib: u64| memory_text(&format!("MemTotal:       {kib} kB\nMemFree: 1 kB\n"));
        assert_eq!(mem(16_167_248).as_deref(), Some("16 GB"));
        assert_eq!(mem(15_600_000).as_deref(), Some("16 GB"));
        assert_eq!(mem(1_990_000).as_deref(), Some("2 GB"));
        assert_eq!(mem(7_900_000).as_deref(), Some("8 GB"));
        assert_eq!(
            mem(5_200_000).as_deref(),
            Some("5 GB"),
            "no usual size near"
        );
        assert_eq!(memory_text("nothing"), None);
    }

    #[test]
    fn the_device_is_the_makers_name_not_a_placeholder() {
        assert_eq!(
            device_name(Some("HP"), Some("HP 250 G8 Notebook PC")).as_deref(),
            Some("HP 250 G8 Notebook PC")
        );
        assert_eq!(
            device_name(Some("QEMU"), Some("Standard PC (Q35 + ICH9, 2009)")).as_deref(),
            Some("QEMU Standard PC (Q35 + ICH9, 2009)")
        );
        assert_eq!(
            device_name(Some("To Be Filled By O.E.M."), Some("System Product Name")),
            None
        );
        assert_eq!(
            device_name(Some("Default string"), Some("ThinkPad X1")).as_deref(),
            Some("ThinkPad X1")
        );
        assert_eq!(device_name(None, None), None);
    }

    #[test]
    fn graphics_are_named_by_their_maker_or_their_driver() {
        assert_eq!(
            graphics_name(Some("0x8086\n"), Some("i915")).as_deref(),
            Some("Intel graphics")
        );
        assert_eq!(
            graphics_name(Some("0x1af4"), Some("virtio-pci")).as_deref(),
            Some("Virtual graphics")
        );
        assert_eq!(
            graphics_name(Some("0xabcd"), Some("xyz")).as_deref(),
            Some("Graphics driver xyz")
        );
        assert_eq!(graphics_name(None, None), None);
    }

    #[test]
    fn the_rows_come_from_the_systems_files_and_skip_what_is_missing() {
        let root = std::env::temp_dir().join(format!("edel-about-rows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = |p: &str| std::fs::create_dir_all(root.join(p)).unwrap();
        let put = |p: &str, text: &str| std::fs::write(root.join(p), text).unwrap();
        dir("proc/sys/kernel");
        dir("sys/class/dmi/id");
        dir("sys/class/drm/card0/device");
        dir("sys/class/drm/card0-eDP-1");
        put("proc/sys/kernel/hostname", "ali-laptop\n");
        put("proc/cpuinfo", CPUINFO);
        put("proc/meminfo", "MemTotal:       16167248 kB\n");
        put("sys/class/dmi/id/sys_vendor", "HP\n");
        put("sys/class/dmi/id/product_name", "HP 250 G8 Notebook PC\n");
        put("sys/class/drm/card0/device/vendor", "0x8086\n");
        let rows = computer_rows(&root);
        let titles: Vec<&str> = rows.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            titles[..5],
            ["Name", "Device", "Processor", "Memory", "Graphics"]
        );
        assert_eq!(rows[0].1, "ali-laptop");
        assert_eq!(rows[1].1, "HP 250 G8 Notebook PC");
        assert_eq!(rows[4].1, "Intel graphics");
        let text = info_text("Edel OS", "Version 2026.10.5 · Stable", &rows);
        assert!(text.starts_with("Edel OS\nVersion 2026.10.5 · Stable\nName: ali-laptop\n"));
        assert!(text.contains("\nMemory: 16 GB\n"), "{text}");
        // A machine that says nothing has no rows of that kind.
        let bare = std::env::temp_dir().join(format!("edel-about-bare-{}", std::process::id()));
        std::fs::create_dir_all(&bare).unwrap();
        let rows = computer_rows(&bare);
        assert!(
            rows.iter().all(|(t, _)| *t == "Storage"),
            "only what the system itself answers: {rows:?}"
        );
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&bare).unwrap();
    }

    #[test]
    fn storage_says_what_is_free_of_the_whole() {
        assert_eq!(
            storage_text(128_000_000_000, 74_000_000_000),
            "74 GB free of 128 GB"
        );
        assert_eq!(storage_text(0, 0), "");
    }

    #[test]
    fn the_log_shows_its_last_lines_and_says_when_there_is_none() {
        let dir = std::env::temp_dir().join(format!("edel-about-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join(places::SESSION_LOG);
        assert_eq!(log_lines(Some(&dir)), None, "no file yet");
        assert_eq!(log_lines(None), None, "no folder");
        std::fs::write(&log, "").unwrap();
        assert_eq!(log_lines(Some(&dir)), None, "an empty file");
        let text: String = (1..=100)
            .map(|n| format!("edel-compositor: line {n}\n"))
            .collect();
        std::fs::write(&log, text).unwrap();
        let shown = log_lines(Some(&dir)).unwrap();
        assert_eq!(shown.lines().count(), session_log::LAST_LINES);
        assert!(shown.starts_with("edel-compositor: line 41\n"));
        assert!(shown.ends_with("edel-compositor: line 100\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_page_says_plainly_where_the_log_will_be() {
        assert_eq!(
            nothing_yet(Some(Path::new("/home/ali/.local/state/edel"))),
            "There is no session log yet. The desktop starts one when you log in, in /home/ali/.local/state/edel/session.log."
        );
        assert_eq!(
            nothing_yet(None),
            "There is no session log: neither XDG_STATE_HOME nor HOME is set."
        );
    }
}
