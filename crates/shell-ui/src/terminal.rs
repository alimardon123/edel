//! foot follows the desktop's light or dark scheme (M6.13). At start,
//! shell-ui writes the person's foot theme file (`edel::places::FOOT_THEME`
//! in their state folder) with the scheme the settings give, and when that
//! changed, tells a running foot to repaint: SIGUSR1 for dark, SIGUSR2 for
//! light. foot's config includes the file (features/terminal), so a new
//! window starts in the same scheme. The compositor starts shell-ui again
//! when the scheme changes, so this runs again then.

use std::path::Path;
use std::process::{Command, Stdio};

use edel::places;

use crate::messages;

/// The theme file's text: the scheme foot starts a new window in.
pub fn theme_text(dark: bool) -> &'static str {
    if dark {
        "[main]\ninitial-color-theme=dark\n"
    } else {
        "[main]\ninitial-color-theme=light\n"
    }
}

/// Writes `dir`'s theme file for `dark`, through a `.new` file that is then
/// renamed over it, and only when its text differs from what is there.
/// Says whether it wrote.
pub fn write_theme(dir: &Path, dark: bool) -> std::io::Result<bool> {
    let path = dir.join(places::FOOT_THEME);
    let text = theme_text(dark);
    if std::fs::read_to_string(&path).is_ok_and(|have| have == text) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let new = dir.join(format!("{}.new", places::FOOT_THEME));
    std::fs::write(&new, text)?;
    std::fs::rename(&new, &path)?;
    Ok(true)
}

/// At start: the person's foot theme follows `dark`, and a running foot
/// repaints when it changed. No foot running is normal, so its signal's
/// failure is ignored. A person with no home has no theme file to write.
pub fn follow(dark: bool) {
    let Some(dir) = places::person_state_dir() else {
        return;
    };
    match write_theme(&dir, dark) {
        Ok(false) => {}
        Ok(true) => {
            let (name, signal) = if dark {
                ("dark", "-USR1")
            } else {
                ("light", "-USR2")
            };
            eprintln!("edel-shell-ui: terminal theme {name}");
            let _ = Command::new("pkill")
                .args([signal, "-x", "foot"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        Err(e) => eprintln!("edel-shell-ui: {}", messages::terminal_theme_not_written(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("edel-terminal-{name}-{}", std::process::id()))
    }

    #[test]
    fn the_theme_text_names_the_scheme_foot_starts_in() {
        assert_eq!(theme_text(true), "[main]\ninitial-color-theme=dark\n");
        assert_eq!(theme_text(false), "[main]\ninitial-color-theme=light\n");
    }

    #[test]
    fn the_theme_file_is_written_once_for_each_scheme() {
        let dir = folder("written");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join(places::FOOT_THEME);
        // A new file says it changed; the same scheme again does not.
        assert!(write_theme(&dir, true).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theme_text(true));
        assert!(!write_theme(&dir, true).unwrap());
        // Light is a change, and the file says light; no .new file is left.
        assert!(write_theme(&dir, false).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theme_text(false));
        assert!(!dir.join(format!("{}.new", places::FOOT_THEME)).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_theme_file_that_is_not_written_says_so() {
        // A file where the folder should be cannot hold the theme.
        let dir = folder("blocked");
        std::fs::write(&dir, "not a folder").unwrap();
        assert!(write_theme(&dir, true).is_err());
        std::fs::remove_file(dir).unwrap();
    }
}
