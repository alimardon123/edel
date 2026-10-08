//! Networks as NetworkManager reports them (roadmap M5.8a), through
//! `nmcli -t`: whether the machine is online, its wired and wireless
//! devices, the Wi-Fi networks in range with their signal and whether they
//! ask for a password, the connections it remembers, and the commands that
//! join, leave and forget. Settings' Network page reads and acts here, and
//! the panel's quick settings will (M5.9), so the reading is written once.
//!
//! Wi-Fi networks and their passwords are NetworkManager's own state, kept
//! in `/etc/NetworkManager/system-connections/` (root only), so no key of
//! the settings file holds them and the file never carries a secret
//! (ADR-006). `nmcli` is the one command line to this; a machine without it
//! (no `network` feature) says so. The list is generic on purpose
//! (ADR-011): a device is whatever NetworkManager lists, and nothing here
//! assumes it is local or that there is only one. Std only.

use std::time::Duration;

use anyhow::{Result, bail};

use crate::i18n::trf;
use crate::tool;

/// The program every call goes through, found on the `PATH`.
pub const NMCLI: &str = "nmcli";

/// How long a read may take before the page gives up on it.
const READ_TIME: Duration = Duration::from_secs(15);

/// How long joining a network may take: `nmcli` waits up to 90 s itself.
const JOIN_TIME: Duration = Duration::from_secs(100);

/// The columns of a line of `nmcli -t`, which separates them with `:` and
/// escapes a `:` or `\` in a value with a `\`: `a:b\:c` is `a` and `b:c`.
pub fn fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        let c = match c {
            ':' => {
                fields.push(String::new());
                continue;
            }
            '\\' => chars.next().unwrap_or('\\'),
            c => c,
        };
        if let Some(last) = fields.last_mut() {
            last.push(c);
        }
    }
    fields
}

/// How online the machine is, the first column of `nmcli general status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Connected to a network
    Connected,
    /// Connected, but only to this network or to nothing beyond it
    Limited,
    /// Joining a network, or leaving one
    Connecting,
    /// Not connected
    Disconnected,
    /// Networking is switched off, as in flight mode
    Asleep,
}

/// The text of `nmcli -t general status`, one line such as
/// `connected:full:enabled:enabled:enabled:enabled`: the state, the
/// connectivity, then the hardware and software switches of Wi-Fi and of
/// mobile broadband. Gives the state and whether Wi-Fi is switched on; none
/// when the text is not that.
pub fn parse_status(text: &str) -> Option<(State, bool)> {
    let f = fields(text.lines().find(|l| !l.trim().is_empty())?);
    let state = match f.first()?.as_str() {
        "connected" => State::Connected,
        "connected (site only)" | "connected (local only)" => State::Limited,
        "connecting" | "disconnecting" => State::Connecting,
        "disconnected" => State::Disconnected,
        "asleep" => State::Asleep,
        _ => return None,
    };
    let on = |at: usize| f.get(at).is_some_and(|s| s == "enabled");
    Some((state, on(2) && on(3)))
}

/// What kind of interface a device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ethernet,
    Wifi,
    /// Loopback, bridges, tunnels and the rest
    Other,
}

impl Kind {
    fn of_device(name: &str) -> Kind {
        match name {
            "ethernet" => Kind::Ethernet,
            "wifi" => Kind::Wifi,
            _ => Kind::Other,
        }
    }

    fn of_connection(name: &str) -> Kind {
        match name {
            "802-3-ethernet" => Kind::Ethernet,
            "802-11-wireless" => Kind::Wifi,
            _ => Kind::Other,
        }
    }
}

/// How far a device is from being connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Connected,
    Connecting,
    /// Ready, with nothing connected
    Disconnected,
    /// No cable, an adapter switched off, or something else in the way
    Unavailable,
    /// NetworkManager leaves it alone
    Unmanaged,
}

/// One interface, a line of `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION
/// device status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub kind: Kind,
    pub link: Link,
    /// The connection it uses, such as `Wired connection 1` or a Wi-Fi's name
    pub connection: Option<String>,
}

