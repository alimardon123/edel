//! Sound devices as PipeWire reports them (roadmap M5.7b), through
//! WirePlumber's `wpctl`: the sinks that play and the sources that listen,
//! each with its volume, and the commands that change them. Settings'
//! Sound page reads them here, and the panel's quick settings and the
//! volume pop-up will (M5.9), so the reading is written once.
//!
//! The list is generic on purpose (ADR-011): a device is whatever PipeWire
//! lists, a laptop's speakers, a headset or, later, a paired phone's
//! microphone lent over the network, and nothing here assumes it is local.
//! Volume and the chosen device are the sound system's own live state,
//! which WirePlumber remembers for each person, so no key of the settings
//! file holds them. `wpctl` is the one command line to this; a machine
//! without it (no `sound` feature) has no devices, and says so.

use std::process::Command;

use anyhow::{Result, bail};

use crate::i18n::{tr, trf};

/// The program every call goes through, found on the `PATH`.
pub const WPCTL: &str = "wpctl";

/// The loudest the Sound page sets, in percent. PipeWire allows more, which
/// hurts ears and speakers; a device already louder shows what it is.
pub const MOST_PERCENT: u32 = 100;

/// Which way a device faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Plays sound: speakers, headphones
    Sink,
    /// Listens: a microphone
    Source,
}

impl Kind {
    /// The name `wpctl` gives the default device of this kind, such as in
    /// `wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%`.
    pub fn default_name(self) -> &'static str {
        match self {
            Kind::Sink => "@DEFAULT_AUDIO_SINK@",
            Kind::Source => "@DEFAULT_AUDIO_SOURCE@",
        }
    }
}

/// One sink or source.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// PipeWire's number for it, which `wpctl` takes
    pub id: u32,
    /// What people read: "Built-in Audio Analog Stereo"
    pub name: String,
    /// Whether it is the one in use (`wpctl status` marks it `*`)
    pub default: bool,
    /// Its volume in percent; none when `wpctl` shows none
    pub volume: Option<u32>,
    pub muted: bool,
}

/// What `wpctl status` lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Devices {
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
}

impl Devices {
    /// The devices of one kind.
    pub fn of(&self, kind: Kind) -> &[Device] {
        match kind {
            Kind::Sink => &self.sinks,
            Kind::Source => &self.sources,
        }
    }
}

/// Reads the sinks and sources out of the text `wpctl status` prints:
///
/// ```text
/// Audio
///  ├─ Sinks:
///  │  *   48. Built-in Audio Analog Stereo   [vol: 0.40]
///  ├─ Sources:
///  │  *   49. Built-in Audio Analog Stereo   [vol: 1.00 MUTED]
/// ```
///
/// A line that is not one of these is skipped, so a later WirePlumber that
/// adds sections or columns still shows its devices.
pub fn parse(status: &str) -> Devices {
    let mut devices = Devices::default();
    // Only the Audio part lists sound; Video has sinks of its own.
    let mut audio = false;
    let mut section: Option<Kind> = None;
    for line in status.lines() {
        // The tree's drawing and the indent come first. A line that starts
        // with a letter is a part's heading: Audio, Video, Settings.
        let plain = line.trim_start_matches([' ', '\t', '│', '├', '└', '─']);
        if plain.is_empty() {
            continue;
        }
        if line.starts_with(char::is_alphanumeric) {
            audio = plain == "Audio";
            section = None;
            continue;
        }
        let numbered = plain.starts_with(|c: char| c.is_ascii_digit() || c == '*');
        if plain.ends_with(':') && !numbered {
            section = match plain {
                "Sinks:" if audio => Some(Kind::Sink),
                "Sources:" if audio => Some(Kind::Source),
                _ => None,
            };
            continue;
        }
        let (Some(kind), Some(device)) = (section, device(plain)) else {
            continue;
        };
        match kind {
            Kind::Sink => devices.sinks.push(device),
            Kind::Source => devices.sources.push(device),
        }
    }
    devices
}

