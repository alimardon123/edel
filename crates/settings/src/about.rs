//! The About page (M5.6a): this release, read from `/usr/lib/os-release`
//! as `edel image build` writes it, and the last lines of the person's
//! session log (M5.28b), so a problem is found without a terminal.

use std::path::Path;

use edel::i18n::{tr, trf};
use edel::{places, session_log};

use crate::widgets;

const OS_RELEASE: &str = "/usr/lib/os-release";

pub fn page() -> gtk::Widget {
    let release = std::fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let (page, content, _) = widgets::page(
        tr("About"),
        tr("The system this machine runs and the release it follows."),
    );
    widgets::heading(&content, tr("This system"));
    let group = widgets::group(&content);
    for (title, key) in [
        (tr("System"), "PRETTY_NAME"),
        (tr("Version"), "VERSION_ID"),
        (tr("Channel"), "EDEL_CHANNEL"),
    ] {
        let value = field(&release, key).unwrap_or_else(|| tr("Unknown").into());
        widgets::value_row(&group, title, &value);
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
