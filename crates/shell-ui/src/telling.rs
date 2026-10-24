//! What shell-ui tells a person by itself (M5.9e), as notifications in its
//! own list: that the last update did not start and the machine went back
//! (once for each person, for each record), that a newer release is out
//! (once for each version), and that an add-on serves this computer's
//! hardware (once for each set of packs, M7.11). Apps never send these; the
//! ids are ones the server never hands out, so a press on them stays inside
//! shell-ui and no app is told (`notify_card.rs`).
//!
//! This file is plain data and tested without a display: the two
//! notifications, the texts of the record and of the update check's
//! output (`edel update --check`, read leniently), the two stamps that
//! remember what a person has seen, and the timing of the check.

use std::fs;
use std::io;
use std::path::Path;

use edel::hardware::Pack;
use edel::i18n::{tr, trf};
use edel::places;

use crate::notify::{Notification, Urgency};

/// The id of the notice that the last update went back, of the notice
/// that a newer release is out, and of the notice that a hardware pack
/// serves this computer: ids the server never hands out.
pub const FALLBACK_ID: u32 = u32::MAX;
pub const UPDATE_ID: u32 = u32::MAX - 1;
pub const HARDWARE_ID: u32 = u32::MAX - 2;

/// What the notices say they come from.
pub const APP: &str = "Edel OS";

/// The first update check comes this long after shell-ui starts, then one
/// every `CHECK_EVERY_SECS`.
pub const FIRST_CHECK_SECS: u64 = 600;
pub const CHECK_EVERY_SECS: u64 = 6 * 3600;

/// The stamps in the person's state folder: the record's date last shown,
/// the version last announced, and the hardware packs last named.
pub const FALLBACK_SEEN: &str = "fallback-seen";
pub const UPDATE_SEEN: &str = "update-seen";
pub const HARDWARE_SEEN: &str = "hardware-seen";

/// Whether `id` is one of the notices shell-ui makes itself.
pub fn is_ours(id: u32) -> bool {
    id == FALLBACK_ID || id == UPDATE_ID || id == HARDWARE_ID
}

/// The record's `date` value, as the stamp holds it; empty when the
/// record has none.
pub fn seen_key(record: &str) -> String {
    date(record).unwrap_or_default()
}

