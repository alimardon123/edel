//! The battery and the power source as UPower reports them (roadmap
//! M5.8b), through its command line `upower`: how full the battery is,
//! whether it charges, how long it will last, and whether the machine is
//! plugged in. Settings' Power page reads them here, and the panel's
//! battery and quick settings will (M5.9), so the reading is written once.
//!
//! One call, `upower -d`, dumps everything UPower knows, so a page asks the
//! daemon once and the text it shows under Details is the text it read.
//! The list is generic on purpose (ADR-011): a device is whatever UPower
//! lists, this machine's battery, a wireless mouse's or, later, a paired
//! phone's, and nothing here assumes the machine has a battery at all: a
//! desktop, a VM and a server have none and are simply plugged in. A
//! machine without `upower` (no `power` feature) says so. Std only.

use std::time::Duration;

use anyhow::{Result, bail};

use crate::i18n::{tr, trf};
use crate::tool;

/// The program every call goes through, found on the `PATH`.
pub const UPOWER: &str = "upower";

/// How long `upower` may take before it is given up on: it answers at
/// once when the daemon runs, and waits for the bus when it does not.
const TIME: Duration = Duration::from_secs(8);

/// The device UPower makes of all the batteries together, whose
/// percentage and time are the ones to show.
const DISPLAY_DEVICE: &str = "DisplayDevice";

/// What a battery is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Charging,
    Discharging,
    FullyCharged,
    Empty,
    /// Plugged in but not charging yet, as a charge limit keeps it
    PendingCharge,
    PendingDischarge,
    Unknown,
}

impl State {
    fn parse(text: &str) -> State {
        match text.trim() {
            "charging" => State::Charging,
            "discharging" => State::Discharging,
            "fully-charged" => State::FullyCharged,
            "empty" => State::Empty,
            "pending-charge" => State::PendingCharge,
            "pending-discharge" => State::PendingDischarge,
            _ => State::Unknown,
        }
    }
}

/// One device UPower lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// Its D-Bus path, such as `/org/freedesktop/UPower/devices/battery_BAT0`
    pub path: String,
    /// What it is: `battery`, `line-power`, `mouse`, `keyboard`, `ups`...
    pub kind: String,
    /// Its maker's and its model's name, when it gives them
    pub name: String,
    /// Whether it is a battery that is in the machine (a bay may be empty)
    pub present: bool,
    pub state: State,
    /// How full it is, in percent
    pub percent: Option<f64>,
    /// Minutes until it is empty (discharging) or full (charging)
    pub minutes: Option<u32>,
    /// For a power cable: whether it carries power
    pub online: Option<bool>,
}

impl Device {
    /// Whether this is the device that stands for all the batteries.
    pub fn is_display(&self) -> bool {
        self.path.rsplit('/').next() == Some(DISPLAY_DEVICE)
    }

    /// Whether it is a battery of this machine, as opposed to a mouse's.
    pub fn is_battery(&self) -> bool {
        self.kind == "battery"
    }

    /// The percentage as a whole number.
    pub fn percent_whole(&self) -> Option<u32> {
        self.percent.map(|p| p.round().clamp(0.0, 100.0) as u32)
    }
}

/// What `upower -d` listed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    /// Whether the machine runs on battery, as the daemon sees it; none
    /// when the dump does not say
    pub on_battery: Option<bool>,
    /// Whether the machine has a lid, and whether it is shut
    pub lid_present: bool,
    pub lid_closed: bool,
    /// The dump as UPower printed it, for the page's Details
    pub raw: String,
}

impl Snapshot {
    /// The battery to show: the one that stands for them all, else the
    /// first battery that is in the machine; none for a machine with no
    /// battery.
    pub fn battery(&self) -> Option<&Device> {
        let present = |d: &&Device| d.is_battery() && d.present;
        self.devices
            .iter()
            .filter(present)
            .find(|d| d.is_display())
            .or_else(|| self.devices.iter().find(present))
    }

