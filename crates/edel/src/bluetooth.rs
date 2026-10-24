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
//! whatever BlueZ lists (ADR-011): nothing here assumes it is local. While
//! a pairing runs, Settings answers BlueZ's questions as an agent of its
//! own over D-Bus; the names it needs and the words for what can go wrong
//! are here, so the reading and the words stay written once. Std only.

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
pub const PAIR_TIME: Duration = Duration::from_secs(70);

/// How long a scan looks, in seconds.
pub const SCAN_SECONDS: u32 = 10;

/// The most devices read in one go: each is a call of its own.
const MOST_DEVICES: usize = 40;

/// BlueZ's name on the system bus.
pub const BLUEZ: &str = "org.bluez";

/// Where BlueZ's agent manager lives.
pub const BLUEZ_ROOT: &str = "/org/bluez";

/// The interfaces a pairing talks to.
pub const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
pub const AGENT: &str = "org.bluez.Agent1";
pub const DEVICE: &str = "org.bluez.Device1";

/// Where an agent of ours sits on the bus while a pairing runs.
pub const AGENT_PATH: &str = "/org/edel/bluetooth/agent";

/// What the agent can do: show a code and take a yes or a no. It cannot
/// take a code typed on this computer, so it never says it can.
pub const AGENT_CAPABILITY: &str = "DisplayYesNo";

/// The agent's interface, as BlueZ's doc/org.bluez.Agent.rst gives it.
pub const AGENT_XML: &str = r#"<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode"><arg name="device" type="o" direction="in"/><arg name="pincode" type="s" direction="out"/></method>
    <method name="DisplayPinCode"><arg name="device" type="o" direction="in"/><arg name="pincode" type="s" direction="in"/></method>
    <method name="RequestPasskey"><arg name="device" type="o" direction="in"/><arg name="passkey" type="u" direction="out"/></method>
    <method name="DisplayPasskey"><arg name="device" type="o" direction="in"/><arg name="passkey" type="u" direction="in"/><arg name="entered" type="q" direction="in"/></method>
    <method name="RequestConfirmation"><arg name="device" type="o" direction="in"/><arg name="passkey" type="u" direction="in"/></method>
    <method name="RequestAuthorization"><arg name="device" type="o" direction="in"/></method>
    <method name="AuthorizeService"><arg name="device" type="o" direction="in"/><arg name="uuid" type="s" direction="in"/></method>
    <method name="Cancel"/>
  </interface>
</node>"#;

/// The answer that refuses a question.
pub const REJECTED: &str = "org.bluez.Error.Rejected";

/// What Pair says about a device that is paired already: not a failure.
pub const ALREADY_PAIRED: &str = "org.bluez.Error.AlreadyExists";

/// What BlueZ asks the agent while a pairing runs, about the device at
/// `device` (its object path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Type `code` on the device, then Enter (DisplayPasskey, DisplayPinCode)
    Show { device: String, code: String },
    /// Does the device show `code`? (RequestConfirmation)
    Confirm { device: String, code: String },
    /// May the device go on (RequestAuthorization, AuthorizeService): yes
    /// for the device being paired, no for any other
    Allow { device: String },
    /// A code to type on this computer (RequestPinCode, RequestPasskey),
    /// which the agent refuses
    Type { device: String },
    /// BlueZ took its question back (Cancel) or let the agent go (Release)
    Gone,
}

/// A passkey as the device shows it: six figures, zeros in front.
pub fn passkey(number: u32) -> String {
    format!("{number:06}")
}

/// The address in a device's object path,
/// `/org/bluez/hci0/dev_A4_C3_F0_12_34_56` to `A4:C3:F0:12:34:56`; none
/// for any other path (an adapter, a device's service).
pub fn address_of_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    // ["", "org", "bluez", "hciN", "dev_..."]
    if parts.len() != 5 || parts[1] != "org" || parts[2] != "bluez" || !parts[3].starts_with("hci")
    {
        return None;
    }
    let address = parts[4].strip_prefix("dev_")?.replace('_', ":");
    is_address(&address).then_some(address)
}

/// The object path of the device at `address` among BlueZ's objects
/// (`paths`, the keys of ObjectManager's GetManagedObjects), else why not.
pub fn device_path<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    address: &str,
    name: &str,
) -> Result<String> {
    for path in paths {
        if address_of_path(path).is_some_and(|a| a.eq_ignore_ascii_case(address)) {
            return Ok(path.to_string());
        }
    }
    bail!(
        "{}",
        trf(
            "{name} is no longer nearby; search again, then pair",
            &[("name", name)]
        )
    )
}

/// Why a pairing failed, from the error BlueZ or the bus gave (`said`, the
/// D-Bus error's name, a colon and its message), in words that tell what
/// to do.
pub fn pair_failed(said: &str, timed_out: bool, name: &str) -> String {
    explain(
        &trf("pairing with {name}", &[("name", name)]),
        said,
        timed_out,
        name,
    )
}

/// What the page says when a device wants a code typed on this computer.
pub fn type_here(name: &str, address: &str) -> String {
    trf(
        "{name} wants a code typed on this computer, which Settings cannot take yet; type bluetoothctl in a terminal, then pair {address} at its prompt, and enter the code there",
        &[("name", name), ("address", address)],
    )
}

/// What the page says when the system bus does not answer.
pub fn bus_failed(error: &str) -> String {
    trf(
        "could not reach Bluetooth over the system bus: {error}; restart the computer if this stays",
        &[("error", error)],
    )
}

