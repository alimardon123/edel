//! The apps the launcher offers (M5.3b), read from the XDG `.desktop`
//! files on each opening, so an app just installed is there: the
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
    pub name: String,
    /// What to run, split into words with the field codes taken out.
    pub argv: Vec<String>,
    pub terminal: bool,
    keywords: Vec<String>,
}

/// The directories `.desktop` files are read from, first wins.
pub fn dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut out: Vec<PathBuf> = data_home.iter().cloned().collect();
    out.extend(data_dirs.split(':').map(PathBuf::from));
    out.extend(data_home.map(|d| d.join("flatpak/exports/share")));
    out.push("/var/lib/flatpak/exports/share".into());
    let mut seen = Vec::new();
    for dir in out {
        let apps = dir.join("applications");
        if !seen.contains(&apps) {
            seen.push(apps);
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
            ids.push(id);
            if let Some(app) = std::fs::read_to_string(&file).ok().and_then(|t| parse(&t)) {
                apps.push(app);
            }
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
        if path.is_dir() {
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
    let (mut terminal, mut shown, mut application) = (false, true, false);
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
            "Name" => name = Some(unescape(value)),
            "Exec" => exec = Some(value.to_string()),
            "Terminal" => terminal = value == "true",
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
    let argv = split_exec(&exec?);
    let name = name.filter(|n| !n.is_empty())?;
    if !application || !shown || argv.is_empty() {
        return None;
    }
    Some(App {
        name,
        argv,
        terminal,
        keywords,
    })
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
        let apps = read_all(&[mine, system]);
        std::fs::remove_dir_all(&root).unwrap();
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Abc", "My Foot", "Tool"]);
    }

    #[test]
    fn a_terminal_app_runs_in_foot() {
        let top =
            parse("[Desktop Entry]\nType=Application\nName=top\nExec=htop\nTerminal=true").unwrap();
        assert_eq!(command(&top), ["foot", "--", "htop"]);
    }
}