/// The devices in the text of `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION
/// device status`, lines such as `eth0:ethernet:connected:Wired connection 1`.
pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let [name, kind, state, connection] = <[String; 4]>::try_from(fields(line)).ok()?;
            if name.is_empty() {
                return None;
            }
            let link = match state.as_str() {
                s if s.starts_with("connected") && s.contains("externally") => Link::Unmanaged,
                s if s.starts_with("connected") => Link::Connected,
                s if s.starts_with("connecting") || s == "deactivating" => Link::Connecting,
                "disconnected" => Link::Disconnected,
                "unmanaged" => Link::Unmanaged,
                _ => Link::Unavailable,
            };
            Some(Device {
                name,
                kind: Kind::of_device(&kind),
                link,
                connection: Some(connection).filter(|c| !c.is_empty() && c != "--"),
            })
        })
        .collect()
}

/// A connection NetworkManager remembers, a line of `nmcli -t -f
/// NAME,TYPE,DEVICE connection show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// What `nmcli connection up id NAME` takes; a Wi-Fi's is its name
    pub name: String,
    pub kind: Kind,
    /// The device it is active on, none when it is not
    pub device: Option<String>,
}

/// The connections in the text of `nmcli -t -f NAME,TYPE,DEVICE connection
/// show`, lines such as `Home:802-11-wireless:wlan0`.
pub fn parse_connections(text: &str) -> Vec<Saved> {
    text.lines()
        .filter_map(|line| {
            let [name, kind, device] = <[String; 3]>::try_from(fields(line)).ok()?;
            if name.is_empty() {
                return None;
            }
            Some(Saved {
                name,
                kind: Kind::of_connection(&kind),
                device: Some(device).filter(|d| !d.is_empty() && d != "--"),
            })
        })
        .collect()
}

/// A Wi-Fi network in range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wifi {
    pub ssid: String,
    /// 0 to 100
    pub signal: u8,
    /// Whether it asks for a password
    pub secured: bool,
    /// Whether this machine is connected to it
    pub in_use: bool,
    /// Whether the machine remembers it, so joining it asks for nothing
    pub saved: bool,
}

impl Wifi {
    /// The signal as one to four bars.
    pub fn bars(&self) -> u8 {
        match self.signal {
            0..=24 => 1,
            25..=49 => 2,
            50..=74 => 3,
            _ => 4,
        }
    }
}

/// The networks in the text of `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY
/// device wifi list`, lines such as `*:Home:78:WPA2`: one for each name (a
/// network with several access points shows the one in use, else the
/// strongest), the one in use first, then the strongest first. A hidden
/// network has no name and is left out. `saved` is false: this text does
/// not say, [`Snapshot`] fills it in.
pub fn parse_wifi(text: &str) -> Vec<Wifi> {
    let mut found: Vec<Wifi> = Vec::new();
    for line in text.lines() {
        let Ok([in_use, ssid, signal, security]) = <[String; 4]>::try_from(fields(line)) else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let network = Wifi {
            signal: signal.trim().parse().unwrap_or(0).min(100),
            secured: !security.is_empty() && security != "--",
            in_use: in_use.trim() == "*",
            saved: false,
            ssid,
        };
        match found.iter_mut().find(|w| w.ssid == network.ssid) {
            Some(same) if (network.in_use, network.signal) > (same.in_use, same.signal) => {
                *same = network;
            }
            Some(_) => {}
            None => found.push(network),
        }
    }
    found.sort_by_key(|w| std::cmp::Reverse((w.in_use, w.signal)));
    found
}

/// How the machine is connected, for the card at the head of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// Through a cable
    Wired { limited: bool },
    /// Through the Wi-Fi network of this name
    Wireless { ssid: String, limited: bool },
    /// Joining a network
    Connecting,
    /// Switched off, as in flight mode
    Asleep,
    /// Connected to nothing
    Offline,
}

/// Everything the Network page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    /// Whether Wi-Fi is switched on (and its adapter's switch is not off)
    pub wifi_on: bool,
    pub devices: Vec<Device>,
    /// The Wi-Fi networks in range; empty with Wi-Fi off or no adapter
    pub networks: Vec<Wifi>,
    pub saved: Vec<Saved>,
}

