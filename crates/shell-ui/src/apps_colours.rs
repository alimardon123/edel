//! Apps' colours follow ours (M6.18): shell-ui writes each person's GTK 4
//! stylesheet, `~/.config/gtk-4.0/gtk.css`, from the design tokens and their
//! settings files, so GTK 4 and libadwaita apps draw in the desktop's
//! colours. A change reaches apps already open through the settings portal
//! (`portal.rs`) and the stylesheet's media queries, which GTK 4.22 matches
//! live. The file is written only when its text differs, through a `.new`
//! file renamed over it, as `terminal.rs` writes foot's theme. A file the
//! person wrote themselves is left alone, and so is one that is not UTF-8.
//! `follow` runs at start (`main.rs`) and at each change of the look
//! (`follow.rs`).

use std::path::{Path, PathBuf};

use edel::places;
use edel::tokens::GTK_CSS_MARK;

use crate::messages;

/// The file every person got before M6.18, which imported the shipped
/// stylesheet. Left as it was, it is ours too, so it is rewritten; with
/// rules the person added below it, as it invited, it is theirs.
fn former() -> String {
    format!(
        "/* GTK4 and libadwaita apps read this file. It takes Edel OS's colours\n   \
         from the design tokens (M5.5b); add your own rules below it. */\n\
         @import url(\"file://{}/gtk.css\");\n",
        places::SHARE_DIR
    )
}

/// What writing the stylesheet did.
#[derive(Debug, PartialEq, Eq)]
pub enum Written {
    /// The file already holds this text.
    Same,
    /// The file was written, or made because it was absent.
    Wrote,
    /// The person's own file, which is left as it is.
    Theirs,
}

/// Writes `text` to `path` through `PATH.new` renamed over it, only when the
/// file is absent or ours (its text starts with [`GTK_CSS_MARK`] or
/// is [`former`]) and differs; a file of the person's own is left alone.
/// Creates the folder.
pub fn write_css(path: &Path, text: &str) -> std::io::Result<Written> {
    match std::fs::read_to_string(path) {
        Ok(have) if have == text => return Ok(Written::Same),
        Ok(have) if !(have.starts_with(GTK_CSS_MARK) || have == former()) => {
            return Ok(Written::Theirs);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Not UTF-8, so not a stylesheet of ours.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Ok(Written::Theirs),
        Err(e) => return Err(e),
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut new = path.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    std::fs::write(&new, text)?;
    std::fs::rename(&new, path)?;
    Ok(Written::Wrote)
}

/// Writes this person's stylesheet from the settings files now, logging
/// `edel-shell-ui: apps' colours written` when it wrote and
/// `edel-shell-ui: apps' colours left to your own gtk.css` when the file is
/// theirs; a failure logs `messages::apps_colours_not_written`. A person
/// with no home has no file. Nothing is written where no shipped share
/// folder exists (the desktop on another distribution, M5.15): there the
/// file would restyle every GTK app of that account. The greeter never gets
/// here, as `run` hands it over before the session's start.
pub fn follow() {
    if !Path::new(places::SHARE_DIR).is_dir() {
        return;
    }
    let Some(path) = places::person_gtk_css() else {
        return;
    };
    let text = edel::tokens::apps_css_on_this_machine();
    match write_css(&path, &text) {
        Ok(Written::Wrote) => eprintln!("edel-shell-ui: apps' colours written"),
        Ok(Written::Theirs) => {
            eprintln!("edel-shell-ui: apps' colours left to your own gtk.css");
        }
        Ok(Written::Same) => {}
        Err(e) => eprintln!("edel-shell-ui: {}", messages::apps_colours_not_written(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("edel-apps-colours-{name}-{}", std::process::id()))
    }

    #[test]
    fn an_absent_file_is_written_and_a_second_write_says_same() {
        let dir = folder("absent");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("gtk-4.0").join("gtk.css");
        let text = format!("{GTK_CSS_MARK}, from the design tokens (M6.18). */\n");
        assert_eq!(write_css(&file, &text).unwrap(), Written::Wrote);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        assert_eq!(write_css(&file, &text).unwrap(), Written::Same);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_persons_own_file_is_left_alone() {
        let dir = folder("theirs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gtk.css");
        std::fs::write(&file, "window { }\n").unwrap();
        let text = format!("{GTK_CSS_MARK}, from the design tokens (M6.18). */\n");
        assert_eq!(write_css(&file, &text).unwrap(), Written::Theirs);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "window { }\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn our_file_with_other_text_is_rewritten_and_leaves_no_new_file() {
        let dir = folder("ours");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gtk.css");
        std::fs::write(&file, format!("{GTK_CSS_MARK}, an older look */\n")).unwrap();
        let text = format!("{GTK_CSS_MARK}, from the design tokens (M6.18). */\n");
        assert_eq!(write_css(&file, &text).unwrap(), Written::Wrote);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        assert!(!dir.join("gtk.css.new").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_file_every_person_got_before_is_rewritten() {
        let dir = folder("former");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gtk.css");
        std::fs::write(&file, former()).unwrap();
        let text = format!("{GTK_CSS_MARK}, from the design tokens (M6.18). */\n");
        assert_eq!(write_css(&file, &text).unwrap(), Written::Wrote);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        // With the person's own rules below it, it is theirs.
        let added = format!("{}window {{ }}\n", former());
        std::fs::write(&file, &added).unwrap();
        assert_eq!(write_css(&file, &text).unwrap(), Written::Theirs);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
