//! The apps people have (M5.3b), as shell-ui's launcher and panel and the
//! compositor's title bars (M5.6a) find them, read from the XDG `.desktop`
//! files each time they are asked for, so an app just installed is there: the
//! person's `~/.local/share/applications`, then each of
//! `$XDG_DATA_DIRS` (else `/usr/local/share` and `/usr/share`), then
//! Flatpak's exports; the first file with a given id wins, as the
//! Desktop Entry spec says. No GLib: the format is a few lines of
//! `key=value` under `[Desktop Entry]`. Plain data, tested without a
//! display.

use std::path::{Path, PathBuf};

/// One app people can start.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    /// Its desktop file id, `foot` for `foot.desktop`, which is also the
    /// app id its windows usually carry.
    pub id: String,
    pub name: String,
    /// What to run, split into words with the field codes taken out.
    pub argv: Vec<String>,
    pub terminal: bool,
    /// Its icon: a name in the icon themes, or a file's path (M5.4c).
    pub icon: Option<String>,
    /// What it is for, as the spec's categories (`WebBrowser`).
    pub categories: Vec<String>,
    keywords: Vec<String>,
}

/// The default apps' Flatpak installation (M6.2), by the id Flatpak's
/// `--installation=` takes; it lies at `places::DEFAULT_APPS`.
pub const INSTALLATION: &str = "edel";

/// The remote the default apps come from, in their installation only
/// (M6.2): Flathub's apps for now, under a name of its own, so the
/// remote people see stays `flathub` in their own installation.
pub const APPS_REMOTE: &str = "edel-apps";

/// The languages the default apps' installation keeps translations for,
/// as `flatpak config --set languages` takes them: English, the one
/// language Edel OS itself is in until `po/` holds a translation. Unset,
/// Flatpak keeps every language's, which made the runtimes' translations
/// about 1.5 GB of the 4.5 GB installation (CI's stick, 2026-10-26).
pub const LANGUAGES: &str = "en";

/// The default apps of a slot (M6.2), as `edel image build` records them
/// in `places::DEFAULT_APPS_LIST`: those the data partition's installation
/// should hold, and those a release dropped, which stay for each person
/// who used them.
#[derive(Debug, Default, PartialEq, serde::Deserialize)]
pub struct DefaultApps {
    #[serde(default)]
    pub flatpak: Vec<String>,
    #[serde(default)]
    pub flatpak_dropped: Vec<String>,
}

impl DefaultApps {
    /// The file's text, as the build writes it.
    pub fn text(&self) -> String {
        let list = |ids: &[String]| {
            ids.iter()
                .map(|id| format!("{id:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "# The default apps of this slot (roadmap M6.2), written by edel image\n\
             # build; edel update installs and updates them on the data partition.\n\
             format = 1\n\
             flatpak = [{}]\n\
             flatpak_dropped = [{}]\n",
            list(&self.flatpak),
            list(&self.flatpak_dropped)
        )
    }

    /// The file read leniently, as a machine reads what another release
    /// wrote (ADR-008): an id that is not one is left out.
    pub fn read(text: &str) -> DefaultApps {
        let mut apps: DefaultApps = toml::from_str(text).unwrap_or_default();
        apps.flatpak.retain(|id| is_flatpak_id(id));
        apps.flatpak_dropped.retain(|id| is_flatpak_id(id));
        apps
    }
}

/// What `flatpak remote-add` needs besides the name and the address
/// (M6.2): `--from` for a `.flatpakrepo` file, which brings its key, and
/// `--no-gpg-verify` for a local or plain-http test repository, which has
/// none; an https repository must carry its own key.
pub fn remote_add_args(from: &str) -> &'static [&'static str] {
    if from.ends_with(".flatpakrepo") {
        &["--from"]
    } else if from.starts_with("file://") || from.starts_with("http://") {
        &["--no-gpg-verify"]
    } else {
        &[]
    }
}

/// The file in `places::FLATPAK_INSTALLATIONS` that tells Flatpak where
/// the default apps' installation lies (M6.2), at `path`: the data
/// partition's place on a machine, the build's folder while an image is
/// made.
pub fn installation_conf(path: &str) -> String {
    format!(
        "# The default apps of Edel OS (roadmap M6.2), on the data partition,\n\
         # written by edel image build.\n\
         [Installation \"{INSTALLATION}\"]\n\
         Path={path}\n\
         DisplayName=Edel OS's default apps\n\
         StorageType=harddisk\n"
    )
}

/// The file's name in `places::FLATPAK_INSTALLATIONS`.
pub fn installation_file() -> String {
    format!("{INSTALLATION}.conf")
}

/// Flathub, the one remote Edel OS installs apps from (M6.1): `edel
/// settings apply` adds it for each admin person who has none, as that
/// person, with `flatpak --user remote-add --if-not-exists`.
pub const FLATHUB_REMOTE: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

/// Whether `id` is a Flatpak app id (M6.1): reverse-DNS, three or more
/// dot-separated parts, each starting with a letter or `_` and holding
/// only letters, digits, `_` and `-`, the last part with no `-`, and under
/// 256 characters, such as `io.mpv.Mpv`.
pub fn is_flatpak_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() < 256
        && parts.len() >= 3
        && parts.iter().all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        && !parts[parts.len() - 1].contains('-')
}

