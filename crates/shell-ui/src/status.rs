//! What the panel's status area and quick settings show (M5.9a): how the
//! machine is connected, how loud it is and how full its battery is, as
//! the modules Settings' pages use say it (`edel::network`,
//! `edel::sound`, `edel::power`, `edel::bluetooth`), so each is read in
//! one place. A piece the machine lacks, because its feature is not
//! installed or its daemon does not answer, is simply not there: the
//! icon and the tile are left out, nothing is guessed.
//!
//! Reading runs commands (`nmcli`, `wpctl`, `upower`, `bluetoothctl`), so
//! it is never done on the thread that draws: [`read`] and [`Cmd::run`]
//! are called from a thread of their own, and their answer comes back to
//! the event loop as a [`Msg`]. When it is read is main.rs's: when the
//! card opens, after a change made through it, on the clock's minute and
//! when the system bus says NetworkManager or UPower changed something
//! (`watch.rs`); never on a timer of its own.

use std::path::Path;

use anyhow::Result;
use edel::{bluetooth, network, power, sound};

/// How the machine is connected, the first part of the status area.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub link: Link,
    /// Whether the machine has a Wi-Fi adapter at all.
    pub has_wifi: bool,
    /// Whether its Wi-Fi is switched on.
    pub wifi_on: bool,
}

/// What the machine is connected through.
#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    /// A cable
    Wired,
    /// The Wi-Fi network `name`, `bars` of 3 strong
    Wifi { name: String, bars: u8 },
    /// Joining a network
    Connecting,
    /// Nothing
    Offline,
}

/// The output in use and how loud it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub percent: u32,
    pub muted: bool,
    /// The output's name, as PipeWire lists it.
    pub output: String,
}

/// One output (a sink) the person may choose.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

/// The battery of the machine, as UPower's display device says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    /// "82 percent, 4 hours 10 minutes left"
    pub line: String,
}

/// Everything the status area and quick settings show; each part is none
/// when the machine has no such thing, or its daemon does not answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub network: Option<Network>,
    pub volume: Option<Volume>,
    pub outputs: Vec<Output>,
    pub battery: Option<Battery>,
    /// Whether the Bluetooth adapter is on, none without an adapter.
    pub bluetooth: Option<bool>,
}

impl Status {
    /// Whether every radio the machine has is off: flight mode. A machine
    /// with no radio is never in it.
    pub fn airplane(&self) -> bool {
        let wifi = self
            .network
            .as_ref()
            .filter(|n| n.has_wifi)
            .map(|n| n.wifi_on);
        let radios: Vec<bool> = [wifi, self.bluetooth].into_iter().flatten().collect();
        !radios.is_empty() && radios.iter().all(|on| !on)
    }

    /// Whether the machine has a Wi-Fi adapter.
    pub fn has_wifi(&self) -> bool {
        self.network.as_ref().is_some_and(|n| n.has_wifi)
    }

    /// Whether anything in it differs from `other`, ignoring what `other`
    /// did not read: a read without Bluetooth keeps the one before's.
    pub fn merged(mut self, before: &Status, bluetooth_read: bool) -> Status {
        if !bluetooth_read {
            self.bluetooth = before.bluetooth;
        }
        self
    }
}

/// A name from outside shown on one line: control characters (a tab or a
/// newline in an access point's name) become spaces.
fn plain(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The network part of `snapshot`.
pub fn from_network(snapshot: &network::Snapshot) -> Network {
    let link = match snapshot.summary() {
        network::Summary::Wired { .. } => Link::Wired,
        network::Summary::Wireless { ssid, .. } => {
            // The list may lack the network in use for a moment; call it
            // strong then, as it is connected.
            let bars = snapshot
                .networks
                .iter()
                .find(|w| w.in_use)
                .map_or(3, network::Wifi::bars);
            Link::Wifi {
                name: plain(&ssid),
                // Four bars, three arcs.
                bars: bars.min(3),
            }
        }
        network::Summary::Connecting => Link::Connecting,
        network::Summary::Asleep | network::Summary::Offline => Link::Offline,
    };
    Network {
        link,
        has_wifi: snapshot.has_wifi(),
        wifi_on: snapshot.wifi_on,
    }
}

/// The volume of the output in use and every output.
pub fn from_sound(devices: &sound::Devices) -> (Option<Volume>, Vec<Output>) {
    let volume = devices.sinks.iter().find(|d| d.default).map(|d| Volume {
        percent: d.volume.unwrap_or(0),
        muted: d.muted,
        output: plain(&d.name),
    });
    let outputs = devices
        .sinks
        .iter()
        .map(|d| Output {
            id: d.id,
            name: plain(&d.name),
            default: d.default,
        })
        .collect();
    (volume, outputs)
}

/// The battery of `snapshot`, none for a machine without one or one that
/// does not say how full it is.
pub fn from_power(snapshot: &power::Snapshot) -> Option<Battery> {
    let battery = snapshot.battery()?;
    Some(Battery {
        percent: battery.percent_whole()?,
        charging: battery.state == power::State::Charging,
        line: power::battery_line(battery),
    })
}

/// Reads what is there to read: a piece only when `features` (the
/// machine's `/usr/share/edel/features/`) has its file, and it answers.
/// Bluetooth is read only when `bluetooth` says so, as it takes two more
/// commands and only the card needs it.
pub fn read(features: &Path, bluetooth: bool) -> Status {
    let has = |name: &str| features.join(format!("{name}.toml")).is_file();
    let mut status = Status::default();
    if has("network") {
        status.network = network::read().ok().map(|s| from_network(&s));
    }
    if has("sound") {
        if let Ok(devices) = sound::read() {
            (status.volume, status.outputs) = from_sound(&devices);
        }
    }
    if has("power") {
        status.battery = power::read().ok().and_then(|s| from_power(&s));
    }
    if bluetooth && has("bluetooth") {
        status.bluetooth = bluetooth::adapter().ok().flatten().map(|a| a.powered);
    }
    status
}

/// What a person's click on the card asks the system to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Wifi(bool),
    Bluetooth(bool),
    /// Every radio the machine has off or on.
    Airplane {
        on: bool,
        wifi: bool,
        bluetooth: bool,
    },
    Volume(u32),
    Mute(bool),
    Output(u32),
}

