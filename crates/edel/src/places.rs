//! Where Edel OS keeps its files (ADR-010): every part names Edel OS's
//! places through this module, so a path or a file's name is written once
//! and a change to it is one line here. M5.25a starts it with the settings
//! file, the one file that describes a machine (ADR-006, ADR-008's same
//! names decision); M5.27 brings Edel OS's other places here.
//!
//! CI's shell scripts read the same names from `ci/names.sh`, which a test
//! holds equal to this module, and a test fails when the settings file's
//! name is written anywhere else in the code, CI or the features.

use std::path::{Path, PathBuf};

/// The settings file's name. A rename, such as to `edel.toml`, changes
/// this line and adds the old name to [`FORMER_SETTINGS`].
pub const SETTINGS: &str = "settings.toml";

/// Names the settings file had before, newest first. Readers still read a
/// file by one of them when none has the new name, and `apply` renames it
/// on disk, so a machine set up before a rename keeps its settings.
pub const FORMER_SETTINGS: &[&str] = &["system.toml"];

// Each root below is written once, in its macro; every place under it is
// built from the macro, so moving a root is one line.
macro_rules! data {
    ($path:literal) => {
        concat!("/data", $path)
    };
}
macro_rules! run {
    ($path:literal) => {
        concat!("/run/edel", $path)
    };
}
macro_rules! share {
    ($path:literal) => {
        concat!("/usr/share/edel", $path)
    };
}

/// Where the data partition, shared by both slots, is mounted.
pub const DATA_MOUNT: &str = data!("");
/// Edel OS's directory on the data partition.
pub const DATA_DIR: &str = data!("/edel");
/// What this machine changed in `/etc`: a file here differs from the
/// slot's (the overlay's upper directory, M1.4).
pub const ETC_UPPER: &str = data!("/etc/upper");
/// Developer mode is on while this file exists (ADR-007).
pub const DEVELOPER_FLAG: &str = data!("/edel/developer");
/// The last fallback from a slot that did not start, for shell-ui to show
/// once (M1.1, M5.9).
pub const LAST_FALLBACK: &str = data!("/edel/last-fallback.toml");

/// Edel OS's directory for this boot only.
pub const RUN_DIR: &str = run!("");
/// What the person's session leaves for root (M4.8).
pub const SESSION_DIR: &str = run!("/session");
/// What the compositor shows: windows, layers, the effect tier (M4.3).
pub const STATE_FILE: &str = run!("/session/state.toml");
/// The desktop's health: the compositor's first frame (M4.8).
pub const READY_FILE: &str = run!("/session/ready");
/// Where a feature's own health checks leave their files, one per name,
/// such as a server's `network` (M1.11).
pub const HEALTH_DIR: &str = run!("/health");
/// The default runlevel's health (M1.5).
pub const DEFAULT_REACHED: &str = run!("/default-reached");
/// Written once the guard confirmed this boot's slot (M1.5).
pub const CONFIRMED: &str = run!("/confirmed");
/// The boot's "Started in" line, for `edel report` (M3.2).
pub const STARTED: &str = run!("/started");
/// One updater at a time (M1.1).
pub const UPDATE_LOCK: &str = run!("/update.lock");
/// greetd's config with the live session (M3.6).
pub const GREETD_LIVE: &str = run!("/greetd.toml");
/// Where a seed stick or partition is mounted while it is read (M2.2).
pub const SEED_MOUNT: &str = run!("/seed");
/// Where the installer mounts the new disk's partitions (M2.4).
pub const INSTALL_DIR: &str = run!("/install");

/// Where a slot keeps Edel OS's own files.
pub const SHARE_DIR: &str = share!("");
/// The image's feature files (M4.0).
pub const FEATURES_DIR: &str = share!("/features");
/// The public keys releases are checked against (M1.6).
pub const KEYS_DIR: &str = share!("/keys");
/// Every package in the slot, one per line (M3.1).
pub const PACKAGES_FILE: &str = share!("/packages");
/// The design tokens a slot may carry in place of the built-in ones.
pub const TOKENS_FILE: &str = share!("/design/tokens.toml");
/// GTK's colours from the tokens (M5.5b).
pub const GTK_CSS: &str = share!("/gtk.css");
/// Where each part's words in each language are (M5.24): `LANG/PART.po`.
pub const LOCALE_DIR: &str = share!("/locale");
/// Alpine's initramfs `init` with our one change, which finds a root
/// named by its partition (`root=PARTUUID=`, M1.12); `mkinitfs -i` uses it.
pub const INITRAMFS_INIT: &str = share!("/initramfs-init");
/// These places as shell variables, for the slot's own scripts (M5.27).
pub const PLACES_SH: &str = share!("/places.sh");