    /// Batteries of other things UPower knows, such as a wireless mouse's
    /// or an uninterruptible power supply, with a charge to show.
    pub fn others(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| {
            !d.is_battery()
                && d.kind != "line-power"
                && d.present
                && d.percent.is_some()
                && !d.is_display()
        })
    }

    /// Whether the machine is plugged in: the daemon's word when it gave
    /// it, else whether a power cable carries power; a machine with no
    /// battery is plugged in, whatever else.
    pub fn plugged_in(&self) -> bool {
        match self.on_battery {
            Some(on) => !on,
            None => {
                self.battery().is_none()
                    || self.devices.iter().any(|d| d.online == Some(true))
                    || self
                        .battery()
                        .is_some_and(|b| matches!(b.state, State::Charging | State::FullyCharged))
            }
        }
    }
}

/// Reads the devices out of the text `upower -d` prints:
///
/// ```text
/// Device: /org/freedesktop/UPower/devices/battery_BAT0
///   native-path:          BAT0
///   battery
///     present:             yes
///     state:               discharging
///     time to empty:       5.4 hours
///     percentage:          82%
///
/// Daemon:
///   on-battery:      yes
/// ```
///
/// A line it does not know is skipped, so a later UPower that adds fields
/// still shows what it did before.
pub fn parse(dump: &str) -> Snapshot {
    let mut snapshot = Snapshot {
        raw: dump.trim_end().to_string(),
        ..Snapshot::default()
    };
    let mut in_daemon = false;
    let mut device: Option<Device> = None;
    let mut model: Option<String> = None;
    let mut vendor: Option<String> = None;
    let finish = |snapshot: &mut Snapshot,
                  device: &mut Option<Device>,
                  vendor: &mut Option<String>,
                  model: &mut Option<String>| {
        if let Some(mut d) = device.take() {
            d.name = match (vendor.take(), model.take()) {
                (Some(v), Some(m)) if !m.starts_with(&v) => format!("{v} {m}"),
                (_, Some(m)) => m,
                (Some(v), None) => v,
                (None, None) => String::new(),
            };
            snapshot.devices.push(d);
        }
        *vendor = None;
        *model = None;
    };
    for line in dump.lines() {
        if let Some(path) = line.strip_prefix("Device:") {
            finish(&mut snapshot, &mut device, &mut vendor, &mut model);
            in_daemon = false;
            device = Some(Device {
                path: path.trim().to_string(),
                kind: String::new(),
                name: String::new(),
                present: true,
                state: State::Unknown,
                percent: None,
                minutes: None,
                online: None,
            });
            continue;
        }
        if line.starts_with("Daemon:") {
            finish(&mut snapshot, &mut device, &mut vendor, &mut model);
            in_daemon = true;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if in_daemon {
            if let Some((key, value)) = text.split_once(':') {
                let yes = value.trim() == "yes";
                match key.trim() {
                    "on-battery" => snapshot.on_battery = Some(yes),
                    "lid-is-present" => snapshot.lid_present = yes,
                    "lid-is-closed" => snapshot.lid_closed = yes,
                    _ => {}
                }
            }
            continue;
        }
        let Some(device) = device.as_mut() else {
            continue;
        };
        // The device's kind is the line that names it, two columns in and
        // with no colon: `battery`, `line-power`, `mouse`.
        if indent == 2 && !text.contains(':') {
            if device.kind.is_empty() {
                device.kind = text.to_string();
            }
            continue;
        }
        let Some((key, value)) = text.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "vendor" if indent == 2 => vendor = Some(value.to_string()).filter(|v| !v.is_empty()),
            "model" if indent == 2 => model = Some(value.to_string()).filter(|v| !v.is_empty()),
            "present" => device.present = value == "yes",
            "online" => device.online = Some(value == "yes"),
            "state" => device.state = State::parse(value),
            "percentage" => {
                device.percent = value.trim_end_matches('%').trim().parse::<f64>().ok();
            }
            "time to empty" | "time to full" => device.minutes = minutes(value),
            _ => {}
        }
    }
    finish(&mut snapshot, &mut device, &mut vendor, &mut model);
    snapshot
}

/// A time as UPower prints it, `5.4 hours`, `41.2 minutes`, `30.0 seconds`
/// or `1.1 days`, in whole minutes.
fn minutes(text: &str) -> Option<u32> {
    let (number, unit) = text.trim().split_once(' ')?;
    let number: f64 = number.parse().ok()?;
    let per_unit = match unit.trim() {
        "seconds" | "second" => 1.0 / 60.0,
        "minutes" | "minute" => 1.0,
        "hours" | "hour" => 60.0,
        "days" | "day" => 1440.0,
        _ => return None,
    };
    let total = number * per_unit;
    (total.is_finite() && total >= 0.0).then(|| total.round() as u32)
}

