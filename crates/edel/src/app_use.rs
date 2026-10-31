//! The record of use (M6.14b, M6.14c): which app used the camera, the
//! microphone, the screen or the place of this computer, when and for how
//! long, kept on this machine only, as there is
//! no telemetry. shell-ui writes a line when a use ends, and Settings' app
//! pages read them; the file is `places::APP_USE` in the person's state
//! directory, plain text, newest last, one use a line:
//!
//! ```text
//! START SECONDS KIND APP
//! 1760515862 125 camera org.gnome.Snapshot
//! ```
//!
//! START is when it began, in seconds since 1970 (UTC), SECONDS how long it
//! lasted, KIND `camera`, `microphone`, `screen` or `location`, and APP the
//! Flatpak app
//! id, or for an app outside Flatpak its program's name. A line a later
//! release writes and this one does not understand is skipped. The record
//! keeps the last `KEEP_DAYS` days and at most `KEEP_LINES` lines
//! ([`trim`]), so it never grows past a few tens of kilobytes. Pure
//! functions on text, and [`append`] to write a line.

use std::io::Write;
use std::path::Path;

use crate::app_permissions;
use crate::i18n::n_;

/// How many days of use the record keeps.
pub const KEEP_DAYS: u64 = 30;
/// How many lines the record keeps at most, the newest.
pub const KEEP_LINES: usize = 1000;

/// What an app used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Camera,
    Microphone,
    Screen,
    /// An app that asked where this computer is (M6.14c).
    Location,
}

impl Kind {
    /// Every kind, in the order the panel's sign and Settings show them.
    pub const ALL: [Kind; 4] = [Kind::Camera, Kind::Microphone, Kind::Screen, Kind::Location];

    /// Its name in the record.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Camera => "camera",
            Kind::Microphone => "microphone",
            Kind::Screen => "screen",
            Kind::Location => "location",
        }
    }

    /// The kind a record's name means.
    pub fn named(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// What people call it, marked with `n_`: the camera's and the
    /// microphone's are their permissions' titles.
    pub fn title(self) -> &'static str {
        match self.permission() {
            Some(permission) => permission.title,
            None => n_("Screen"),
        }
    }

    /// Its icon, one of the shell's own: the camera's and the microphone's
    /// are their permissions' icons, the screen's the screenshots'.
    pub fn icon(self) -> &'static str {
        let key = match self {
            Kind::Camera => "apps.camera",
            Kind::Microphone => "apps.microphone",
            Kind::Screen => "apps.screenshots",
            Kind::Location => "apps.location",
        };
        app_permissions::find(key).map_or("app-generic", |p| p.icon)
    }

    /// The permission that decides whether an app may use it, if one does:
    /// sharing the screen is asked by its portal every time, so none.
    pub fn permission(self) -> Option<&'static app_permissions::Permission> {
        match self {
            Kind::Camera => app_permissions::find("apps.camera"),
            Kind::Microphone => app_permissions::find("apps.microphone"),
            Kind::Screen => None,
            Kind::Location => app_permissions::find("apps.location"),
        }
    }
}

/// One use: an app used `kind` from `start` for `seconds`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    /// Seconds since 1970, UTC.
    pub start: u64,
    pub seconds: u64,
    pub kind: Kind,
    /// The Flatpak app id, else the program's name.
    pub app: String,
}

/// The record's line for `used`, with its newline. An app with white space
/// in it is written with each run of it as one `_`, so the line reads back.
pub fn line(used: &Use) -> String {
    let app: Vec<&str> = used.app.split_whitespace().collect();
    format!(
        "{} {} {} {}\n",
        used.start,
        used.seconds,
        used.kind.name(),
        app.join("_")
    )
}

/// The uses a record's text holds, oldest first; a line this release does
/// not understand is skipped.
pub fn parse(text: &str) -> Vec<Use> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Use> {
    let mut words = line.split_whitespace();
    let start = words.next()?.parse().ok()?;
    let seconds = words.next()?.parse().ok()?;
    let kind = Kind::named(words.next()?)?;
    let app = words.next()?.to_string();
    // A fifth word is a later release's: the line is still this use.
    Some(Use {
        start,
        seconds,
        kind,
        app,
    })
}

/// The record's text kept to the last [`KEEP_DAYS`] days before `now` and
/// at most [`KEEP_LINES`] lines, the newest; lines it does not understand
/// are kept while they fit, as a later release may read them.
pub fn trim(text: &str, now: u64) -> String {
    let oldest = now.saturating_sub(KEEP_DAYS * 24 * 60 * 60);
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| parse_line(line).is_none_or(|used| used.start >= oldest))
        .collect();
    let from = kept.len().saturating_sub(KEEP_LINES);
    kept[from..]
        .iter()
        .map(|line| format!("{line}\n"))
        .collect()
}