/// Where people's home directories are.
pub const HOMES: &str = "/home";
/// A person's state directory for Edel OS inside their home, where
/// `XDG_STATE_HOME` is not set: their session's log (M5.28a).
pub const STATE_IN_HOME: &str = ".local/state/edel";
/// The desktop session's log, in a person's state directory: the
/// compositor and what it starts write to it (M5.28a).
pub const SESSION_LOG: &str = "session.log";
/// The session before's log, beside it: two sessions are kept.
pub const SESSION_LOG_BEFORE: &str = "session.old.log";
/// The system log busybox's syslogd writes, on the data partition.
pub const SYSTEM_LOG: &str = "/var/log/messages";

/// The boot loader the slot carries, which rides along with updates (M1.8).
pub const SLOT_BOOT_DIR: &str = "/usr/lib/edel/boot";
/// Edel OS's directory on the EFI system partition: GRUB, its counters and
/// a seed settings file.
pub const ESP_DIR: &str = "/EFI/edel";

/// Where admins look first: `/etc/edel/` holds a link to the machine's
/// settings file.
pub const ETC_DIR: &str = "/etc/edel";

/// The places shell scripts use, as `NAME=VALUE` lines: `ci/names.sh` for
/// CI and `/usr/share/edel/places.sh` in every image, both written from
/// here (M5.27).
pub fn shell_vars() -> Vec<(&'static str, String)> {
    vec![
        ("settings_name", SETTINGS.to_string()),
        ("data_dir", DATA_DIR.to_string()),
        ("run_dir", RUN_DIR.to_string()),
        ("session_dir", SESSION_DIR.to_string()),
        ("state_file", STATE_FILE.to_string()),
        ("ready_file", READY_FILE.to_string()),
        ("default_reached", DEFAULT_REACHED.to_string()),
        ("health_dir", HEALTH_DIR.to_string()),
        ("confirmed_file", CONFIRMED.to_string()),
        ("started_file", STARTED.to_string()),
        ("greetd_live", GREETD_LIVE.to_string()),
        ("last_fallback", LAST_FALLBACK.to_string()),
        ("share_dir", SHARE_DIR.to_string()),
        ("esp_dir", ESP_DIR.to_string()),
        // Inside a person's home (M5.28a).
        ("home_session_log", format!("{STATE_IN_HOME}/{SESSION_LOG}")),
        ("system_log", SYSTEM_LOG.to_string()),
    ]
}

/// `shell_vars` as a sourced shell file, with a header saying what wrote
/// it.
pub fn shell_file(what: &str) -> String {
    let mut text = format!(
        "# {what}.\n\
         # Written by edel::places (crates/edel/src/places.rs); never edit it\n\
         # by hand. Sourced, not run.\n\
         # shellcheck disable=SC2034\n"
    );
    for (name, value) in shell_vars() {
        text.push_str(&format!("{name}={value}\n"));
    }
    text
}

/// The machine's settings file, on the data partition.
pub fn machine_settings() -> PathBuf {
    Path::new(DATA_DIR).join(SETTINGS)
}

/// The slot's own settings file, which seeds a first boot when no stick
/// or EFI system partition holds one.
pub fn slot_settings() -> PathBuf {
    Path::new(SHARE_DIR).join(SETTINGS)
}

/// The link to the machine's settings file in `/etc/edel/`.
pub fn etc_settings() -> PathBuf {
    Path::new(ETC_DIR).join(SETTINGS)
}

/// A person's own directory for Edel OS: `$XDG_CONFIG_HOME/edel`, else
/// `~/.config/edel`; none without either variable.
pub fn person_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(config.join("edel"))
}

/// A person's state directory for Edel OS: `$XDG_STATE_HOME/edel`, else
/// [`STATE_IN_HOME`] in their home; none without either variable.
pub fn person_state_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    match var("XDG_STATE_HOME") {
        Some(state) => Some(PathBuf::from(state).join("edel")),
        None => var("HOME").map(|home| PathBuf::from(home).join(STATE_IN_HOME)),
    }
}