impl Snapshot {
    /// Whether the machine has a Wi-Fi adapter NetworkManager manages.
    pub fn has_wifi(&self) -> bool {
        self.devices.iter().any(|d| d.kind == Kind::Wifi)
    }

    /// The first wired device, which the page shows as the wired connection.
    pub fn wired(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.kind == Kind::Ethernet)
    }

    /// How the machine is connected: a cable wins over Wi-Fi, as it does
    /// for the route NetworkManager prefers.
    pub fn summary(&self) -> Summary {
        let limited = self.state == State::Limited;
        let on = |kind| {
            self.devices
                .iter()
                .find(|d| d.kind == kind && d.link == Link::Connected)
        };
        if self.state == State::Asleep {
            Summary::Asleep
        } else if on(Kind::Ethernet).is_some() {
            Summary::Wired { limited }
        } else if let Some(device) = on(Kind::Wifi) {
            let ssid = self
                .networks
                .iter()
                .find(|w| w.in_use)
                .map(|w| w.ssid.clone())
                .or_else(|| device.connection.clone())
                .unwrap_or_default();
            Summary::Wireless { ssid, limited }
        } else if self.state == State::Connecting
            || self.devices.iter().any(|d| d.link == Link::Connecting)
        {
            Summary::Connecting
        } else {
            Summary::Offline
        }
    }
}

/// The address facts of one device, `nmcli -t device show`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail {
    pub device: String,
    pub hardware: Option<String>,
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
}

/// The devices in the text of `nmcli -t -f GENERAL.DEVICE,GENERAL.HWADDR,
/// IP4.ADDRESS,IP4.GATEWAY,IP4.DNS device show`, lines such as
/// `IP4.ADDRESS[1]:10.0.2.15/24`; a new device starts at its
/// `GENERAL.DEVICE`. Empty values are left out.
pub fn parse_details(text: &str) -> Vec<Detail> {
    let mut details: Vec<Detail> = Vec::new();
    for line in text.lines() {
        let f = fields(line);
        let (Some(key), value) = (f.first(), f.get(1..).unwrap_or_default().join(":")) else {
            continue;
        };
        // `IP4.ADDRESS[1]` and `IP4.ADDRESS[2]` are the same key.
        let key = key.split('[').next().unwrap_or_default();
        if key == "GENERAL.DEVICE" {
            details.push(Detail {
                device: value,
                ..Detail::default()
            });
            continue;
        }
        let (Some(current), false) = (details.last_mut(), value.is_empty() || value == "--") else {
            continue;
        };
        match key {
            "GENERAL.HWADDR" => current.hardware = Some(value),
            "IP4.ADDRESS" => current.addresses.push(value),
            "IP4.GATEWAY" => current.gateway = Some(value),
            "IP4.DNS" => current.dns.push(value),
            _ => {}
        }
    }
    details
}

/// `nmcli` with `args`, giving what it printed, or why it could not.
fn nmcli(args: &[String], time: Duration) -> Result<String> {
    let line = tool::command_line(NMCLI, args);
    let done = match tool::run(NMCLI, args, time) {
        Ok(done) => done,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "{}",
            trf(
                "this machine has no {program}, so it has no network manager; the network feature brings NetworkManager",
                &[("program", NMCLI)]
            )
        ),
        Err(e) => bail!(
            "{}",
            trf(
                "could not run {program}: {error}",
                &[("program", NMCLI), ("error", &e.to_string())]
            )
        ),
    };
    if done.timed_out {
        bail!(
            "{}",
            trf(
                "{command} did not answer within {seconds} s; NetworkManager may be busy, so try again in a moment",
                &[("command", &line), ("seconds", &time.as_secs().to_string())]
            )
        );
    }
    if !done.ok() {
        bail!("{}", explain(&line, &done.err, &done.out));
    }
    Ok(done.out)
}

