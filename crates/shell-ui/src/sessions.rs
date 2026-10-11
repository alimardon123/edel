//! The desktop sessions the login screen offers (M5.10c3b): the standard
//! `wayland-sessions` files, read as plain text, the one `--cmd` names when
//! no file does, and each person's last choice, kept in
//! `places::GREETER_SESSIONS`. Plain data and text, tested without a
//! display. The login screen writes a choice only once it is a session
//! other than the default, so a default is never written.

use std::path::Path;

use crate::telling;

/// What the login screen writes above each person's choice.
const HEADER: &str =
    "# Each person's desktop for their login, written by the login screen (M5.10c3b).\n";

/// One desktop session: its file's id (the name without `.desktop`), the
/// name people read and the command greetd starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub cmd: Vec<String>,
}

/// The words of a command line: split on whitespace, with the desktop
/// entry's field codes (`%f`, `%U` and the like) dropped.
fn words(exec: &str) -> Vec<String> {
    exec.split_whitespace()
        .filter(|w| !w.starts_with('%'))
        .map(str::to_string)
        .collect()
}

/// One `.desktop` file as a session: its `[Desktop Entry]`'s `Name` (its
/// id when it has none) and `Exec`, with `Hidden` and `NoDisplay` sessions
/// and those without a command left out. Localised names (`Name[xx]`) are
/// not read.
pub fn parse(id: &str, text: &str) -> Option<Session> {
    let mut in_entry = false;
    let (mut name, mut exec, mut skip) = (None, None, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" => name = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "Hidden" | "NoDisplay" => skip |= value == "true",
            _ => {}
        }
    }
    if skip {
        return None;
    }
    let cmd = words(&exec?);
    if cmd.is_empty() {
        return None;
    }
    let name = name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string());
    Some(Session {
        id: id.to_string(),
        name,
        cmd,
    })
}

/// The sessions in `dir`, the `.desktop` files that parse, by name. A
/// directory that cannot be read has none.
pub fn read(dir: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Session> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            parse(&id, &text)
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    found
}

/// Our own session's desktop file id, `features/login`'s `edel.desktop`.
pub const OURS: &str = "edel";

/// The sessions to offer and the index of the default one. The session
/// whose command is `cmd` (the login screen's `--cmd`) is the default; when
/// no file names it, our own session takes `cmd` as its command, and
/// without our file `cmd` is an extra first session named as Edel OS, so
/// a machine with no desktop files offers exactly what it did before.
pub fn with_default(found: Vec<Session>, cmd: &str) -> (Vec<Session>, usize) {
    let wanted = words(cmd);
    if let Some(index) = found.iter().position(|s| s.cmd == wanted) {
        return (found, index);
    }
    if wanted.is_empty() {
        return (found, 0);
    }
    // Our own session's file (`features/login`) with another command: the
    // login screen's `--cmd` is how this machine starts it, so it takes
    // that command rather than standing twice in the menu.
    let mut found = found;
    if let Some(ours) = found.iter_mut().position(|s| s.id == OURS) {
        found[ours].cmd = wanted;
        return (found, ours);
    }
    let id = Path::new(&wanted[0])
        .file_name()
        .map_or(String::new(), |f| f.to_string_lossy().into_owned());
    let extra = Session {
        id,
        name: telling::APP.to_string(),
        cmd: wanted,
    };
    let mut all = vec![extra];
    all.extend(found);
    (all, 0)
}

/// The desktop id `login` remembered, if its line is in `text`.
pub fn remembered(text: &str, login: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table
        .get("people")?
        .as_table()?
        .get(login)?
        .as_str()
        .map(str::to_string)
}