/// `part`'s words in `language`, such as `de` or `pt_BR` (M5.24).
pub fn catalogue(language: &str, part: &str) -> PathBuf {
    Path::new(LOCALE_DIR)
        .join(language)
        .join(format!("{part}.po"))
}

/// A person's own settings file, with the same keys as the machine's; for
/// the keys the desktop reads, its values win.
pub fn person_settings() -> Option<PathBuf> {
    person_dir().map(|dir| dir.join(SETTINGS))
}

/// The settings file named `name` inside `dir`, such as on a seed stick or
/// the EFI system partition.
pub fn settings_in(dir: &Path) -> PathBuf {
    dir.join(SETTINGS)
}

/// Whether `name` is the settings file's name, or one it had before; a
/// watcher follows a file by either.
pub fn is_settings_name(name: &str) -> bool {
    name == SETTINGS || FORMER_SETTINGS.contains(&name)
}

/// `path`, a settings file, as it is found: itself when it exists, else
/// the first file beside it by a former name that does, else `path`.
pub fn found(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    FORMER_SETTINGS
        .iter()
        .map(|old| path.with_file_name(old))
        .find(|old| old.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// The names to look for in a directory, the settings file's own first.
pub fn settings_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(SETTINGS).chain(FORMER_SETTINGS.iter().copied())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_former_name_is_found_when_the_new_one_is_missing() {
        let dir = std::env::temp_dir().join(format!("edel-places-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = settings_in(&dir);
        assert_eq!(found(&path), path);
        fs::write(dir.join(FORMER_SETTINGS[0]), "format = 1\n").unwrap();
        assert_eq!(found(&path), dir.join(FORMER_SETTINGS[0]));
        fs::write(&path, "format = 1\n").unwrap();
        assert_eq!(found(&path), path);
        fs::remove_dir_all(&dir).unwrap();
        assert!(is_settings_name(SETTINGS) && is_settings_name(FORMER_SETTINGS[0]));
        assert!(!is_settings_name("state.toml"));
    }

    /// `ci/names.sh` says what this module says, for CI's shell scripts;
    /// with EDEL_WRITE_DOCS set, the test writes it.
    #[test]
    fn ci_names_are_these() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ci/names.sh");
        let want = shell_file("The places CI's scripts share with the code (ADR-010, M5.27)");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            fs::write(&path, &want).unwrap();
        }
        assert!(
            fs::read_to_string(&path).unwrap_or_default() == want,
            "ci/names.sh differs from edel::places; run EDEL_WRITE_DOCS=1 cargo test -p edel ci_names"
        );
    }

    /// The settings file's name, and every name it had, is written here
    /// and in `ci/names.sh` only: code, CI and features take it from
    /// them, and docs say "the settings file" (ADR-010).
    #[test]
    fn the_settings_file_is_named_in_one_place() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let allowed = [
            root.join("crates/edel/src/places.rs"),
            root.join("ci/names.sh"),
        ];
        let mut found = Vec::new();
        for top in ["crates", "ci", "features", "images", "presets"] {
            walk(&root.join(top), &mut |path| {
                if allowed.iter().any(|a| same(a, path))
                    || path.extension().is_some_and(|e| e == "md")
                    || path.components().any(|c| c.as_os_str() == "target")
                {
                    return;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let text = fs::read_to_string(path).unwrap_or_default();
                // A feature file is named after its feature: the settings
                // feature (M5.6a) is features/settings.toml, not this file.
                let feature = path
                    .parent()
                    .is_some_and(|p| same(p, &root.join("features")));
                for named in settings_names() {
                    if (name == named && !feature) || text.contains(named) {
                        found.push(format!("{} names {named}", path.display()));
                    }
                }
            });
        }
        assert!(
            found.is_empty(),
            "the settings file's name belongs in edel::places and ci/names.sh only:\n{}",
            found.join("\n")
        );
    }

    fn same(a: &Path, b: &Path) -> bool {
        fs::canonicalize(a).ok() == fs::canonicalize(b).ok()
    }

    fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, visit);
            } else {
                visit(&path);
            }
        }
    }
}