impl Cmd {
    /// What it does, in the words of a message: "turn Wi-Fi off".
    pub fn what(&self) -> String {
        let switch =
            |name: &str, on: bool| format!("turn {name} {}", if on { "on" } else { "off" });
        match self {
            Cmd::Wifi(on) => switch("Wi-Fi", *on),
            Cmd::Bluetooth(on) => switch("Bluetooth", *on),
            Cmd::Airplane { on, .. } => {
                format!("turn airplane mode {}", if *on { "on" } else { "off" })
            }
            Cmd::Volume(p) => format!("set the volume to {p} percent"),
            Cmd::Mute(on) => format!("{} the sound", if *on { "mute" } else { "unmute" }),
            Cmd::Output(_) => "choose the output".to_string(),
        }
    }

    /// Runs it, waiting for the program; call it from a thread of its
    /// own, never from the one that draws.
    pub fn run(&self) -> Result<()> {
        let sink = sound::Kind::Sink.default_name();
        match self {
            Cmd::Wifi(on) => network::set_wifi(*on),
            Cmd::Bluetooth(on) => bluetooth::set_power(*on),
            Cmd::Airplane {
                on,
                wifi,
                bluetooth,
            } => {
                // Flight mode is every radio off, and back on is every one
                // on; one that fails does not stop the other.
                let first = if *wifi {
                    network::set_radios(!on)
                } else {
                    Ok(())
                };
                let second = if *bluetooth {
                    bluetooth::set_power(!on)
                } else {
                    Ok(())
                };
                first.and(second)
            }
            Cmd::Volume(percent) => {
                sound::set_volume(sink, *percent)?;
                // Dragging the slider up from silence is meant to be heard.
                if *percent > 0 {
                    sound::set_mute(sink, false)?;
                }
                Ok(())
            }
            Cmd::Mute(on) => sound::set_mute(sink, *on),
            Cmd::Output(id) => sound::set_default(*id),
        }
    }
}

/// What the threads that read and run tell the event loop.
#[derive(Debug)]
pub enum Msg {
    /// What was read, after `ran` if a command ran first, with whether
    /// Bluetooth was read too; a command that failed brings its message.
    Read {
        status: Box<Status>,
        bluetooth: bool,
        ran: Option<(Cmd, Option<String>)>,
    },
    /// NetworkManager or UPower said something changed.
    Changed,
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERAL: &str = "connected:full:enabled:enabled:enabled:enabled\n";
    const DEVICES: &str =
        "wlan0:wifi:connected:Home 5G\neth0:ethernet:unavailable:\nlo:loopback:unmanaged:\n";
    const WIFI: &str = "*:Home 5G:78:WPA2\n :Cafe:40:WPA2\n";

    fn snapshot(general: &str, devices: &str, wifi: &str) -> network::Snapshot {
        let (state, wifi_on) = network::parse_status(general).unwrap();
        network::Snapshot {
            state,
            wifi_on,
            devices: network::parse_devices(devices),
            networks: network::parse_wifi(wifi),
            saved: Vec::new(),
        }
    }

