//! Bluetooth as BlueZ reports it (roadmap M5.8a), through
//! `bluetoothctl`'s one-command form: whether the machine has an adapter
//! and whether it is on, the devices it has paired with and the ones it
//! found nearby, and the commands that switch it on and off, scan, pair,
//! connect, disconnect and remove. Settings' Bluetooth page reads and acts
//! here, and the panel's quick settings will (M5.9), so the reading is
//! written once.
//!
//! The pairings are BlueZ's own state, kept in `/var/lib/bluetooth/`, and
//! the keys that go with them are secrets, so no key of the settings file
//! holds them (ADR-006). `bluetoothctl` is the one command line to this; a
//! machine without it (no `bluetooth` feature) says so. A device is
//! whatever BlueZ lists (ADR-011): nothing here assumes it is local. Std
//! only.

use std::path::Path;
use std::time::Duration;

use anyhow::{Result, bail};

use crate::i18n::{tr, trf};
use crate::tool;

/// The program every call goes through, found on the `PATH`.
pub const BLUETOOTHCTL: &str = "bluetoothctl";

/// Where the kernel lists Bluetooth adapters, one `hciN` each.
pub const SYSFS: &str = "/sys/class/bluetooth";

/// How long a read may take before the page gives up on it.
const READ_TIME: Duration = Duration::from_secs(8);

/// How long pairing, which waits for the device, may take.
const PAIR_TIME: Duration = Duration::from_secs(70);

/// How long a scan looks, in seconds.
pub const SCAN_SECONDS: u32 = 10;

/// The most devices read in one go: each is a call of its own.
const MOST_DEVICES: usize = 40;

/// What a device is, from the icon BlueZ gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Headphones, a headset, a speaker
    Audio,
    Keyboard,
    /// A mouse, a trackpad, a tablet
    Pointer,
    /// A game controller
    Controller,
    Phone,
    Computer,
    Other,
}

impl Kind {
    fn of_icon(icon: &str) -> Kind {
        match icon {
            "audio-headset" | "audio-headphones" | "audio-card" | "audio-speakers"
            | "multimedia-player" => Kind::Audio,
            "input-keyboard" => Kind::Keyboard,
            "input-mouse" | "input-tablet" => Kind::Pointer,
            "input-gaming" => Kind::Controller,
            "phone" => Kind::Phone,
            "computer" => Kind::Computer,
            _ => Kind::Other,
        }
    }
}

/// An adapter, from `bluetoothctl show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub address: String,
    /// What other devices see it called
    pub name: String,
    pub powered: bool,
    /// Whether it is looking for devices now
    pub scanning: bool,
}

/// A device, from `bluetoothctl info ADDRESS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
}

impl Device {
    /// Whether it has no name of its own, only its address (BlueZ then
    /// names it `AA-BB-CC-DD-EE-FF`): the beacons and trackers nearby,
    /// which a list would drown in.
    pub fn unnamed(&self) -> bool {
        self.name == self.address.replace(':', "-")
    }
}

/// What the page shows: no adapter, or the adapter and its devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub adapter: Option<Adapter>,
    /// Paired devices first (connected ones among them first), then the
    /// ones found nearby, each in the order of their names.
    pub devices: Vec<Device>,
}

/// The adapters in the text of `bluetoothctl list`, lines such as
/// `Controller A4:C3:F0:12:34:56 edel [default]`, as their addresses and
/// names.
pub fn parse_list(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = clean(line).strip_prefix("Controller ")?.to_string();
            let (address, name) = rest.split_once(' ').unwrap_or((&rest, ""));
            is_address(address).then(|| {
                let name = name.trim().trim_end_matches("[default]").trim();
                (address.to_string(), name.to_string())
            })
        })
        .collect()
}

/// The devices in the text of `bluetoothctl devices`, lines such as
/// `Device 00:11:22:33:44:55 Headphones`, as their addresses and names.
pub fn parse_devices(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = clean(line).strip_prefix("Device ")?.to_string();
            let (address, name) = rest.split_once(' ').unwrap_or((&rest, ""));
            is_address(address).then(|| (address.to_string(), name.trim().to_string()))
        })
        .collect()
}

/// The value of `key` in the lines `\tKey: value` of `show` and `info`.
fn property(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = clean(line);
        let (k, v) = line.trim().split_once(':')?;
        (k == key).then(|| v.trim().to_string())
    })
}