/// What the page says when a pairing is asked for while one runs.
pub fn pairing_runs() -> String {
    tr("a pairing is running already; finish it, then try again").to_string()
}

/// Bluetooth is not running though the computer has an adapter.
pub fn not_running() -> String {
    tr("Bluetooth is not running, though this computer has an adapter; it starts with the machine, so restart the computer if it stays off").to_string()
}

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

/// The adapter alone, without the devices it knows: two quick calls, for
/// the panel's quick settings (M5.9a), which only needs to know whether
/// there is one and whether it is on. None is no adapter, as in [`read`].
pub fn adapter() -> Result<Option<Adapter>> {
    let sysfs = Path::new(SYSFS);
    if !has_adapter(sysfs)? {
        return Ok(None);
    }
    let show = bluetoothctl(&words(&["show"]), READ_TIME)?;
    Ok(parse_show(&show.out))
}

/// Whether `bluetoothctl list` names an adapter. An adapter the kernel
/// has but `bluetoothd` does not answer for is an error, not none.
fn has_adapter(sysfs: &Path) -> Result<bool> {
    let list = bluetoothctl(&words(&["list"]), READ_TIME)?;
    if !list.ok() || parse_list(&list.out).is_empty() {
        if kernel_has_adapter(sysfs) {
            bail!("{}", not_running());
        }
        return Ok(false);
    }
    Ok(true)
}

fn read_in(sysfs: &Path) -> Result<Snapshot> {
    if !has_adapter(sysfs)? {
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

/// The arguments that pair with `address`, what the Pair row copies: the
/// agent has no keyboard and no screen, which is how headphones, speakers,
/// mice and most phones pair ("Just Works"). Settings itself pairs with an
/// agent of its own that shows a code and asks whether numbers match
/// (`crates/settings/src/pairing.rs`); on the command line the same is
/// `bluetoothctl`'s own prompt: type `bluetoothctl`, then `pair ADDRESS`.
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

/// Trusts a device that is paired, so it connects by itself next time, and
/// connects it: what follows a pairing.
pub fn trust_and_connect(address: &str, name: &str) -> Result<()> {
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

    #[test]
    fn a_passkey_keeps_its_zeros() {
        assert_eq!(passkey(42), "000042");
        assert_eq!(passkey(123456), "123456");
    }

    #[test]
    fn a_device_path_gives_its_address() {
        assert_eq!(
            address_of_path("/org/bluez/hci0/dev_A4_C3_F0_12_34_56").as_deref(),
            Some("A4:C3:F0:12:34:56")
        );
        for path in [
            "/org/bluez/hci0",
            "/org/bluez/hci0/dev_A4_C3_F0_12_34_56/service000a",
            "/org/bluez/hci0/dev_XY",
            "/org/edel/x/y/dev_A4_C3_F0_12_34_56",
        ] {
            assert_eq!(address_of_path(path), None, "{path}");
        }
    }

    #[test]
    fn the_device_is_found_among_bluezs_objects() {
        let paths = [
            "/org/bluez",
            "/org/bluez/hci1",
            "/org/bluez/hci1/dev_00_11_22_33_44_55",
            "/org/bluez/hci1/dev_00_11_22_33_44_55/service0001",
        ];
        assert_eq!(
            device_path(paths, "00:11:22:33:44:55", "Keys").unwrap(),
            "/org/bluez/hci1/dev_00_11_22_33_44_55"
        );
        assert_eq!(
            device_path(paths, "00:11:22:33:44:55".to_lowercase().as_str(), "Keys").unwrap(),
            "/org/bluez/hci1/dev_00_11_22_33_44_55"
        );
        let missing = device_path(paths, "66:77:88:99:AA:BB", "Keys").unwrap_err();
        assert_eq!(
            format!("{missing}"),
            "Keys is no longer nearby; search again, then pair"
        );
    }

    #[test]
    fn a_failed_pairing_says_what_to_do() {
        assert_eq!(
            pair_failed(
                "org.bluez.Error.AuthenticationFailed: Authentication Failed",
                false,
                "Keys"
            ),
            "Keys refused to pair; put it in pairing mode and try again"
        );
        assert_eq!(
            pair_failed("", true, "Keys"),
            "Keys did not answer; check that it is on and close by, then try again"
        );
        assert_eq!(
            pair_failed("org.bluez.Error.Failed: Input/output error", false, "Keys"),
            "pairing with Keys failed: org.bluez.Error.Failed: Input/output error"
        );
    }

    #[test]
    fn a_code_typed_here_says_how_to_pair_on_the_command_line() {
        assert_eq!(
            type_here("Keys", "00:11:22:33:44:55"),
            "Keys wants a code typed on this computer, which Settings cannot take yet; type bluetoothctl in a terminal, then pair 00:11:22:33:44:55 at its prompt, and enter the code there"
        );
    }

    #[test]
    fn the_agent_answers_no_more_than_it_says() {
        assert_eq!(AGENT_CAPABILITY, "DisplayYesNo");
        for method in [
            "Release",
            "RequestPinCode",
            "DisplayPinCode",
            "RequestPasskey",
            "DisplayPasskey",
            "RequestConfirmation",
            "RequestAuthorization",
            "AuthorizeService",
            "Cancel",
        ] {
            assert!(
                AGENT_XML.contains(&format!("<method name=\"{method}\"")),
                "{method}"
            );
        }
    }
}