    #[test]
    fn wifi_in_use_names_the_network_and_its_bars_up_to_three() {
        let net = from_network(&snapshot(GENERAL, DEVICES, WIFI));
        assert_eq!(
            net.link,
            Link::Wifi {
                name: "Home 5G".into(),
                bars: 3
            }
        );
        assert!(net.has_wifi && net.wifi_on);
        let weak = from_network(&snapshot(GENERAL, DEVICES, "*:Home 5G:30:WPA2\n"));
        assert_eq!(
            weak.link,
            Link::Wifi {
                name: "Home 5G".into(),
                bars: 2
            }
        );
    }

    #[test]
    fn a_cable_wins_and_nothing_connected_is_offline() {
        let wired = "eth0:ethernet:connected:Wired connection 1\nwlan0:wifi:disconnected:\n";
        assert_eq!(
            from_network(&snapshot(GENERAL, wired, "")).link,
            Link::Wired
        );
        let off = "disconnected:none:enabled:disabled:enabled:enabled\n";
        let net = from_network(&snapshot(off, "wlan0:wifi:unavailable:\n", ""));
        assert_eq!(net.link, Link::Offline);
        assert!(net.has_wifi && !net.wifi_on);
        let machine = from_network(&snapshot(
            "disconnected:none:enabled:enabled:enabled:enabled\n",
            "lo:loopback:unmanaged:\n",
            "",
        ));
        assert!(!machine.has_wifi, "a machine with no adapter says so");
    }

    #[test]
    fn a_network_name_with_a_tab_stays_on_one_line() {
        assert_eq!(plain("a\tb\nc"), "a b c");
    }

    #[test]
    fn the_output_in_use_gives_the_volume_and_all_are_listed() {
        let devices = sound::parse(
            "Audio\n ├─ Sinks:\n │  *   48. Built-in Audio Analog Stereo   [vol: 0.64 MUTED]\n │      52. HDMI Output   [vol: 1.00]\n",
        );
        let (volume, outputs) = from_sound(&devices);
        assert_eq!(
            volume,
            Some(Volume {
                percent: 64,
                muted: true,
                output: "Built-in Audio Analog Stereo".into()
            })
        );
        assert_eq!(outputs.len(), 2);
        assert!(outputs[0].default && !outputs[1].default);
        assert_eq!(from_sound(&sound::Devices::default()), (None, Vec::new()));
    }

    #[test]
    fn a_battery_gives_its_charge_and_its_line_and_none_gives_none() {
        let dump = "Device: /org/freedesktop/UPower/devices/DisplayDevice\n  battery\n    present:             yes\n    state:               charging\n    time to full:        41.0 minutes\n    percentage:          82%\n\nDaemon:\n  on-battery:      no\n";
        let battery = from_power(&power::parse(dump)).unwrap();
        assert_eq!(battery.percent, 82);
        assert!(battery.charging);
        assert_eq!(battery.line, "82 percent, 41 minutes until full");
        assert_eq!(
            from_power(&power::parse("Daemon:\n  on-battery: no\n")),
            None
        );
    }

    #[test]
    fn flight_mode_is_every_radio_the_machine_has_off() {
        let net = |has_wifi, wifi_on| Network {
            link: Link::Offline,
            has_wifi,
            wifi_on,
        };
        let status = |network: Option<Network>, bluetooth| Status {
            network,
            bluetooth,
            ..Status::default()
        };
        assert!(!status(None, None).airplane(), "no radio, no flight mode");
        assert!(!status(Some(net(false, false)), None).airplane());
        assert!(status(Some(net(true, false)), None).airplane());
        assert!(!status(Some(net(true, true)), None).airplane());
        assert!(status(Some(net(true, false)), Some(false)).airplane());
        assert!(!status(Some(net(true, false)), Some(true)).airplane());
        assert!(status(None, Some(false)).airplane());
    }

    #[test]
    fn a_read_without_bluetooth_keeps_what_was_known() {
        let before = Status {
            bluetooth: Some(true),
            ..Status::default()
        };
        let now = Status::default();
        assert_eq!(now.clone().merged(&before, false).bluetooth, Some(true));
        assert_eq!(now.merged(&before, true).bluetooth, None);
    }

    #[test]
    fn a_machine_without_the_features_reads_nothing() {
        let dir = std::env::temp_dir().join(format!("edel-status-features-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read(&dir, true), Status::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_command_says_what_it_does() {
        assert_eq!(Cmd::Wifi(false).what(), "turn Wi-Fi off");
        assert_eq!(Cmd::Bluetooth(true).what(), "turn Bluetooth on");
        assert_eq!(
            Cmd::Airplane {
                on: true,
                wifi: true,
                bluetooth: true
            }
            .what(),
            "turn airplane mode on"
        );
        assert_eq!(Cmd::Volume(40).what(), "set the volume to 40 percent");
        assert_eq!(Cmd::Mute(true).what(), "mute the sound");
    }
}