/// The directories `.desktop` files are read from, first wins.
pub fn dirs() -> Vec<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|d| d.join("applications"))
        .collect()
}

/// The XDG data directories, first wins: the person's, then
/// `$XDG_DATA_DIRS`, then Flatpak's exports; apps and icons live below.
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // Empty is unset, and relative paths are left out, as the spec says.
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut out: Vec<PathBuf> = data_home.iter().cloned().collect();
    out.extend(
        data_dirs
            .split(':')
            .map(PathBuf::from)
            .filter(|d| d.is_absolute()),
    );
    out.extend(data_home.map(|d| d.join("flatpak/exports/share")));
    out.push("/var/lib/flatpak/exports/share".into());
    out.push(Path::new(crate::places::DEFAULT_APPS).join("exports/share"));
    let mut seen = Vec::new();
    for dir in out {
        if !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    seen
}

/// Every app in `dirs`, by name.
pub fn read_all(dirs: &[PathBuf]) -> Vec<App> {
    let mut ids: Vec<String> = Vec::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.sort();
        for file in files {
            // The id is the path below the directory, / as -.
            let Ok(rel) = file.strip_prefix(dir) else {
                continue;
            };
            let id = rel.to_string_lossy().replace('/', "-");
            if ids.contains(&id) {
                continue;
            }
            if let Some(mut app) = std::fs::read_to_string(&file).ok().and_then(|t| parse(&t)) {
                app.id = id.trim_end_matches(".desktop").to_string();
                apps.push(app);
            }
            ids.push(id);
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Directories are followed only when they are not symlinks, so a
        // loop cannot recurse for ever; linked files, as Flatpak's
        // exports are, are read.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            out.push(path);
        }
    }
}

/// The app a `.desktop` file describes, if people can start it from a
/// launcher: an application, not hidden, with a name and a command.
pub fn parse(text: &str) -> Option<App> {
    let mut inside = false;
    let (mut name, mut exec, mut keywords) = (None, None, Vec::new());
    let (mut icon, mut categories) = (None, Vec::new());
    let (mut terminal, mut shown, mut application) = (false, true, false);
    let mut store = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if !inside || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" => application = value == "Application",
            // One line, whatever escapes it holds.
            "Name" => name = Some(unescape(value).replace(char::is_control, " ")),
            "Exec" => exec = Some(value.to_string()),
            "Terminal" => terminal = value == "true",
            "Icon" if !value.is_empty() => icon = Some(value.to_string()),
            "Categories" => {
                categories = value
                    .split(';')
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "MimeType" => store = value.split(';').any(|m| m == STORE_LINKS),
            "NoDisplay" | "Hidden" if value == "true" => shown = false,
            // Shown only on other desktops.
            "OnlyShowIn" => shown &= value.split(';').any(|d| d == "Edel"),
            "NotShowIn" => shown &= !value.split(';').any(|d| d == "Edel"),
            "Keywords" => {
                keywords = value
                    .split(';')
                    .filter(|k| !k.is_empty())
                    .map(|k| k.to_lowercase())
                    .collect();
            }
            _ => {}
        }
    }
    if store && !categories.iter().any(|c| c == PACKAGE_MANAGER) {
        categories.push(PACKAGE_MANAGER.to_string());
    }
    let argv = split_exec(&exec?);
    let name = name.filter(|n| !n.is_empty())?;
    if !application || !shown || argv.is_empty() {
        return None;
    }
    Some(App {
        id: String::new(),
        name,
        argv,
        terminal,
        icon,
        categories,
        keywords,
    })
}