/// The adapter in the text of `bluetoothctl show`: a line
/// `Controller ADDRESS (public)` and then its properties.
pub fn parse_show(text: &str) -> Option<Adapter> {
    let head = text
        .lines()
        .map(clean)
        .find(|l| l.starts_with("Controller "))?;
    let address = head.split_whitespace().nth(1).filter(|a| is_address(a))?;
    let name = property(text, "Alias")
        .or_else(|| property(text, "Name"))
        .unwrap_or_default();
    Some(Adapter {
        address: address.to_string(),
        name,
        powered: property(text, "Powered").is_some_and(|v| v == "yes"),
        scanning: property(text, "Discovering").is_some_and(|v| v == "yes"),
    })
}

/// The device in the text of `bluetoothctl info ADDRESS`: a line
/// `Device ADDRESS (public)` and then its properties.
pub fn parse_info(text: &str) -> Option<Device> {
    let head = text.lines().map(clean).find(|l| l.starts_with("Device "))?;
    let address = head.split_whitespace().nth(1).filter(|a| is_address(a))?;
    let yes = |key: &str| property(text, key).is_some_and(|v| v == "yes");
    Some(Device {
        address: address.to_string(),
        name: property(text, "Alias")
            .or_else(|| property(text, "Name"))
            .unwrap_or_else(|| address.to_string()),
        kind: property(text, "Icon").map_or(Kind::Other, |i| Kind::of_icon(&i)),
        paired: yes("Paired"),
        connected: yes("Connected"),
        trusted: yes("Trusted"),
    })
}

/// Six pairs of hex digits and colons, `A4:C3:F0:12:34:56`.
fn is_address(text: &str) -> bool {
    text.len() == 17
        && text.split(':').count() == 6
        && text
            .split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A line without the colour codes `bluetoothctl` writes even when it is
/// not on a terminal.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// Whether the kernel lists a Bluetooth adapter in `sysfs` (one `hciN`; the
/// entries with a `:` are connections).
pub fn kernel_has_adapter(sysfs: &Path) -> bool {
    std::fs::read_dir(sysfs).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("hci") && !name.contains(':')
        })
    })
}

/// `bluetoothctl` with `args`, its answer even when it failed (it writes
/// its errors to standard output), or why it could not run.
fn bluetoothctl(args: &[String], time: Duration) -> Result<tool::Done> {
    match tool::run(BLUETOOTHCTL, args, time) {
        Ok(done) => Ok(done),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "{}",
            trf(
                "this machine has no {program}, so it has no Bluetooth; the bluetooth feature brings BlueZ",
                &[("program", BLUETOOTHCTL)]
            )
        ),
        Err(e) => bail!(
            "{}",
            trf(
                "could not run {program}: {error}",
                &[("program", BLUETOOTHCTL), ("error", &e.to_string())]
            )
        ),
    }
}

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// `bluetoothctl` with `args` as a line to type, what Copy as command shows.
pub fn command_line(args: &[String]) -> String {
    tool::command_line(BLUETOOTHCTL, args)
}

/// A command that must succeed: its output, else why it did not, in words
/// that tell what to do. `name` is the device it was about, for the words.
fn must(args: &[String], time: Duration, name: &str) -> Result<String> {
    let done = bluetoothctl(args, time)?;
    let said = clean(&format!("{}{}", done.out, done.err));
    let failed =
        !done.ok() || said.contains("Failed to ") || said.contains("No default controller");
    if !failed {
        return Ok(done.out);
    }
    bail!(
        "{}",
        explain(&command_line(args), &said, done.timed_out, name)
    );
}

/// What a failed command said, in words that tell what to do.
fn explain(line: &str, said: &str, timed_out: bool, name: &str) -> String {
    let last = said
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("[CHG]") && !l.starts_with("[NEW]"))
        .unwrap_or_default();
    if said.contains("No default controller") {
        tr("this computer has no Bluetooth adapter").to_string()
    } else if said.contains("Blocked") || said.contains("rfkill") {
        tr("Bluetooth is turned off by a switch or key on this computer; turn it on there and try again").to_string()
    } else if said.contains("AuthenticationFailed")
        || said.contains("AuthenticationRejected")
        || said.contains("AuthenticationCanceled")
    {
        trf(
            "{name} refused to pair; put it in pairing mode and try again",
            &[("name", name)],
        )
    } else if said.contains("AuthenticationTimeout")
        || said.contains("ConnectionAttemptFailed")
        || said.contains("page-timeout")
        || said.contains("Host is down")
        || timed_out
    {
        trf(
            "{name} did not answer; check that it is on and close by, then try again",
            &[("name", name)],
        )
    } else {
        trf(
            "{command} failed: {said}",
            &[("command", line), ("said", last)],
        )
    }
}