/// What a failed `nmcli` said, in words that tell what to do.
fn explain(line: &str, err: &str, out: &str) -> String {
    let said = if err.trim().is_empty() { out } else { err };
    let said = said.trim().trim_start_matches("Error: ");
    if said.contains("NetworkManager is not running") {
        return crate::i18n::tr(
            "NetworkManager is not running, so there is no network to show; it starts with the machine, so restart the computer if it stays off",
        )
        .to_string();
    }
    if said.contains("Not authorized") || said.contains("not authorized") {
        return crate::i18n::tr(
            "NetworkManager refused, because only the people at this machine (group seat) may change the network",
        )
        .to_string();
    }
    trf(
        "{command} failed: {said}",
        &[("command", line), ("said", said)],
    )
}

/// `nmcli` with `args` as a line to type, what Copy as command shows.
pub fn command_line(args: &[String]) -> String {
    tool::command_line(NMCLI, args)
}

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// The arguments that switch Wi-Fi on or off.
pub fn radio_args(on: bool) -> Vec<String> {
    words(&["radio", "wifi", if on { "on" } else { "off" }])
}

/// The arguments that join the Wi-Fi network `ssid`, with `password` when
/// it is a new one that asks for it.
pub fn join_args(ssid: &str, password: Option<&str>) -> Vec<String> {
    let mut args = words(&["device", "wifi", "connect", ssid]);
    if let Some(password) = password {
        args.extend(words(&["password", password]));
    }
    args
}

/// The arguments that join a connection the machine remembers.
pub fn up_args(name: &str) -> Vec<String> {
    words(&["connection", "up", "id", name])
}

/// The arguments that make the machine forget a connection and its
/// password.
pub fn forget_args(name: &str) -> Vec<String> {
    words(&["connection", "delete", "id", name])
}

/// The arguments that disconnect a device.
pub fn leave_args(device: &str) -> Vec<String> {
    words(&["device", "disconnect", device])
}

/// What the page shows now: the state, the devices, and the networks in
/// range with the ones the machine remembers marked.
pub fn read() -> Result<Snapshot> {
    let run = |args: &[&str]| nmcli(&words(args), READ_TIME);
    let status = run(&["-t", "general", "status"])?;
    let Some((state, wifi_on)) = parse_status(&status) else {
        bail!(
            "{}",
            trf(
                "could not read the state of the network from nmcli, which printed {text:?}",
                &[("text", status.trim())]
            )
        );
    };
    let devices = parse_devices(&run(&[
        "-t",
        "-f",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])?);
    let saved = parse_connections(&run(&[
        "-t",
        "-f",
        "NAME,TYPE,DEVICE",
        "connection",
        "show",
    ])?);
    let mut networks = Vec::new();
    if wifi_on && devices.iter().any(|d| d.kind == Kind::Wifi) {
        // A list that cannot be read is an empty one: the rest still shows.
        let list = run(&[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
        ])
        .unwrap_or_default();
        networks = parse_wifi(&list);
        for network in &mut networks {
            network.saved = saved
                .iter()
                .any(|s| s.kind == Kind::Wifi && s.name == network.ssid);
        }
    }
    Ok(Snapshot {
        state,
        wifi_on,
        devices,
        networks,
        saved,
    })
}

/// The addresses of each device, for the page's details.
pub fn details() -> Result<Vec<Detail>> {
    let text = nmcli(
        &words(&[
            "-t",
            "-f",
            "GENERAL.DEVICE,GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
            "device",
            "show",
        ]),
        READ_TIME,
    )?;
    Ok(parse_details(&text))
}

/// Asks the Wi-Fi adapters to look again. A scan too soon after the last
/// one is refused and does no harm, so the answer is ignored.
pub fn rescan() {
    let _ = nmcli(&words(&["device", "wifi", "rescan"]), READ_TIME);
}

/// Switches Wi-Fi on or off.
pub fn set_wifi(on: bool) -> Result<()> {
    nmcli(&radio_args(on), READ_TIME).map(drop)
}