/// What each role a preset can pin stands for (M5.4c): the apps whose
/// `.desktop` files give that category. People's own choice of default
/// apps comes with M6.2 and M6.10.
const ROLES: &[(&str, &str)] = &[
    ("files", "FileManager"),
    ("browser", "WebBrowser"),
    ("mail", "Email"),
    ("editor", "TextEditor"),
    ("terminal", "TerminalEmulator"),
    ("music", "Audio"),
    ("settings", "Settings"),
    // The store (M6.4), Bazaar by default.
    ("store", PACKAGE_MANAGER),
];

/// The category of an app that adds and removes software.
const PACKAGE_MANAGER: &str = "PackageManager";

/// The links every software centre opens (`appstream:` names an app in
/// the catalogue). Bazaar lists only `GTK;Utility;` as categories, so an
/// app that opens these links counts as a package manager too.
const STORE_LINKS: &str = "x-scheme-handler/appstream";

/// The app `pin` names: a role (`browser`) or a desktop file id
/// (`org.mozilla.firefox`); none when nothing installed fits.
pub fn pinned<'a>(apps: &'a [App], pin: &str) -> Option<&'a App> {
    match ROLES.iter().find(|(role, _)| *role == pin) {
        Some((_, category)) => apps
            .iter()
            .find(|a| a.categories.iter().any(|c| c == category)),
        None => apps.iter().find(|a| a.id == pin),
    }
}

/// The apps of `role`'s category (M6.10), in the order `apps` gives them,
/// so the first is the one [`default_app`] picks when no key names one.
/// None for a role `ROLES` does not hold.
pub fn apps_of_role<'a>(apps: &'a [App], role: &str) -> Vec<&'a App> {
    let Some((_, category)) = ROLES.iter().find(|(r, _)| *r == role) else {
        return Vec::new();
    };
    apps.iter()
        .filter(|a| a.categories.iter().any(|c| c == category))
        .collect()
}

/// The default app each role has in `[default_apps]` (M6.10): the role
/// and the settings key that names its app.
pub const DEFAULT_APP_KEYS: &[(&str, &str)] = &[
    ("files", "default_apps.files"),
    ("browser", "default_apps.browser"),
    ("editor", "default_apps.editor"),
    ("terminal", "default_apps.terminal"),
    ("mail", "default_apps.mail"),
];

/// The MIME types each role's default app opens (M6.10), in the order
/// the system's `mimeapps.list` lists them. A terminal has no MIME type,
/// so it is not here.
const MIME_TYPES: &[(&str, &[&str])] = &[
    (
        "browser",
        &[
            "x-scheme-handler/http",
            "x-scheme-handler/https",
            "text/html",
        ],
    ),
    ("files", &["inode/directory"]),
    ("editor", &["text/plain"]),
    ("mail", &["x-scheme-handler/mailto"]),
];

/// The app a role opens (M6.10): the one `chosen` names when it is
/// installed (an empty name is none), else the first app of the role's
/// category, as `pinned` finds it.
pub fn default_app<'a>(apps: &'a [App], role: &str, chosen: Option<&str>) -> Option<&'a App> {
    chosen
        .filter(|id| !id.is_empty())
        .and_then(|id| apps.iter().find(|a| a.id == id))
        .or_else(|| pinned(apps, role))
}