/// One device line without its tree: `*   48. Name   [vol: 0.40 MUTED]`.
fn device(line: &str) -> Option<Device> {
    let (default, rest) = match line.strip_prefix('*') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, line),
    };
    let (id, rest) = rest.split_once('.')?;
    let id: u32 = id.trim().parse().ok()?;
    let rest = rest.trim();
    // The last bracket holds the volume; a name may have brackets of its own.
    let (name, extra) = match rest.rfind('[') {
        Some(at) if rest.ends_with(']') && rest[at..].starts_with("[vol:") => {
            (rest[..at].trim(), Some(&rest[at + 1..rest.len() - 1]))
        }
        _ => (rest, None),
    };
    if name.is_empty() {
        return None;
    }
    let volume = extra
        .and_then(|e| e.strip_prefix("vol:"))
        .and_then(|e| e.split_whitespace().next())
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v * 100.0).round() as u32);
    Some(Device {
        id,
        name: name.to_string(),
        default,
        volume,
        muted: extra.is_some_and(|e| e.split_whitespace().any(|w| w == "MUTED")),
    })
}

/// The arguments of `wpctl` that set `device` (a number, or a name such as
/// [`Kind::default_name`]) to `percent`, kept at most [`MOST_PERCENT`].
pub fn volume_args(device: &str, percent: u32) -> Vec<String> {
    vec![
        "set-volume".into(),
        device.into(),
        format!("{}%", percent.min(MOST_PERCENT)),
    ]
}

/// The arguments that mute or unmute `device`.
pub fn mute_args(device: &str, muted: bool) -> Vec<String> {
    vec![
        "set-mute".into(),
        device.into(),
        if muted { "1" } else { "0" }.into(),
    ]
}

/// The arguments that make `id` the device in use; WirePlumber remembers
/// the choice for this person.
pub fn default_args(id: u32) -> Vec<String> {
    vec!["set-default".into(), id.to_string()]
}

/// `wpctl` with `args` as a line to type, what Copy as command shows.
pub fn command_line(args: &[String]) -> String {
    format!("{WPCTL} {}", args.join(" "))
}

