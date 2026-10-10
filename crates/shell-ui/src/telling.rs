//! What shell-ui tells a person by itself (M5.9e), as notifications in its
//! own list: that the last update did not start and the machine went back
//! (once for each person, for each record), and that a newer release is
//! out (once for each version). Apps never send these; the ids are ones
//! the server never hands out, so a press on them stays inside shell-ui
//! and no app is told (`notify_card.rs`).
//!
//! This file is plain data and tested without a display: the two
//! notifications, the texts of the record and of the update check's
//! output (`edel update --check`, read leniently), the two stamps that
//! remember what a person has seen, and the timing of the check.

use std::fs;
use std::io;
use std::path::Path;

use edel::i18n::{tr, trf};
use edel::places;

use crate::notify::{Notification, Urgency};

/// The id of the notice that the last update went back, and of the notice
/// that a newer release is out: ids the server never hands out.
pub const FALLBACK_ID: u32 = u32::MAX;
pub const UPDATE_ID: u32 = u32::MAX - 1;

/// What the notices say they come from.
pub const APP: &str = "Edel OS";

/// The first update check comes this long after shell-ui starts, then one
/// every `CHECK_EVERY_SECS`.
pub const FIRST_CHECK_SECS: u64 = 600;
pub const CHECK_EVERY_SECS: u64 = 6 * 3600;

/// The stamps in the person's state folder: the record's date last shown,
/// and the version last announced.
pub const FALLBACK_SEEN: &str = "fallback-seen";
pub const UPDATE_SEEN: &str = "update-seen";

/// Whether `id` is one of the two notices shell-ui makes itself.
pub fn is_ours(id: u32) -> bool {
    id == FALLBACK_ID || id == UPDATE_ID
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

/// [`write_stamp`] in `dir`, which is made when missing: the value is
/// written to `NAME.new` and renamed over the stamp, so a stamp is never
/// half written.
pub fn write_stamp_in(dir: &Path, name: &str, value: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let new = dir.join(format!("{name}.new"));
    fs::write(&new, format!("{value}\n"))?;
    fs::rename(&new, dir.join(name))
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