/// A length of time in words people use: `less than a minute`,
/// `41 minutes`, `5 hours 25 minutes`, `2 hours`. From an hour on the
/// minutes are rounded to five, as an estimate is not exact to the minute.
pub fn duration_text(minutes: u32) -> String {
    if minutes == 0 {
        return tr("less than a minute").to_string();
    }
    if minutes < 60 {
        return minutes_text(minutes);
    }
    let rounded = (minutes + 2) / 5 * 5;
    let (hours, rest) = (rounded / 60, rounded % 60);
    if rest == 0 {
        hours_text(hours)
    } else {
        trf(
            "{hours} {minutes}",
            &[
                ("hours", &hours_text(hours)),
                ("minutes", &minutes_text(rest)),
            ],
        )
    }
}

/// What the panel's quick settings say under the battery's icon (M5.9a),
/// as the mockups' "82 percent, 4 h 10 min left", in [`duration_text`]'s
/// words: how full it is, then how long it lasts, or takes to fill, when
/// UPower knows; a full battery says so, and one whose charge is not
/// reported says only what it is doing.
pub fn battery_line(battery: &Device) -> String {
    let time = battery.minutes.map(duration_text);
    let Some(percent) = battery.percent_whole().map(|p| p.to_string()) else {
        return match battery.state {
            State::Charging => tr("Charging").to_string(),
            State::FullyCharged => tr("Fully charged").to_string(),
            _ => tr("Battery").to_string(),
        };
    };
    let at = [("percent", percent.as_str())];
    match (battery.state, time) {
        (State::FullyCharged, _) => tr("Fully charged").to_string(),
        (State::Charging, Some(t)) => trf(
            "{percent} percent, {time} until full",
            &[at[0], ("time", &t)],
        ),
        (State::Charging, None) => trf("{percent} percent, charging", &at),
        (State::Discharging, Some(t)) => {
            trf("{percent} percent, {time} left", &[at[0], ("time", &t)])
        }
        _ => trf("{percent} percent", &at),
    }
}

fn minutes_text(n: u32) -> String {
    if n == 1 {
        tr("1 minute").to_string()
    } else {
        trf("{n} minutes", &[("n", &n.to_string())])
    }
}

fn hours_text(n: u32) -> String {
    if n == 1 {
        tr("1 hour").to_string()
    } else {
        trf("{n} hours", &[("n", &n.to_string())])
    }
}

/// `upower` with `args` as a line to type, what Copy as command shows.
pub fn command_line(args: &[&str]) -> String {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tool::command_line(UPOWER, &args)
}