/// The uses of the app known by any of `ids` (its Flatpak id, its desktop
/// file's id or its program's name), newest first.
pub fn of_app<'a>(uses: &'a [Use], ids: &[&str]) -> Vec<&'a Use> {
    let mut found: Vec<&Use> = uses
        .iter()
        .filter(|used| ids.contains(&used.app.as_str()))
        .collect();
    found.reverse();
    found
}

/// Adds `used` to the record at `path`, making its folder when it is not
/// there; once the file passes 64 KiB it is written again trimmed
/// ([`trim`]) through a rename, so a reader never sees half a file.
pub fn append(path: &Path, used: &Use) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line(used).as_bytes())?;
    if file.metadata()?.len() > 64 * 1024 {
        let text = std::fs::read_to_string(path)?;
        let now = used.start + used.seconds;
        crate::files::replace(path, trim(&text, now).as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(start: u64, kind: Kind, app: &str) -> Use {
        Use {
            start,
            seconds: 60,
            kind,
            app: app.to_string(),
        }
    }

    #[test]
    fn a_line_reads_back_as_the_use_it_was() {
        let camera = used(1_760_515_862, Kind::Camera, "org.gnome.Snapshot");
        assert_eq!(line(&camera), "1760515862 60 camera org.gnome.Snapshot\n");
        assert_eq!(parse(&line(&camera)), vec![camera]);
        let spaced = used(5, Kind::Microphone, "Some App");
        assert_eq!(parse(&line(&spaced))[0].app, "Some_App");
    }

    #[test]
    fn lines_it_does_not_understand_are_skipped() {
        let text = "1 2 camera a\nnot a line\n3 4 hologram b\n5 6 screen c later words\n";
        let uses = parse(text);
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[1].kind, Kind::Screen);
        assert_eq!(uses[1].app, "c");
    }

    #[test]
    fn the_record_keeps_thirty_days_and_a_thousand_lines() {
        let day = 24 * 60 * 60;
        let now = 100 * day;
        let text = format!(
            "{}{}",
            line(&used(now - 31 * day, Kind::Camera, "old")),
            line(&used(now - day, Kind::Camera, "new"))
        );
        assert_eq!(parse(&trim(&text, now))[0].app, "new");
        let many: String = (0..1500)
            .map(|i| line(&used(now - 10 + i, Kind::Microphone, "a")))
            .collect();
        let kept = parse(&trim(&many, now));
        assert_eq!(kept.len(), KEEP_LINES);
        assert_eq!(kept[KEEP_LINES - 1].start, now - 10 + 1499);
    }

    #[test]
    fn an_apps_uses_come_newest_first_by_any_of_its_names() {
        let uses = parse(&format!(
            "{}{}{}",
            line(&used(1, Kind::Camera, "org.mozilla.firefox")),
            line(&used(2, Kind::Camera, "foot")),
            line(&used(3, Kind::Microphone, "firefox"))
        ));
        let found = of_app(&uses, &["org.mozilla.firefox", "firefox"]);
        assert_eq!(
            found.iter().map(|u| u.start).collect::<Vec<_>>(),
            vec![3, 1]
        );
    }

    #[test]
    fn each_kind_has_a_name_a_title_and_a_built_in_icon() {
        for kind in Kind::ALL {
            assert_eq!(Kind::named(kind.name()), Some(kind));
            assert!(!kind.title().is_empty());
            #[cfg(feature = "icons")]
            assert!(
                crate::icons::BUILT_IN
                    .iter()
                    .any(|(name, _)| *name == kind.icon()),
                "{} has no built-in icon {}",
                kind.name(),
                kind.icon()
            );
        }
        assert_eq!(Kind::Camera.title(), "Camera");
        assert_eq!(Kind::Screen.permission(), None);
        assert_eq!(Kind::named("location"), Some(Kind::Location));
        assert_eq!(Kind::Location.icon(), "location");
    }

    #[test]
    fn appending_writes_a_line_and_trims_a_long_record() {
        let dir = std::env::temp_dir().join(format!("edel-app-use-{}", std::process::id()));
        let path = dir.join("state").join(crate::places::APP_USE);
        append(&path, &used(1_000_000_000, Kind::Screen, "a")).expect("append");
        assert_eq!(
            parse(&std::fs::read_to_string(&path).expect("read")).len(),
            1
        );
        for i in 0..3000 {
            append(
                &path,
                &used(1_000_000_000 + i, Kind::Camera, "an.app.With.A.Long.Id"),
            )
            .expect("append");
        }
        let text = std::fs::read_to_string(&path).expect("read");
        let kept = parse(&text);
        assert!(text.len() <= 64 * 1024 + 64, "{} bytes", text.len());
        assert_eq!(kept.last().map(|u| u.start), Some(1_000_000_000 + 2999));
        std::fs::remove_dir_all(&dir).ok();
    }
}