fn date(record: &str) -> Option<String> {
    let table: toml::Table = record.parse().ok()?;
    match table.get("date")? {
        toml::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// The notice that the last update did not start, from the record
/// `edel::places::LAST_FALLBACK` holds; none when the record does not
/// parse or has no `date`.
pub fn fallback(record: &str) -> Option<Notification> {
    date(record)?;
    if let Some(names) = addons(record) {
        return Some(Notification {
            id: FALLBACK_ID,
            app: APP.to_string(),
            icon: "status-problem".to_string(),
            summary: trf("Add-on {names} did not start", &[("names", &names)]),
            body: tr("Edel OS started without it, so everything works as it did. It is not tried again until you turn it off and on in Add-ons.").to_string(),
            actions: vec![
                ("default".to_string(), String::new()),
                ("addons".to_string(), tr("Open Add-ons").to_string()),
                ("later".to_string(), tr("Later").to_string()),
            ],
            timeout: -1,
            urgency: Urgency::Normal,
        });
    }
    Some(Notification {
        id: FALLBACK_ID,
        app: APP.to_string(),
        icon: "status-problem".to_string(),
        summary: tr("The last update did not start").to_string(),
        body: tr("Edel OS went back to the version before it, so everything works as it did and nothing was lost.").to_string(),
        actions: open_updates(),
        timeout: -1,
        urgency: Urgency::Normal,
    })
}

/// The add-ons a record names as not started (M7.2b), joined by commas;
/// none for a record of a slot's fallback.
fn addons(record: &str) -> Option<String> {
    let table: toml::Table = record.parse().ok()?;
    let names: Vec<&str> = table
        .get("addons")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// The notice that a newer release is out, from `edel update --check`'s
/// output: the version (the text after `available: ` up to its first
/// space) and the notice. None unless a line says `newer: yes`.
pub fn update(lines: &str) -> Option<(String, Notification)> {
    if !lines.lines().any(|line| line.trim() == "newer: yes") {
        return None;
    }
    let version = lines
        .lines()
        .find_map(|line| line.strip_prefix("available: "))?
        .split(' ')
        .next()?
        .to_string();
    if version.is_empty() {
        return None;
    }
    let n = Notification {
        id: UPDATE_ID,
        app: APP.to_string(),
        icon: "status-new".to_string(),
        summary: trf("Edel OS {version} is ready", &[("version", &version)]),
        body: tr("Open Updates to install it; your apps and files stay as they are.").to_string(),
        actions: open_updates(),
        timeout: -1,
        urgency: Urgency::Normal,
    };
    Some((version, n))
}

/// The notice that an add-on serves this computer's hardware and is not
/// chosen (M7.11), from the packs file; none when there are none. The key
/// is the packs' names, so the same packs are not named twice (the stamp
/// keeps it). Its buttons open Add-ons, as an add-on's fallback notice does.
pub fn hardware(packs: &[Pack]) -> Option<(String, Notification)> {
    if packs.is_empty() {
        return None;
    }
    let names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
    let (summary, body) = match packs {
        [one] => (
            tr("An add-on serves this computer's hardware").to_string(),
            trf(
                "{summary}. Add it in Settings, Add-ons; it starts at the next restart.",
                &[("summary", &one.summary)],
            ),
        ),
        _ => (
            trf(
                "{count} add-ons serve this computer's hardware",
                &[("count", &packs.len().to_string())],
            ),
            trf(
                "{names}. Add them in Settings, Add-ons; they start at the next restart.",
                &[("names", &names.join(", "))],
            ),
        ),
    };
    let n = Notification {
        id: HARDWARE_ID,
        app: APP.to_string(),
        icon: "status-new".to_string(),
        summary,
        body,
        actions: vec![
            ("default".to_string(), String::new()),
            ("addons".to_string(), tr("Open Add-ons").to_string()),
            ("later".to_string(), tr("Later").to_string()),
        ],
        timeout: -1,
        urgency: Urgency::Normal,
    };
    Some((names.join(","), n))
}

/// What a press on either notice does: a press on its body (the
/// `default` action, which shows no button) and on Open Updates both
/// open Settings' Updates page, and Later only goes.
fn open_updates() -> Vec<(String, String)> {
    vec![
        ("default".to_string(), String::new()),
        ("updates".to_string(), tr("Open Updates").to_string()),
        ("later".to_string(), tr("Later").to_string()),
    ]
}

/// The stamp `name` in the person's state folder, trimmed; none when
/// there is no folder or no stamp.
pub fn read_stamp(name: &str) -> Option<String> {
    read_stamp_in(&places::person_state_dir()?, name)
}

/// Writes the stamp `name` in the person's state folder; nothing when
/// there is no such folder.
pub fn write_stamp(name: &str, value: &str) -> io::Result<()> {
    match places::person_state_dir() {
        Some(dir) => write_stamp_in(&dir, name, value),
        None => Ok(()),
    }
}

/// [`read_stamp`] in `dir`.
pub fn read_stamp_in(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// [`write_stamp`] in `dir`, which is made when missing, through
/// `files::replace`, so a stamp is never half written.
pub fn write_stamp_in(dir: &Path, name: &str, value: &str) -> io::Result<()> {
    crate::files::replace(&dir.join(name), format!("{value}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = "format = 1\nfrom = \"B\"\nto = \"A\"\ndate = 2026-10-10T00:00:00Z\n";

    #[test]
    fn a_record_gives_the_fallback_notice_and_its_date_is_the_seen_key() {
        let n = fallback(RECORD).expect("a record with a date gives a notice");
        assert_eq!(n.id, FALLBACK_ID);
        assert!(is_ours(n.id));
        assert_eq!(n.app, APP);
        assert_eq!(n.summary, "The last update did not start");
        assert_eq!(seen_key(RECORD), "2026-10-10T00:00:00Z");
        assert!(n.has_default());
        let buttons: Vec<&str> = n.buttons().map(|(key, _)| key.as_str()).collect();
        assert_eq!(buttons, ["updates", "later"]);
    }

    #[test]
    fn an_add_on_that_did_not_start_gives_its_own_notice() {
        let record = "format = 1\naddons = [\"ci-hang\"]\ndate = 2026-10-17T10:00:00Z\n";
        let n = fallback(record).expect("a record with a date gives a notice");
        assert_eq!(n.summary, "Add-on ci-hang did not start");
        let buttons: Vec<&str> = n.buttons().map(|(key, _)| key.as_str()).collect();
        assert_eq!(buttons, ["addons", "later"]);
    }

    #[test]
    fn a_broken_record_or_one_without_a_date_gives_nothing() {
        assert!(fallback("format = 1\ndate = [").is_none());
        assert!(fallback("format = 1\nfrom = \"B\"\n").is_none());
        assert!(fallback("").is_none());
    }

    #[test]
    fn an_update_names_its_version_when_a_newer_one_is_out() {
        let lines = "running: 2026.10.1\navailable: 2026.10.5 (newer)\nnewer: yes\n";
        let (version, n) = update(lines).expect("a newer release gives a notice");
        assert_eq!(version, "2026.10.5");
        assert_eq!(n.id, UPDATE_ID);
        assert_eq!(n.summary, "Edel OS 2026.10.5 is ready");
        assert_eq!(n.icon, "status-new");
    }

    #[test]
    fn no_notice_when_the_release_is_not_newer() {
        let lines = "running: 2026.10.5\navailable: 2026.10.5 (not newer)\nnewer: no\n";
        assert!(update(lines).is_none());
        assert!(update("").is_none());
        assert!(update("running: 1\n").is_none());
    }

    fn pack(name: &str, summary: &str) -> Pack {
        Pack {
            name: name.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn one_hardware_pack_gives_a_notice_with_the_add_ons_button() {
        let packs = [pack("graphics-nvidia", "NVIDIA graphics")];
        let (key, n) = hardware(&packs).expect("a pack gives a notice");
        assert_eq!(key, "graphics-nvidia");
        assert_eq!(n.id, HARDWARE_ID);
        assert!(is_ours(n.id));
        assert_eq!(n.summary, "An add-on serves this computer's hardware");
        assert_eq!(
            n.body,
            "NVIDIA graphics. Add it in Settings, Add-ons; it starts at the next restart."
        );
        assert!(n.has_default());
        let buttons: Vec<&str> = n.buttons().map(|(key, _)| key.as_str()).collect();
        assert_eq!(buttons, ["addons", "later"]);
    }

    #[test]
    fn more_hardware_packs_are_named_together_and_keyed_by_their_names() {
        let packs = [pack("graphics-nvidia", "NVIDIA"), pack("ci-hang", "Hangs")];
        let (key, n) = hardware(&packs).expect("packs give a notice");
        assert_eq!(key, "graphics-nvidia,ci-hang");
        assert_eq!(n.summary, "2 add-ons serve this computer's hardware");
        assert_eq!(
            n.body,
            "graphics-nvidia, ci-hang. Add them in Settings, Add-ons; they start at the next restart."
        );
    }

    #[test]
    fn no_pack_gives_no_notice() {
        assert!(hardware(&[]).is_none());
    }

    #[test]
    fn stamps_round_trip_in_a_folder() {
        let dir = std::env::temp_dir().join(format!("edel-telling-{}", std::process::id()));
        assert_eq!(read_stamp_in(&dir, FALLBACK_SEEN), None);
        write_stamp_in(&dir, FALLBACK_SEEN, "2026-10-10T00:00:00Z").expect("write");
        assert_eq!(
            read_stamp_in(&dir, FALLBACK_SEEN).as_deref(),
            Some("2026-10-10T00:00:00Z")
        );
        write_stamp_in(&dir, FALLBACK_SEEN, "2026-10-11T00:00:00Z").expect("write again");
        assert_eq!(
            read_stamp_in(&dir, FALLBACK_SEEN).as_deref(),
            Some("2026-10-11T00:00:00Z")
        );
        assert!(!dir.join("fallback-seen.new").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