/// The first line of the `mimeapps.list` [`mime_defaults`] writes (M6.10).
/// A file that starts with it is Edel OS's to change; any other is the
/// administrator's, and nothing writes or removes it.
pub const MIME_COMMENT: &str = "# Written by edel settings apply from [default_apps] (M6.10); edit the settings file, not this file.";

/// The text of the system's `mimeapps.list` for the default apps a
/// settings file sets (M6.10), given as (role, desktop file id) pairs.
/// Empty when no MIME type is set, so nothing is written for a file that
/// sets none, or only the terminal.
pub fn mime_defaults(chosen: &[(&str, &str)]) -> String {
    let mut lines = Vec::new();
    for (role, types) in MIME_TYPES {
        let Some((_, id)) = chosen.iter().find(|(r, _)| r == role) else {
            continue;
        };
        for mime in types.iter() {
            lines.push(format!("{mime}={id}.desktop;\n"));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut text = format!("{MIME_COMMENT}\n[Default Applications]\n");
    for line in lines {
        text.push_str(&line);
    }
    text
}

/// A string value's escapes: `\s`, `\n`, `\t`, `\r` and `\\`.
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Others, such as Exec's \", are for the next reader.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `Exec` split into words: double quotes group, a backslash in them
/// escapes the next character, and the field codes (`%f`, `%U` and the
/// rest) go, as the launcher opens no files; `%%` is `%`.
fn split_exec(exec: &str) -> Vec<String> {
    let exec = unescape(exec);
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if any || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                any = false;
            }
            '%' => {
                if chars.next() == Some('%') {
                    word.push('%');
                }
            }
            c => word.push(c),
        }
    }
    if any || !word.is_empty() {
        words.push(word);
    }
    words.retain(|w| !w.is_empty());
    words
}

/// The apps matching `query`, best first: the name itself, then a name
/// that starts with it, then a word of the name that does, then a name,
/// keyword or command that holds it; case aside. An empty query matches
/// every app, in their order (by name).
pub fn search<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let query = query.trim().to_lowercase();
    let mut found: Vec<(u8, &App)> = apps
        .iter()
        .filter_map(|app| {
            if query.is_empty() {
                return Some((0, app));
            }
            let name = app.name.to_lowercase();
            let command = app.argv[0].rsplit('/').next().unwrap_or("").to_lowercase();
            let rank = if name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else if name.split_whitespace().any(|w| w.starts_with(&query)) {
                2
            } else if name.contains(&query)
                || command.contains(&query)
                || app.keywords.iter().any(|k| k.contains(&query))
            {
                3
            } else {
                return None;
            };
            Some((rank, app))
        })
        .collect();
    // Stable: within a rank the apps stay by name.
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, app)| app).collect()
}

/// What to run for `app`: its command, in foot if it wants a terminal.
pub fn command(app: &App) -> Vec<String> {
    if app.terminal {
        let mut argv = vec!["foot".to_string(), "--".to_string()];
        argv.extend(app.argv.iter().cloned());
        argv
    } else {
        app.argv.clone()
    }
}

/// Whether a window with app id `app_id` belongs to the app whose desktop
/// file id is `id`: the same, whatever the case, or the same last part, as
/// `org.mozilla.firefox` and `firefox` are.
pub fn belongs(app_id: &str, id: &str) -> bool {
    if app_id.is_empty() || id.is_empty() {
        return false;
    }
    let last = |s: &str| s.rsplit('.').next().unwrap_or(s).to_lowercase();
    app_id.eq_ignore_ascii_case(id) || last(app_id) == last(id)
}