/// Asks UPower what it knows now.
pub fn read() -> Result<Snapshot> {
    let args = ["--dump".to_string()];
    let done = match tool::run(UPOWER, &args, TIME) {
        Ok(done) => done,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "{}",
            trf(
                "this machine has no {program}, so it cannot tell the battery; the power feature brings it",
                &[("program", UPOWER)]
            )
        ),
        Err(e) => bail!(
            "{}",
            trf(
                "could not run {program}: {error}",
                &[("program", UPOWER), ("error", &e.to_string())]
            )
        ),
    };
    if done.timed_out {
        bail!(
            "{}",
            trf(
                "{command} did not answer in {seconds} s; the power service (upowerd) starts with the machine, so restart the computer if it is not running",
                &[
                    ("command", &command_line(&["--dump"])),
                    ("seconds", &TIME.as_secs().to_string())
                ]
            )
        );
    }
    if !done.ok() {
        let said = done.err.trim();
        bail!(
            "{}",
            trf(
                "{command} failed: {said}; the power service (upowerd) starts with the machine, so restart the computer if it is not running",
                &[("command", &command_line(&["--dump"])), ("said", said)]
            )
        );
    }
    Ok(parse(&done.out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What UPower 1.91 prints (shortened) on a laptop that is charging,
    /// with a wireless mouse.
    const LAPTOP: &str = "\
Device: /org/freedesktop/UPower/devices/line_power_AC
  native-path:          AC
  power supply:         yes
  updated:              Thu 08 Oct 2026 03:45:26 PM +05 (12 seconds ago)
  has history:          no
  has statistics:       no
  line-power
    warning-level:       none
    online:              yes
    icon-name:          'ac-adapter-symbolic'

Device: /org/freedesktop/UPower/devices/battery_BAT0
  native-path:          BAT0
  vendor:               SMP
  model:                L20C2PF0
  power supply:         yes
  has history:          yes
  has statistics:       yes
  battery
    present:             yes
    rechargeable:        yes
    state:               charging
    warning-level:       none
    energy:              37.2 Wh
    time to full:        1.2 hours
    percentage:          82%
    capacity:            95.5%
    icon-name:          'battery-good-charging-symbolic'
  History (charge):
    1759932326	82.000	charging
    1759932266	81.000	charging

Device: /org/freedesktop/UPower/devices/mouse_hidpp_battery_0
  native-path:          hidpp_battery_0
  model:                MX Master 3
  power supply:         no
  mouse
    present:             yes
    state:               discharging
    percentage:          70%

Device: /org/freedesktop/UPower/devices/DisplayDevice
  power supply:         yes
  battery
    present:             yes
    state:               charging
    warning-level:       none
    time to full:        1.1 hours
    percentage:          82%
    icon-name:          'battery-good-charging-symbolic'

Daemon:
  daemon-version:  1.91.0
  on-battery:      no
  lid-is-closed:   no
  lid-is-present:  yes
  critical-action: HybridSleep
";

    /// A VM: nothing but the display device, which holds no battery.
    const VM: &str = "\
Device: /org/freedesktop/UPower/devices/DisplayDevice
  power supply:         yes
  updated:              Thu 08 Oct 2026 03:45:26 PM +05 (3 seconds ago)
  has history:          no
  has statistics:       no
  battery
    present:             no
    state:               unknown
    warning-level:       none
    energy:              0 Wh
    percentage:          0%
    icon-name:          'battery-missing-symbolic'

Daemon:
  daemon-version:  1.91.0
  on-battery:      no
  lid-is-closed:   no
  lid-is-present:  no
  critical-action: HybridSleep
";

    #[test]
    fn a_laptop_on_its_charger_is_charging_and_plugged_in() {
        let snapshot = parse(LAPTOP);
        let battery = snapshot.battery().unwrap();
        assert!(battery.is_display(), "the display device stands for all");
        assert_eq!(battery.state, State::Charging);
        assert_eq!(battery.percent_whole(), Some(82));
        assert_eq!(battery.minutes, Some(66), "1.1 hours");
        assert!(snapshot.plugged_in());
        assert!(snapshot.lid_present && !snapshot.lid_closed);
        assert_eq!(snapshot.devices.len(), 4);
        assert_eq!(snapshot.raw.lines().next(), LAPTOP.lines().next());
    }

    #[test]
    fn a_device_has_its_makers_name_and_a_mouse_is_another_battery() {
        let snapshot = parse(LAPTOP);
        let bat = &snapshot.devices[1];
        assert_eq!(
            (bat.kind.as_str(), bat.name.as_str()),
            ("battery", "SMP L20C2PF0")
        );
        assert_eq!(bat.minutes, Some(72), "its own 1.2 hours");
        assert_eq!(snapshot.devices[0].online, Some(true));
        let others: Vec<&Device> = snapshot.others().collect();
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].name, "MX Master 3");
        assert_eq!(others[0].percent_whole(), Some(70));
    }

    #[test]
    fn history_lines_are_not_mistaken_for_fields() {
        let snapshot = parse(LAPTOP);
        let bat = &snapshot.devices[1];
        assert_eq!(bat.state, State::Charging);
        assert_eq!(bat.percent, Some(82.0));
    }

    #[test]
    fn a_machine_with_no_battery_is_plugged_in() {
        let snapshot = parse(VM);
        assert!(
            snapshot.battery().is_none(),
            "the display device holds none"
        );
        assert!(snapshot.plugged_in());
        assert_eq!(snapshot.others().count(), 0);
        assert!(!snapshot.lid_present);
        // Nothing at all, as a daemon that listed nothing.
        let empty = parse("");
        assert!(empty.battery().is_none() && empty.plugged_in());
    }

    #[test]
    fn a_laptop_on_its_battery_is_not_plugged_in() {
        let on_battery = LAPTOP
            .replace("on-battery:      no", "on-battery:      yes")
            .replace("charging", "discharging")
            .replace("time to full", "time to empty");
        let snapshot = parse(&on_battery);
        assert!(!snapshot.plugged_in());
        let battery = snapshot.battery().unwrap();
        assert_eq!(battery.state, State::Discharging);
        assert_eq!(battery.minutes, Some(66));
        // Without the daemon's word, the cable says.
        let no_word = on_battery.replace("on-battery:      yes", "");
        let unplugged = no_word.replace("online:              yes", "online:              no");
        assert!(!parse(&unplugged).plugged_in());
    }

    #[test]
    fn the_first_battery_stands_in_without_a_display_device() {
        let old: String = LAPTOP
            .split("Device: /org/freedesktop/UPower/devices/DisplayDevice")
            .next()
            .unwrap()
            .to_string();
        let snapshot = parse(&old);
        let battery = snapshot.battery().unwrap();
        assert!(battery.path.ends_with("battery_BAT0"));
        assert_eq!(battery.percent_whole(), Some(82));
    }

    #[test]
    fn times_are_read_in_minutes() {
        assert_eq!(minutes("5.4 hours"), Some(324));
        assert_eq!(minutes("41.2 minutes"), Some(41));
        assert_eq!(minutes("30.0 seconds"), Some(1));
        assert_eq!(minutes("1.1 days"), Some(1584));
        assert_eq!(minutes("soon"), None);
        assert_eq!(minutes("3 fortnights"), None);
    }

    #[test]
    fn a_duration_reads_as_people_say_it() {
        assert_eq!(duration_text(0), "less than a minute");
        assert_eq!(duration_text(1), "1 minute");
        assert_eq!(duration_text(41), "41 minutes");
        assert_eq!(duration_text(60), "1 hour");
        assert_eq!(duration_text(66), "1 hour 5 minutes");
        assert_eq!(duration_text(324), "5 hours 25 minutes");
        assert_eq!(duration_text(120), "2 hours");
        assert_eq!(duration_text(121), "2 hours");
    }

    #[test]
    fn an_unknown_state_and_a_strange_line_are_skipped() {
        let snapshot = parse(
            "Device: /org/freedesktop/UPower/devices/battery_X\n  battery\n    present: yes\n    state: warming-up\n    something new: 1\n    percentage: 7%\nunrelated words\n",
        );
        let battery = snapshot.battery().unwrap();
        assert_eq!(battery.state, State::Unknown);
        assert_eq!(battery.percent_whole(), Some(7));
    }

    #[test]
    fn the_battery_line_says_how_full_and_how_long() {
        let battery = |state, percent, minutes| Device {
            path: "/org/freedesktop/UPower/devices/DisplayDevice".into(),
            kind: "battery".into(),
            name: String::new(),
            present: true,
            state,
            percent,
            minutes,
            online: None,
        };
        let line = |state, percent, minutes| battery_line(&battery(state, percent, minutes));
        assert_eq!(
            line(State::Discharging, Some(82.0), Some(250)),
            "82 percent, 4 hours 10 minutes left"
        );
        assert_eq!(
            line(State::Charging, Some(41.4), Some(41)),
            "41 percent, 41 minutes until full"
        );
        assert_eq!(
            line(State::Charging, Some(41.0), None),
            "41 percent, charging"
        );
        assert_eq!(
            line(State::FullyCharged, Some(100.0), None),
            "Fully charged"
        );
        assert_eq!(line(State::Discharging, Some(7.0), None), "7 percent");
        assert_eq!(line(State::Charging, None, None), "Charging");
        assert_eq!(line(State::Unknown, None, None), "Battery");
    }

    #[test]
    fn the_command_line_is_upower_and_its_words() {
        assert_eq!(command_line(&["--dump"]), "upower --dump");
        assert_eq!(command_line(&["-i", "/org/x y"]), "upower -i '/org/x y'");
    }
}