/// Joins the Wi-Fi network `ssid`: a remembered one by its name, a new one
/// with `password` when it asks for one. A new network that could not be
/// joined is not kept, so a wrong password leaves nothing behind.
pub fn join(ssid: &str, saved: bool, password: Option<&str>) -> Result<()> {
    let args = if saved {
        up_args(ssid)
    } else {
        join_args(ssid, password)
    };
    match nmcli(&args, JOIN_TIME) {
        Ok(_) => Ok(()),
        Err(e) => {
            if !saved {
                let _ = forget(ssid);
            }
            let text = format!("{e:#}");
            if text.contains("Secrets were required") || text.contains("secrets were required") {
                bail!(
                    "{}",
                    trf(
                        "the password for {ssid} was missing or wrong; try again with the right one",
                        &[("ssid", ssid)]
                    )
                );
            }
            if text.contains("No network with SSID") {
                bail!(
                    "{}",
                    trf(
                        "{ssid} is not in range now; move closer and try again",
                        &[("ssid", ssid)]
                    )
                );
            }
            Err(e)
        }
    }
}

/// Makes the machine forget a connection and its password.
pub fn forget(name: &str) -> Result<()> {
    nmcli(&forget_args(name), READ_TIME).map(drop)
}

/// Disconnects a device.
pub fn leave(device: &str) -> Result<()> {
    nmcli(&leave_args(device), READ_TIME).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIFI: &str = "\
 :Cafe Guest:34:--
*:Home:78:WPA2
 :Home:52:WPA2
 :Neighbour\\: 5G:90:WPA2 WPA3
 ::40:WPA2
 :Open Air:61:
 :Weak Open:12:--
";

    #[test]
    fn a_colon_or_backslash_in_a_value_is_escaped_by_nmcli() {
        assert_eq!(fields("a:b"), ["a", "b"]);
        assert_eq!(fields(r"a:b\:c:d"), ["a", "b:c", "d"]);
        assert_eq!(fields(r"back\\slash:x"), [r"back\slash", "x"]);
        assert_eq!(fields(""), [""]);
        assert_eq!(fields("::"), ["", "", ""]);
        // A lone backslash at the end is kept.
        assert_eq!(fields(r"a\"), [r"a\"]);
    }

    #[test]
    fn the_state_and_the_wifi_switch_come_from_general_status() {
        assert_eq!(
            parse_status("connected:full:enabled:enabled:enabled:enabled\n"),
            Some((State::Connected, true))
        );
        assert_eq!(
            parse_status("disconnected:none:enabled:disabled:missing:enabled"),
            Some((State::Disconnected, false))
        );
        // The hardware switch off means off, whatever the software says.
        assert_eq!(
            parse_status("connected (site only):limited:disabled:enabled:enabled:enabled"),
            Some((State::Limited, false))
        );
        assert_eq!(
            parse_status("connecting:none:enabled:enabled:enabled:enabled").map(|s| s.0),
            Some(State::Connecting)
        );
        assert_eq!(
            parse_status("asleep:unknown:enabled:enabled:enabled:enabled").map(|s| s.0),
            Some(State::Asleep)
        );
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("Error: NetworkManager is not running."), None);
    }

    #[test]
    fn devices_are_read_with_their_kind_link_and_connection() {
        let devices = parse_devices(
            "eth0:ethernet:connected:Wired connection 1\n\
             wlan0:wifi:disconnected:\n\
             wlan1:wifi:connecting (getting IP configuration):Home\n\
             enp3s0:ethernet:unavailable:\n\
             lo:loopback:connected (externally):lo\n\
             br0:bridge:unmanaged:--\n",
        );
        assert_eq!(devices.len(), 6);
        assert_eq!(
            devices[0],
            Device {
                name: "eth0".into(),
                kind: Kind::Ethernet,
                link: Link::Connected,
                connection: Some("Wired connection 1".into()),
            }
        );
        assert_eq!(
            (devices[1].kind, devices[1].link, &devices[1].connection),
            (Kind::Wifi, Link::Disconnected, &None)
        );
        assert_eq!(devices[2].link, Link::Connecting);
        assert_eq!(devices[3].link, Link::Unavailable);
        assert_eq!(
            (devices[4].kind, devices[4].link),
            (Kind::Other, Link::Unmanaged)
        );
        assert_eq!(devices[5].connection, None);
    }

    #[test]
    fn connections_are_read_with_their_device_if_active() {
        let saved = parse_connections(
            "Wired connection 1:802-3-ethernet:eth0\nHome:802-11-wireless:wlan0\n\
             Old\\: flat:802-11-wireless:--\nlo:loopback:lo\n",
        );
        assert_eq!(saved.len(), 4);
        assert_eq!(saved[0].kind, Kind::Ethernet);
        assert_eq!(saved[1].device.as_deref(), Some("wlan0"));
        assert_eq!(saved[2].name, "Old: flat");
        assert_eq!(saved[2].device, None);
        assert_eq!(saved[3].kind, Kind::Other);
    }

    #[test]
    fn wifi_networks_are_listed_once_each_in_use_first_then_strongest() {
        let list = parse_wifi(WIFI);
        let names: Vec<&str> = list.iter().map(|w| w.ssid.as_str()).collect();
        // Home is in use; a name with a colon is whole; the hidden network
        // (no name) is left out; Home's second access point is not a
        // second entry.
        assert_eq!(
            names,
            [
                "Home",
                "Neighbour: 5G",
                "Open Air",
                "Cafe Guest",
                "Weak Open"
            ]
        );
        let home = &list[0];
        assert!(home.in_use && home.secured);
        assert_eq!(home.signal, 78);
        assert_eq!(home.bars(), 4);
        let open = &list[2];
        assert!(!open.secured, "an empty security column is an open network");
        assert!(!list[3].secured, "-- is an open network too");
        assert_eq!((list[4].signal, list[4].bars()), (12, 1));
        assert!(list.iter().all(|w| !w.saved));
    }

    #[test]
    fn the_strongest_access_point_of_a_network_is_the_one_shown() {
        let list = parse_wifi(" :Mesh:30:WPA2\n :Mesh:80:WPA2\n :Mesh:55:WPA2\n");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].signal, 80);
        // The access point in use wins over a stronger one.
        let list = parse_wifi(" :Mesh:90:WPA2\n*:Mesh:40:WPA2\n");
        assert_eq!((list[0].signal, list[0].in_use), (40, true));
    }

    #[test]
    fn nothing_in_range_is_an_empty_list() {
        assert!(parse_wifi("").is_empty());
        assert!(parse_wifi("not nmcli at all").is_empty());
        assert!(parse_wifi("Error: Wi-Fi is disabled").is_empty());
    }

    fn snapshot(state: State, devices: &str) -> Snapshot {
        Snapshot {
            state,
            wifi_on: true,
            devices: parse_devices(devices),
            networks: parse_wifi("*:Home:78:WPA2\n"),
            saved: Vec::new(),
        }
    }

    #[test]
    fn the_summary_says_how_the_machine_is_connected() {
        let wired = snapshot(
            State::Connected,
            "eth0:ethernet:connected:Wired connection 1\nwlan0:wifi:connected:Home\n",
        );
        assert_eq!(wired.summary(), Summary::Wired { limited: false });
        let wireless = snapshot(
            State::Connected,
            "eth0:ethernet:unavailable:\nwlan0:wifi:connected:Home\n",
        );
        assert_eq!(
            wireless.summary(),
            Summary::Wireless {
                ssid: "Home".into(),
                limited: false
            }
        );
        let limited = snapshot(State::Limited, "wlan0:wifi:connected:Home\n");
        assert_eq!(
            limited.summary(),
            Summary::Wireless {
                ssid: "Home".into(),
                limited: true
            }
        );
        assert_eq!(
            snapshot(State::Connecting, "wlan0:wifi:connecting (prepare):Home\n").summary(),
            Summary::Connecting
        );
        assert_eq!(
            snapshot(
                State::Disconnected,
                "eth0:ethernet:unavailable:\nlo:loopback:unmanaged:\n"
            )
            .summary(),
            Summary::Offline
        );
        assert_eq!(
            snapshot(State::Asleep, "eth0:ethernet:unavailable:\n").summary(),
            Summary::Asleep
        );
        // The loopback device alone is not a connection.
        assert_eq!(
            snapshot(
                State::Disconnected,
                "lo:loopback:connected (externally):lo\n"
            )
            .summary(),
            Summary::Offline
        );
    }

    #[test]
    fn the_wired_device_and_the_wifi_adapter_are_found() {
        let s = snapshot(
            State::Disconnected,
            "lo:loopback:unmanaged:\neth0:ethernet:connected:x\n",
        );
        assert_eq!(s.wired().map(|d| d.name.as_str()), Some("eth0"));
        assert!(!s.has_wifi());
        assert!(snapshot(State::Disconnected, "wlan0:wifi:disconnected:\n").has_wifi());
    }

    #[test]
    fn the_addresses_of_each_device_are_read() {
        let details = parse_details(
            "GENERAL.DEVICE:eth0\n\
             GENERAL.HWADDR:52\\:54\\:00\\:12\\:34\\:56\n\
             IP4.ADDRESS[1]:10.0.2.15/24\n\
             IP4.GATEWAY:10.0.2.2\n\
             IP4.DNS[1]:10.0.2.3\n\
             IP4.DNS[2]:1.1.1.1\n\
             GENERAL.DEVICE:wlan0\n\
             GENERAL.HWADDR:aa\\:bb\\:cc\\:dd\\:ee\\:ff\n\
             IP4.ADDRESS[1]:\n\
             IP4.GATEWAY:\n",
        );
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].device, "eth0");
        assert_eq!(details[0].hardware.as_deref(), Some("52:54:00:12:34:56"));
        assert_eq!(details[0].addresses, ["10.0.2.15/24"]);
        assert_eq!(details[0].gateway.as_deref(), Some("10.0.2.2"));
        assert_eq!(details[0].dns, ["10.0.2.3", "1.1.1.1"]);
        assert!(details[1].addresses.is_empty() && details[1].gateway.is_none());
        assert!(parse_details("IP4.ADDRESS[1]:1.2.3.4/8\n").is_empty());
    }

    #[test]
    fn the_commands_are_nmclis() {
        assert_eq!(radio_args(false), ["radio", "wifi", "off"]);
        assert_eq!(
            command_line(&join_args("Cafe Guest", Some("secret one"))),
            "nmcli device wifi connect 'Cafe Guest' password 'secret one'"
        );
        assert_eq!(
            join_args("Open Air", None),
            ["device", "wifi", "connect", "Open Air"]
        );
        assert_eq!(
            command_line(&up_args("Home")),
            "nmcli connection up id Home"
        );
        assert_eq!(
            command_line(&forget_args("Old: flat")),
            "nmcli connection delete id 'Old: flat'"
        );
        assert_eq!(
            command_line(&leave_args("wlan0")),
            "nmcli device disconnect wlan0"
        );
    }

    #[test]
    fn a_failure_says_what_to_do() {
        assert!(
            explain(
                "nmcli -t general status",
                "Error: NetworkManager is not running.\n",
                ""
            )
            .contains("restart the computer")
        );
        assert!(
            explain(
                "nmcli radio wifi on",
                "Error: Not authorized to control networking.",
                ""
            )
            .contains("group seat")
        );
        assert_eq!(
            explain("nmcli device wifi rescan", "Error: Wi-Fi is disabled.", ""),
            "nmcli device wifi rescan failed: Wi-Fi is disabled."
        );
        // What it said on standard output, when it said nothing on error.
        assert!(explain("nmcli x", "", "oops").ends_with("failed: oops"));
    }

    #[test]
    fn a_machine_without_nmcli_says_so() {
        // Only when this machine has none, as CI's runners do not.
        if tool::run(NMCLI, &[], Duration::from_secs(1)).is_err() {
            let message = format!("{:#}", read().unwrap_err());
            assert!(message.contains("has no nmcli"), "{message}");
            assert!(message.contains("the network feature"), "{message}");
        }
    }
}