/// The app a window with app id `app_id` belongs to, its own desktop
/// file first.
pub fn of_window<'a>(apps: &'a [App], app_id: &str) -> Option<&'a App> {
    apps.iter()
        .find(|a| a.id.eq_ignore_ascii_case(app_id))
        .or_else(|| apps.iter().find(|a| belongs(app_id, &a.id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOOT: &str = "[Desktop Entry]\nType=Application\nExec=foot\nIcon=foot\nTerminal=false\nCategories=System;TerminalEmulator;\nKeywords=shell;prompt;command;commandline;\nName=Foot\nGenericName=Terminal\n\n[Desktop Action new]\nName=Other\nExec=other\n";

    #[test]
    fn a_desktop_file_gives_its_name_and_command() {
        let foot = parse(FOOT).unwrap();
        assert_eq!(foot.name, "Foot");
        assert_eq!(foot.argv, ["foot"]);
        assert!(!foot.terminal);
        // Hidden, not an application, or for another desktop: none.
        assert_eq!(
            parse(&format!("{FOOT}\n[Desktop Entry]\nNoDisplay=true")),
            None
        );
        assert_eq!(parse("[Desktop Entry]\nType=Link\nName=Web\nExec=x"), None);
        assert_eq!(
            parse("[Desktop Entry]\nType=Application\nName=K\nExec=k\nOnlyShowIn=KDE;"),
            None
        );
        assert!(
            parse("[Desktop Entry]\nType=Application\nName=E\nExec=e\nOnlyShowIn=KDE;Edel;")
                .is_some()
        );
        assert_eq!(
            parse("[Desktop Entry]\nType=Application\nName=X"),
            None,
            "no Exec"
        );
        let two = parse("[Desktop Entry]\nType=Application\nName=Two\\nLines\nExec=x").unwrap();
        assert_eq!(two.name, "Two Lines", "one line");
    }

    #[test]
    fn exec_splits_into_words_without_field_codes() {
        assert_eq!(split_exec("gimp %U"), ["gimp"]);
        assert_eq!(
            split_exec(r#""/opt/My App/run" --name "a \"b\"" %f"#),
            ["/opt/My App/run", "--name", "a \"b\""]
        );
        assert_eq!(split_exec("printf 100%%"), ["printf", "100%"]);
        assert_eq!(
            split_exec("flatpak run --branch=stable org.gnome.Calculator @@u %U @@"),
            [
                "flatpak",
                "run",
                "--branch=stable",
                "org.gnome.Calculator",
                "@@u",
                "@@"
            ]
        );
    }

    #[test]
    fn search_puts_names_that_start_with_the_query_first() {
        let app = |name: &str, exec: &str| {
            parse(&format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nKeywords=shell;"
            ))
            .unwrap()
        };
        let apps = vec![
            app("Foot Client", "footclient"),
            app("Foot", "foot"),
            app("Files", "nautilus"),
            app("Bigfoot", "bigfoot"),
            app("Text Editor", "gnome-text-editor"),
        ];
        let names = |q: &str| {
            search(&apps, q)
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        };
        // The name itself first, then those starting with it.
        assert_eq!(names("foot"), ["Foot", "Foot Client", "Bigfoot"]);
        assert_eq!(names("fo"), ["Foot Client", "Foot", "Bigfoot"]);
        assert_eq!(names("ed"), ["Text Editor"]);
        assert_eq!(names("nautilus"), ["Files"]);
        assert_eq!(names("SHELL").len(), 5, "by keyword");
        assert_eq!(names("").len(), 5);
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn the_first_file_with_an_id_wins_and_apps_sort_by_name() {
        let root = std::env::temp_dir().join(format!("edel-apps-{}", std::process::id()));
        let (mine, system) = (root.join("mine"), root.join("system"));
        std::fs::create_dir_all(mine.join("sub")).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        let entry =
            |name: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\n");
        std::fs::write(mine.join("foot.desktop"), entry("My Foot")).unwrap();
        std::fs::write(system.join("foot.desktop"), entry("Foot")).unwrap();
        std::fs::write(system.join("abc.desktop"), entry("Abc")).unwrap();
        std::fs::write(mine.join("sub/tool.desktop"), entry("Tool")).unwrap();
        std::fs::write(system.join("notes.txt"), "not an app").unwrap();
        // A linked file is read, as Flatpak's exports are; a linked
        // directory, here a loop, is not followed.
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("flat.desktop"), entry("Flat")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("flat.desktop"), system.join("flat.desktop"))
            .unwrap();
        std::os::unix::fs::symlink(&mine, mine.join("loop")).unwrap();
        let apps = read_all(&[mine, system]);
        std::fs::remove_dir_all(&root).unwrap();
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Abc", "Flat", "My Foot", "Tool"]);
    }

    #[test]
    fn a_terminal_app_runs_in_foot() {
        let top =
            parse("[Desktop Entry]\nType=Application\nName=top\nExec=htop\nTerminal=true").unwrap();
        assert_eq!(command(&top), ["foot", "--", "htop"]);
    }

    #[test]
    fn a_flatpak_id_is_reverse_dns_with_no_dash_in_its_last_part() {
        for good in [
            "io.mpv.Mpv",
            "org.gnome.Calculator",
            "com.github.user-name.app",
            "_x.y.z_1",
        ] {
            assert!(is_flatpak_id(good), "{good} is a Flatpak id");
        }
        for bad in [
            "",
            "mpv",
            "io.mpv",
            "io..mpv",
            "1o.mpv.Mpv",
            "io.mpv.Mpv-x",
            "io.mp v.Mpv",
            "io.mpv.Mp/v",
            ".io.mpv",
        ] {
            assert!(!is_flatpak_id(bad), "{bad:?} is not a Flatpak id");
        }
        let long = format!("a.b.{}", "c".repeat(256));
        assert!(!is_flatpak_id(&long), "256 characters or more is too long");
    }

    #[test]
    fn the_apps_of_a_role_are_its_category_in_the_given_order() {
        let app = |id: &str, category: &str| App {
            id: id.to_string(),
            ..parse(&format!(
                "[Desktop Entry]\nType=Application\nName={id}\nExec={id}\nCategories={category};\n"
            ))
            .unwrap()
        };
        let apps = vec![
            app("org.gnome.Nautilus", "FileManager"),
            app("org.mozilla.firefox", "WebBrowser"),
            app("org.gnome.Epiphany", "WebBrowser"),
        ];
        let ids = |role: &str| -> Vec<String> {
            apps_of_role(&apps, role)
                .iter()
                .map(|a| a.id.clone())
                .collect()
        };
        assert_eq!(
            ids("browser"),
            ["org.mozilla.firefox", "org.gnome.Epiphany"]
        );
        assert_eq!(ids("files"), ["org.gnome.Nautilus"]);
        assert!(ids("mail").is_empty(), "no app of the category, none");
        assert!(ids("nonesuch").is_empty(), "a role ROLES does not hold");
        assert_eq!(
            apps_of_role(&apps, "browser")
                .first()
                .map(|a| a.id.as_str()),
            default_app(&apps, "browser", None).map(|a| a.id.as_str()),
            "the first is the one default_app picks"
        );
    }

    #[test]
    fn a_default_app_is_the_chosen_one_when_installed_else_the_first_of_its_role() {
        // `read_all` gives each app its id from its file name; `parse` does not.
        let app = |id: &str, category: &str| App {
            id: id.to_string(),
            ..parse(&format!(
                "[Desktop Entry]\nType=Application\nName={id}\nExec={id}\nCategories={category};\n"
            ))
            .unwrap()
        };
        let apps = vec![
            app("org.gnome.Nautilus", "FileManager"),
            app("org.mozilla.firefox", "WebBrowser"),
            app("org.gnome.Epiphany", "WebBrowser"),
        ];
        let id_of = |found: Option<&App>| found.map(|a| a.id.clone());
        assert_eq!(
            id_of(default_app(&apps, "browser", Some("org.gnome.Epiphany"))),
            Some("org.gnome.Epiphany".into()),
            "the chosen app, when installed"
        );
        assert_eq!(
            id_of(default_app(&apps, "browser", Some("org.example.Missing"))),
            Some("org.mozilla.firefox".into()),
            "a chosen app that is not installed falls back to the category"
        );
        assert_eq!(
            id_of(default_app(&apps, "browser", None)),
            Some("org.mozilla.firefox".into()),
            "nothing chosen takes the category's first app"
        );
        assert_eq!(
            id_of(default_app(&apps, "browser", Some(""))),
            Some("org.mozilla.firefox".into()),
            "an empty choice is none"
        );
        assert!(
            default_app(&apps, "terminal", None).is_none(),
            "no app of the role"
        );
    }

    #[test]
    fn mime_defaults_write_each_role_under_its_types_in_table_order() {
        assert_eq!(
            mime_defaults(&[
                ("mail", "org.example.Mail"),
                ("editor", "org.gnome.TextEditor"),
                ("browser", "org.mozilla.firefox"),
                ("files", "org.gnome.Nautilus"),
            ]),
            "# Written by edel settings apply from [default_apps] (M6.10); edit the settings file, not this file.\n\
             [Default Applications]\n\
             x-scheme-handler/http=org.mozilla.firefox.desktop;\n\
             x-scheme-handler/https=org.mozilla.firefox.desktop;\n\
             text/html=org.mozilla.firefox.desktop;\n\
             inode/directory=org.gnome.Nautilus.desktop;\n\
             text/plain=org.gnome.TextEditor.desktop;\n\
             x-scheme-handler/mailto=org.example.Mail.desktop;\n"
        );
    }

    #[test]
    fn mime_defaults_is_empty_when_no_mime_type_is_set() {
        assert_eq!(mime_defaults(&[]), "");
        assert_eq!(
            mime_defaults(&[("terminal", "foot")]),
            "",
            "a terminal has no MIME type"
        );
    }

    #[test]
    fn flathub_is_added_from_the_one_constant() {
        assert_eq!(
            FLATHUB_REMOTE,
            "https://dl.flathub.org/repo/flathub.flatpakrepo"
        );
    }

    #[test]
    fn flatpak_finds_the_default_apps_where_the_build_put_them() {
        let conf = installation_conf("/data/edel/apps");
        assert!(
            conf.contains("[Installation \"edel\"]\nPath=/data/edel/apps\n"),
            "{conf}"
        );
        assert_eq!(installation_file(), "edel.conf");
        assert!(
            data_dirs()
                .iter()
                .any(|d| d == &Path::new(crate::places::DEFAULT_APPS).join("exports/share"))
        );
    }

    #[test]
    fn a_remote_is_added_with_its_key_or_as_a_test_repository() {
        assert_eq!(remote_add_args(FLATHUB_REMOTE), ["--from"]);
        assert_eq!(
            remote_add_args("file:///src/out/apps-repo"),
            ["--no-gpg-verify"]
        );
        assert_eq!(
            remote_add_args("http://10.0.2.2:8002/"),
            ["--no-gpg-verify"]
        );
        assert!(remote_add_args("https://example.org/repo").is_empty());
    }

    #[test]
    fn the_default_apps_list_reads_back_and_drops_what_is_no_id() {
        let apps = DefaultApps {
            flatpak: vec!["org.edel.TestB".into()],
            flatpak_dropped: vec!["org.edel.TestA".into()],
        };
        assert_eq!(DefaultApps::read(&apps.text()), apps);
        let odd =
            DefaultApps::read("format = 1\nflatpak = [\"org.edel.TestB\", \"nope\"]\nnew = 1\n");
        assert_eq!(odd.flatpak, ["org.edel.TestB"]);
        assert_eq!(DefaultApps::read("not toml ["), DefaultApps::default());
    }

    #[test]
    fn the_store_role_finds_the_package_manager() {
        let store = parse(
            "[Desktop Entry]\nType=Application\nName=Bazaar\nExec=bazaar-daemon %U\nCategories=GTK;Utility;\nMimeType=x-scheme-handler/appstream;x-scheme-handler/flatpak;\n",
        )
        .unwrap();
        let apps = [store];
        assert_eq!(
            pinned(&apps, "store").map(|a| a.name.as_str()),
            Some("Bazaar")
        );
        let other = parse(
            "[Desktop Entry]\nType=Application\nName=Viewer\nExec=viewer\nCategories=Utility;\nMimeType=x-scheme-handler/appstreamx;\n",
        )
        .unwrap();
        assert_eq!(pinned(&[other], "store"), None);
    }
}