/// `text` with `login`'s choice set to `id`, or taken out when `id` is
/// `None` (a default is never written). Other people's lines stay, and a
/// file that does not read is started afresh.
pub fn remember(text: &str, login: &str, id: Option<&str>) -> String {
    let mut table: toml::Table = text.parse().unwrap_or_default();
    let mut people = match table.remove("people") {
        Some(toml::Value::Table(people)) => people,
        _ => toml::Table::new(),
    };
    match id {
        Some(id) => {
            people.insert(login.to_string(), toml::Value::String(id.to_string()));
        }
        None => {
            people.remove(login);
        }
    }
    if !people.is_empty() {
        table.insert("people".to_string(), toml::Value::Table(people));
    }
    format!("{HEADER}{}", toml::to_string(&table).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDEL: &str = "[Desktop Entry]\nName=Edel OS\nComment=The Edel shell\nExec=/usr/libexec/edel-session\nType=Application\n";

    #[test]
    fn a_desktop_file_gives_its_name_and_command() {
        let s = parse("edel", EDEL).expect("a session");
        assert_eq!(s.id, "edel");
        assert_eq!(s.name, "Edel OS");
        assert_eq!(s.cmd, vec!["/usr/libexec/edel-session".to_string()]);
    }

    #[test]
    fn field_codes_are_dropped_from_the_command() {
        let text = "[Desktop Entry]\nName=Two\nExec=env A=1 /usr/bin/app %f %U %%x\n";
        let s = parse("two", text).expect("a session");
        assert_eq!(s.cmd, vec!["env", "A=1", "/usr/bin/app"]);
    }

    #[test]
    fn hidden_and_no_display_sessions_are_left_out() {
        let hidden = "[Desktop Entry]\nName=H\nExec=/bin/h\nHidden=true\n";
        let shy = "[Desktop Entry]\nName=N\nExec=/bin/n\nNoDisplay=true\n";
        assert_eq!(parse("h", hidden), None);
        assert_eq!(parse("n", shy), None);
        let shown = "[Desktop Entry]\nName=S\nExec=/bin/s\nHidden=false\n";
        assert!(parse("s", shown).is_some());
    }

    #[test]
    fn a_session_without_an_exec_is_left_out() {
        assert_eq!(parse("x", "[Desktop Entry]\nName=X\n"), None);
        // Field codes alone are no command.
        assert_eq!(parse("y", "[Desktop Entry]\nExec=%u\n"), None);
    }

    #[test]
    fn localised_names_are_ignored_and_the_id_stands_in_for_no_name() {
        let text = "[Desktop Entry]\nName[de]=Deutsch\nExec=/bin/d\n";
        assert_eq!(parse("plain", text).unwrap().name, "plain");
        let both = "[Desktop Entry]\nName[de]=Deutsch\nName=English\nExec=/bin/d\n";
        assert_eq!(parse("both", both).unwrap().name, "English");
    }

    #[test]
    fn only_the_desktop_entry_section_counts() {
        let text =
            "[Other]\nName=Wrong\nExec=/bin/wrong\n[Desktop Entry]\nName=Right\nExec=/bin/right\n";
        let s = parse("r", text).unwrap();
        assert_eq!(s.name, "Right");
        assert_eq!(s.cmd, vec!["/bin/right"]);
    }

    #[test]
    fn read_lists_the_desktop_files_by_name() {
        let dir = std::env::temp_dir().join(format!("edel-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("zeta.desktop"),
            "[Desktop Entry]\nName=Alpha\nExec=/bin/a\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("beta.desktop"),
            "[Desktop Entry]\nName=Beta\nExec=/bin/b\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes.txt"),
            "[Desktop Entry]\nName=Not\nExec=/bin/n\n",
        )
        .unwrap();
        let ids: Vec<String> = read(&dir).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["zeta", "beta"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(read(&dir).is_empty());
    }

    #[test]
    fn the_default_is_the_session_whose_command_is_the_cmd() {
        let found = vec![
            parse("a", "[Desktop Entry]\nName=A\nExec=/bin/a\n").unwrap(),
            parse("edel", EDEL).unwrap(),
        ];
        let (list, default) = with_default(found.clone(), "/usr/libexec/edel-session");
        assert_eq!(list, found, "no extra session when a file names it");
        assert_eq!(default, 1);
        assert_eq!(list[default].id, "edel");
    }

    #[test]
    fn a_cmd_no_file_names_is_an_extra_first_session() {
        let found = vec![parse("a", "[Desktop Entry]\nName=A\nExec=/bin/a\n").unwrap()];
        let (list, default) = with_default(found.clone(), "env X=1 /bin/edel");
        assert_eq!(default, 0);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, telling::APP);
        assert_eq!(list[0].cmd, vec!["env", "X=1", "/bin/edel"]);
        assert_eq!(list[0].id, "env");
        assert_eq!(&list[1..], &found[..]);
    }

    #[test]
    fn our_own_file_takes_a_cmd_no_file_names_so_it_stands_once() {
        let found = vec![
            parse("ci-other", "[Desktop Entry]\nName=CI other\nExec=/bin/a\n").unwrap(),
            parse(OURS, EDEL).unwrap(),
        ];
        let (list, default) = with_default(found, "env X=1 dbus-run-session -- edel-compositor");
        assert_eq!(list.len(), 2);
        assert_eq!(default, 1);
        assert_eq!(list[1].id, OURS);
        assert_eq!(
            list[1].cmd,
            vec!["env", "X=1", "dbus-run-session", "--", "edel-compositor"]
        );
    }

    #[test]
    fn a_machine_with_no_desktop_files_offers_only_the_cmd() {
        let (list, default) = with_default(Vec::new(), "/usr/libexec/edel-session");
        assert_eq!(default, 0);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].cmd, vec!["/usr/libexec/edel-session"]);
        assert_eq!(list[0].id, "edel-session");
    }

    #[test]
    fn remembered_reads_a_persons_line() {
        let text = "# A comment.\n[people]\nci = \"ci-other\"\nmira = \"edel\"\n";
        assert_eq!(remembered(text, "ci").as_deref(), Some("ci-other"));
        assert_eq!(remembered(text, "mira").as_deref(), Some("edel"));
        assert_eq!(remembered(text, "nobody"), None);
    }

    #[test]
    fn a_missing_or_broken_file_remembers_nothing() {
        assert_eq!(remembered("", "ci"), None);
        assert_eq!(remembered("[people\nci = ", "ci"), None);
        assert_eq!(remembered("people = 3\n", "ci"), None);
        assert_eq!(remembered("[people]\nci = 3\n", "ci"), None);
    }

    #[test]
    fn remember_then_forget_keeps_the_other_people() {
        let one = remember("", "ci", Some("ci-other"));
        assert!(one.starts_with("# Each person's desktop"));
        assert_eq!(remembered(&one, "ci").as_deref(), Some("ci-other"));
        let two = remember(&one, "mira", Some("edel"));
        assert_eq!(remembered(&two, "ci").as_deref(), Some("ci-other"));
        assert_eq!(remembered(&two, "mira").as_deref(), Some("edel"));
        let forgot = remember(&two, "ci", None);
        assert_eq!(remembered(&forgot, "ci"), None);
        assert_eq!(remembered(&forgot, "mira").as_deref(), Some("edel"));
        let last = remember(&forgot, "mira", None);
        assert!(!last.contains("people"), "{last}");
        assert_eq!(remember(&one, "ci", None).lines().count(), 1);
    }

    #[test]
    fn remember_starts_a_file_that_does_not_read_afresh() {
        let text = remember("[people\nbroken", "ci", Some("ci-other"));
        assert_eq!(remembered(&text, "ci").as_deref(), Some("ci-other"));
    }
}