/// Everything the page shows. No adapter is not an error: the snapshot
/// then has none. An adapter the kernel has but `bluetoothd` does not
/// answer for is one, because the machine has Bluetooth and it is not
/// running.
pub fn read() -> Result<Snapshot> {
    read_in(Path::new(SYSFS))
}

fn read_in(sysfs: &Path) -> Result<Snapshot> {
    let list = bluetoothctl(&words(&["list"]), READ_TIME)?;
    if !list.ok() || parse_list(&list.out).is_empty() {
        if kernel_has_adapter(sysfs) {
            bail!(
                "{}",
                tr(
                    "Bluetooth is not running, though this computer has an adapter; it starts with the machine, so restart the computer if it stays off"
                )
            );
        }
        return Ok(Snapshot {
            adapter: None,
            devices: Vec::new(),
        });
    }
    let show = bluetoothctl(&words(&["show"]), READ_TIME)?;
    let adapter = parse_show(&show.out);
    let mut devices = Vec::new();
    let listed = bluetoothctl(&words(&["devices"]), READ_TIME)?;
    for (address, _) in parse_devices(&listed.out).into_iter().take(MOST_DEVICES) {
        let info = bluetoothctl(&["info".to_string(), address], READ_TIME)?;
        if let Some(device) = parse_info(&info.out) {
            devices.push(device);
        }
    }
    Ok(Snapshot {
        adapter,
        devices: arrange(devices),
    })
}

/// The devices to show: nameless ones that are not paired left out, then
/// the paired first, the connected among them first, each group by name.
pub fn arrange(mut devices: Vec<Device>) -> Vec<Device> {
    devices.retain(|d| d.paired || !d.unnamed());
    devices.sort_by_key(|d| (!d.paired, !d.connected, d.name.to_lowercase()));
    devices
}

/// The arguments that switch the adapter on or off.
pub fn power_args(on: bool) -> Vec<String> {
    words(&["power", if on { "on" } else { "off" }])
}

/// The arguments that look for devices for [`SCAN_SECONDS`]; the scan
/// ends when `bluetoothctl` does.
pub fn scan_args() -> Vec<String> {
    vec![
        "--timeout".into(),
        SCAN_SECONDS.to_string(),
        "scan".into(),
        "on".into(),
    ]
}

/// The arguments that pair with `address`. The agent that answers the
/// device's questions has no keyboard and no screen, which is how
/// headphones, speakers, mice and most phones pair ("Just Works").
pub fn pair_args(address: &str) -> Vec<String> {
    words(&["--agent", "NoInputNoOutput", "pair", address])
}

/// The arguments for a command about one device: `connect`, `disconnect`,
/// `trust` or `remove`.
pub fn device_args(command: &str, address: &str) -> Vec<String> {
    words(&[command, address])
}

/// Switches the adapter on or off.
pub fn set_power(on: bool) -> Result<()> {
    must(&power_args(on), READ_TIME, "").map(drop)
}

/// Looks for devices nearby for [`SCAN_SECONDS`]. Returns when the look is
/// over; the devices found are in the next [`read`].
pub fn scan() -> Result<()> {
    let args = scan_args();
    let done = bluetoothctl(&args, Duration::from_secs(u64::from(SCAN_SECONDS) + 10))?;
    let said = clean(&format!("{}{}", done.out, done.err));
    // The scan ends by its own time, which `bluetoothctl` may report as a
    // failure; only an adapter that cannot scan is one.
    if said.contains("Failed to start discovery") || said.contains("No default controller") {
        bail!("{}", explain(&command_line(&args), &said, false, ""));
    }
    Ok(())
}

/// Pairs with a device, trusts it so it connects by itself next time, and
/// connects.
pub fn pair(address: &str, name: &str) -> Result<()> {
    must(&pair_args(address), PAIR_TIME, name)?;
    must(&device_args("trust", address), READ_TIME, name)?;
    must(&device_args("connect", address), PAIR_TIME, name).map(drop)
}