/// Runs `wpctl` with `args`, giving what it printed.
fn wpctl(args: &[String]) -> Result<String> {
    let out = match Command::new(WPCTL).args(args).output() {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "{}",
            trf(
                "this machine has no {program}, so it has no sound system; the sound feature brings PipeWire",
                &[("program", WPCTL)]
            )
        ),
        Err(e) => bail!(
            "{}",
            trf(
                "could not run {program}: {error}",
                &[("program", WPCTL), ("error", &e.to_string())]
            )
        ),
    };
    if !out.status.success() {
        // `wpctl` says why on standard error; "Failed to connect to
        // PipeWire" is the usual one, when the session's sound system is not
        // running.
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.trim();
        bail!(
            "{}",
            trf(
                "{command} failed: {said}; the sound system runs in your session, so log in again if it is not running",
                &[("command", &command_line(args)), ("said", said)]
            )
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The devices PipeWire lists now.
pub fn read() -> Result<Devices> {
    Ok(parse(&wpctl(&["status".to_string()])?))
}

/// Sets `device` to `percent`.
pub fn set_volume(device: &str, percent: u32) -> Result<()> {
    wpctl(&volume_args(device, percent)).map(drop)
}

/// Mutes or unmutes `device`.
pub fn set_mute(device: &str, muted: bool) -> Result<()> {
    wpctl(&mute_args(device, muted)).map(drop)
}

/// Makes device `id` the one in use.
pub fn set_default(id: u32) -> Result<()> {
    wpctl(&default_args(id)).map(drop)
}

/// What the Sound page says when there is nothing to list.
pub fn no_devices() -> &'static str {
    tr(
        "No sound device found. Plug in speakers or a headset, or check that the machine has a sound card.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What WirePlumber 0.5 prints (shortened), with the default sink at
    /// 40 percent and a muted default source.
    const STATUS: &str = "\
PipeWire 'pipewire-0' [1.4.2, ci@edel, cookie:1234]
 └─ Clients:
        33. WirePlumber                         [1.4.2, ci@edel, pid:301]
        45. wpctl                               [1.4.2, ci@edel, pid:322]

Audio
 ├─ Devices:
 │      47. Built-in Audio                      [alsa]
 │
 ├─ Sinks:
 │  *   48. Built-in Audio Analog Stereo        [vol: 0.40]
 │      52. HDMI / DisplayPort 1 Output [Digital] [vol: 1.00]
 │
 ├─ Sink endpoints:
 │
 ├─ Sources:
 │  *   49. Built-in Audio Analog Stereo        [vol: 1.00 MUTED]
 │
 ├─ Source endpoints:
 │
 └─ Streams:
        60. Firefox
             61. output_FL > Built-in Audio:playback_FL   [active]

Video
 ├─ Devices:
 │      50. Camera                              [v4l2]
 │
 ├─ Sinks:
 │      99. Not a sound sink                    [vol: 0.10]

Settings
 └─ Default Configured Devices:
         0. Audio/Sink    alsa_output.pci-0000_00_1b.0.analog-stereo
";

    #[test]
    fn the_sinks_and_sources_of_wpctl_status_are_listed() {
        let devices = parse(STATUS);
        assert_eq!(devices.sinks.len(), 2);
        assert_eq!(devices.sources.len(), 1);
        assert_eq!(
            devices.sinks[0],
            Device {
                id: 48,
                name: "Built-in Audio Analog Stereo".into(),
                default: true,
                volume: Some(40),
                muted: false,
            }
        );
        // A name with brackets of its own keeps them.
        assert_eq!(
            devices.sinks[1].name,
            "HDMI / DisplayPort 1 Output [Digital]"
        );
        assert!(!devices.sinks[1].default);
        assert_eq!(devices.sinks[1].volume, Some(100));
        assert!(devices.sources[0].default && devices.sources[0].muted);
        assert_eq!(devices.sources[0].volume, Some(100));
    }

    #[test]
    fn a_heading_that_is_not_audio_ends_the_section() {
        // The video section's "Sinks:" is not a sound sink.
        assert!(parse(STATUS).sinks.iter().all(|d| d.id != 99));
    }

    #[test]
    fn a_device_without_a_volume_is_listed_without_one() {
        let devices = parse("Audio\n ├─ Sinks:\n │      7. A network speaker\n");
        assert_eq!(devices.sinks.len(), 1);
        assert_eq!(devices.sinks[0].volume, None);
        assert_eq!(devices.sinks[0].name, "A network speaker");
    }

    #[test]
    fn nothing_listed_is_no_devices() {
        assert_eq!(parse(""), Devices::default());
        assert_eq!(
            parse("Audio\n ├─ Sinks:\n │\n ├─ Sources:\n │\n"),
            Devices::default()
        );
        assert_eq!(parse("not wpctl at all\n  1. x"), Devices::default());
    }

    #[test]
    fn the_commands_are_wpctls() {
        assert_eq!(volume_args("48", 30), ["set-volume", "48", "30%"]);
        assert_eq!(
            command_line(&volume_args(Kind::Sink.default_name(), 30)),
            "wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%"
        );
        // Never louder than the page allows.
        assert_eq!(volume_args("48", 400)[2], "100%");
        assert_eq!(mute_args("48", true), ["set-mute", "48", "1"]);
        assert_eq!(mute_args("48", false), ["set-mute", "48", "0"]);
        assert_eq!(default_args(52), ["set-default", "52"]);
    }

    #[test]
    fn devices_of_a_kind_are_found_by_kind() {
        let devices = parse(STATUS);
        assert_eq!(devices.of(Kind::Sink).len(), 2);
        assert_eq!(devices.of(Kind::Source).len(), 1);
    }
}