/// Connects a device that is paired.
pub fn connect(address: &str, name: &str) -> Result<()> {
    must(&device_args("connect", address), PAIR_TIME, name).map(drop)
}

/// Disconnects a device, which stays paired.
pub fn disconnect(address: &str, name: &str) -> Result<()> {
    must(&device_args("disconnect", address), READ_TIME, name).map(drop)
}

/// Removes a device: unpairs it and forgets its keys.
pub fn remove(address: &str, name: &str) -> Result<()> {
    must(&device_args("remove", address), READ_TIME, name).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = "\
Controller A4:C3:F0:12:34:56 (public)
\tName: edel
\tAlias: Ali's laptop
\tClass: 0x007c0104
\tPowered: yes
\tPowerState: on
\tDiscoverable: no
\tPairable: yes
\tUUID: Generic Attribute Profile (00001801-0000-1000-8000-00805f9b34fb)
\tModalias: usb:v1D6Bp0246d0540
\tDiscovering: no
";

    const INFO: &str = "\
Device 00:11:22:33:44:55 (public)
\tName: WH-1000XM4
\tAlias: My headphones
\tClass: 0x00240404
\tIcon: audio-headset
\tPaired: yes
\tBonded: yes
\tTrusted: yes
\tBlocked: no
\tConnected: yes
\tLegacyPairing: no
";

    fn device(address: &str, name: &str, paired: bool, connected: bool) -> Device {
        Device {
            address: address.into(),
            name: name.into(),
            kind: Kind::Other,
            paired,
            connected,
            trusted: false,
        }
    }

    #[test]
    fn the_adapters_of_bluetoothctl_list_are_read() {
        let list = parse_list(
            "Controller A4:C3:F0:12:34:56 edel [default]\nController 11:22:33:44:55:66 USB dongle\n",
        );
        assert_eq!(
            list,
            [
                ("A4:C3:F0:12:34:56".to_string(), "edel".to_string()),
                ("11:22:33:44:55:66".to_string(), "USB dongle".to_string()),
            ]
        );
        assert!(parse_list("").is_empty());
        assert!(parse_list("Waiting to connect to bluetoothd...").is_empty());
        assert!(parse_list("Controller not-an-address x").is_empty());
    }

    #[test]
    fn the_adapter_of_bluetoothctl_show_is_read() {
        let adapter = parse_show(SHOW).unwrap();
        assert_eq!(adapter.address, "A4:C3:F0:12:34:56");
        assert_eq!(adapter.name, "Ali's laptop", "the alias is what others see");
        assert!(adapter.powered && !adapter.scanning);
        let off = parse_show(
            &SHOW
                .replace("Powered: yes", "Powered: no")
                .replace("Discovering: no", "Discovering: yes"),
        )
        .unwrap();
        assert!(!off.powered && off.scanning);
        assert_eq!(parse_show("No default controller available"), None);
        assert_eq!(parse_show(""), None);
    }

    #[test]
    fn the_devices_of_bluetoothctl_devices_keep_names_with_spaces() {
        let devices = parse_devices(
            "Device 00:11:22:33:44:55 My headphones\nDevice AA-BB not an address\nDevice AA:BB:CC:DD:EE:FF AA-BB-CC-DD-EE-FF\n",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].1, "My headphones");
        assert_eq!(devices[1].1, "AA-BB-CC-DD-EE-FF");
    }

    #[test]
    fn a_device_is_read_from_bluetoothctl_info() {
        let device = parse_info(INFO).unwrap();
        assert_eq!(device.address, "00:11:22:33:44:55");
        assert_eq!(device.name, "My headphones");
        assert_eq!(device.kind, Kind::Audio);
        assert!(device.paired && device.connected && device.trusted);
        let found = parse_info(
            "Device AA:BB:CC:DD:EE:FF (random)\n\tName: Mouse\n\tPaired: no\n\tConnected: no\n",
        )
        .unwrap();
        assert_eq!(found.name, "Mouse");
        assert_eq!(found.kind, Kind::Other);
        assert!(!found.paired && !found.connected);
        assert_eq!(parse_info("Device 12 (public)"), None);
    }

    #[test]
    fn the_icons_of_bluez_are_kinds() {
        for (icon, kind) in [
            ("audio-headphones", Kind::Audio),
            ("input-keyboard", Kind::Keyboard),
            ("input-mouse", Kind::Pointer),
            ("input-gaming", Kind::Controller),
            ("phone", Kind::Phone),
            ("computer", Kind::Computer),
            ("something-new", Kind::Other),
        ] {
            assert_eq!(Kind::of_icon(icon), kind, "{icon}");
        }
    }

    #[test]
    fn colour_codes_do_not_break_a_line() {
        let list = parse_list("\x1b[0;94mController A4:C3:F0:12:34:56 edel [default]\x1b[0m\r\n");
        assert_eq!(list.len(), 1);
        assert_eq!(clean("\x1b[1;39mhello\x1b[0m"), "hello");
    }

    #[test]
    fn nameless_devices_are_hidden_unless_paired() {
        let a = "AA:BB:CC:DD:EE:01";
        let nameless = device(a, "AA-BB-CC-DD-EE-01", false, false);
        assert!(nameless.unnamed());
        let shown = arrange(vec![
            nameless.clone(),
            device("AA:BB:CC:DD:EE:02", "Zebra speaker", false, false),
            device("AA:BB:CC:DD:EE:03", "mouse", true, false),
            device("AA:BB:CC:DD:EE:04", "Headphones", true, true),
            Device {
                paired: true,
                ..nameless
            },
        ]);
        let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
        // Connected first, then paired by name, then the ones nearby.
        assert_eq!(
            names,
            ["Headphones", "AA-BB-CC-DD-EE-01", "mouse", "Zebra speaker"]
        );
    }

    #[test]
    fn an_adapter_in_the_kernel_is_found_by_its_name() {
        let dir = std::env::temp_dir().join(format!("edel-bluetooth-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!kernel_has_adapter(&dir), "no directory, no adapter");
        std::fs::create_dir_all(dir.join("hci0:256")).unwrap();
        assert!(!kernel_has_adapter(&dir), "a connection is not an adapter");
        std::fs::create_dir_all(dir.join("hci0")).unwrap();
        assert!(kernel_has_adapter(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_commands_are_bluetoothctls() {
        assert_eq!(power_args(true), ["power", "on"]);
        assert_eq!(
            command_line(&scan_args()),
            "bluetoothctl --timeout 10 scan on"
        );
        assert_eq!(
            command_line(&pair_args("00:11:22:33:44:55")),
            "bluetoothctl --agent NoInputNoOutput pair 00:11:22:33:44:55"
        );
        assert_eq!(
            command_line(&device_args("remove", "00:11:22:33:44:55")),
            "bluetoothctl remove 00:11:22:33:44:55"
        );
    }

    #[test]
    fn a_failure_says_what_to_do() {
        let line = "bluetoothctl connect X";
        assert!(
            explain(line, "No default controller available", false, "")
                .contains("no Bluetooth adapter")
        );
        assert!(
            explain(
                line,
                "Failed to set power on: org.bluez.Error.Blocked",
                false,
                ""
            )
            .contains("turned off by a switch")
        );
        assert_eq!(
            explain(
                line,
                "Failed to pair: org.bluez.Error.AuthenticationFailed",
                false,
                "Mouse"
            ),
            "Mouse refused to pair; put it in pairing mode and try again"
        );
        assert_eq!(
            explain(
                line,
                "Failed to connect: org.bluez.Error.Failed page-timeout",
                false,
                "Mouse"
            ),
            "Mouse did not answer; check that it is on and close by, then try again"
        );
        assert_eq!(
            explain(line, "", true, "Mouse"),
            "Mouse did not answer; check that it is on and close by, then try again"
        );
        assert_eq!(
            explain(
                line,
                "[CHG] Device X Connected: no\nFailed to connect: org.bluez.Error.Failed\n",
                false,
                "Mouse"
            ),
            "bluetoothctl connect X failed: Failed to connect: org.bluez.Error.Failed"
        );
    }

    #[test]
    fn a_machine_without_bluetoothctl_says_so() {
        // Only when this machine has none, as CI's runners do not.
        if tool::run(BLUETOOTHCTL, &words(&["--version"]), Duration::from_secs(5)).is_err() {
            let message = format!("{:#}", read().unwrap_err());
            assert!(message.contains("has no bluetoothctl"), "{message}");
            assert!(message.contains("the bluetooth feature"), "{message}");
        }
    }
}
