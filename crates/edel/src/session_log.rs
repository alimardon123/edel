//! The desktop session's log (roadmap M5.28a): one per person, where
//! people and `edel report` find what the compositor, shell-ui and the apps
//! they start wrote. The compositor opens it at the start of a session,
//! after moving the one before aside, so the log holds two sessions and
//! stays small; the last line of a session that closed as it should is
//! [`ENDED`], so one that lacks it ended badly. No daemon of our own:
//! busybox's syslogd keeps the system's log ([`places::SYSTEM_LOG`]).

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::places;

/// The line a session that closed as it should ends its log with.
pub const ENDED: &str = "edel-compositor: the session ended";

/// How many of a log's last lines `edel report` gathers.
pub const LAST_LINES: usize = 60;

/// A new session's log in `dir`, the one before moved to
/// [`places::SESSION_LOG_BEFORE`]: the log's path, the file to write to,
/// and whether the session before ended badly (its log lacks [`ENDED`]).
pub fn start(dir: &Path) -> io::Result<(PathBuf, File, bool)> {
    fs::create_dir_all(dir)?;
    let path = dir.join(places::SESSION_LOG);
    let before = dir.join(places::SESSION_LOG_BEFORE);
    let mut badly = false;
    if let Ok(text) = fs::read_to_string(&path) {
        badly = !ended(&text);
        fs::rename(&path, &before)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((path, file, badly))
}

/// Whether a log's text shows its session closed as it should.
pub fn ended(text: &str) -> bool {
    text.lines().any(|line| line.trim_end() == ENDED)
}

/// The last `n` lines of `text`, each ending in a newline.
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for line in &lines[lines.len().saturating_sub(n)..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One person's session logs.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub name: String,
    pub dir: PathBuf,
}

/// The people whose logs a command may read: the one running it, and,
/// for root, everyone with a home under `homes` that holds a log.
pub fn people(homes: &Path, root: bool) -> Vec<Person> {
    let mut found = Vec::new();
    if let Some(dir) = places::person_state_dir() {
        let name = std::env::var("USER").unwrap_or_else(|_| "you".into());
        found.push(Person { name, dir });
    }
    if root {
        let mut homes: Vec<PathBuf> = fs::read_dir(homes)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        homes.sort();
        for home in homes {
            let dir = home.join(places::STATE_IN_HOME);
            let name = home
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if dir.join(places::SESSION_LOG).exists() && !found.iter().any(|p| p.dir == dir) {
                found.push(Person { name, dir });
            }
        }
    }
    found
}

/// The log of `person`'s last session that is over, if it ended badly:
/// the session before's while one runs now (`running`), else the last.
pub fn ended_badly(person: &Person, running: bool) -> Option<PathBuf> {
    let name = if running {
        places::SESSION_LOG_BEFORE
    } else {
        places::SESSION_LOG
    };
    let path = person.dir.join(name);
    let text = fs::read_to_string(&path).ok()?;
    (!ended(&text)).then_some(path)
}

/// Whether a process named `name` runs as the owner of `dir`, read from
/// `/proc`: whether that person's session runs now.
pub fn runs_as_owner_of(proc: &Path, name: &str, dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(owner) = fs::metadata(dir).map(|m| m.uid()) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(proc) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        let comm = fs::read_to_string(path.join("comm")).unwrap_or_default();
        comm.trim_end() == name
            && fs::read_to_string(path.join("status"))
                .unwrap_or_default()
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next()?.parse::<u32>().ok())
                == Some(owner)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_new_session_keeps_the_one_before_and_says_if_it_ended_badly() {
        let dir = dir("start");
        let (path, _, badly) = start(&dir).unwrap();
        assert!(!badly, "there was no session before");
        fs::write(&path, format!("edel-compositor: output ready\n{ENDED}\n")).unwrap();
        let (_, _, badly) = start(&dir).unwrap();
        assert!(!badly);
        let kept = fs::read_to_string(dir.join(places::SESSION_LOG_BEFORE)).unwrap();
        assert!(ended(&kept));
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        // A session killed half way leaves no last line.
        fs::write(&path, "edel-compositor: output ready\n").unwrap();
        let (_, _, badly) = start(&dir).unwrap();
        assert!(badly);
        let person = Person {
            name: "ci".into(),
            dir: dir.clone(),
        };
        assert_eq!(
            ended_badly(&person, true),
            Some(dir.join(places::SESSION_LOG_BEFORE))
        );
        // The new log has no last line yet, but its session still runs.
        assert_eq!(ended_badly(&person, false), Some(path.clone()));
        fs::write(&path, format!("{ENDED}\n")).unwrap();
        assert_eq!(ended_badly(&person, false), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_last_lines_are_kept_in_order() {
        assert_eq!(tail("a\nb\nc\n", 2), "b\nc\n");
        assert_eq!(tail("a\nb", 5), "a\nb\n");
        assert_eq!(tail("", 3), "");
    }

    #[test]
    fn root_reads_the_logs_of_everyone_with_one() {
        let homes = dir("homes");
        for name in ["ana", "ci", "nobody"] {
            fs::create_dir_all(homes.join(name).join(places::STATE_IN_HOME)).unwrap();
        }
        for name in ["ana", "ci"] {
            let log = homes
                .join(name)
                .join(places::STATE_IN_HOME)
                .join(places::SESSION_LOG);
            fs::write(log, "x\n").unwrap();
        }
        let names = |root| -> Vec<String> {
            people(&homes, root)
                .into_iter()
                .filter(|p| p.dir.starts_with(&homes))
                .map(|p| p.name)
                .collect()
        };
        assert_eq!(names(true), ["ana", "ci"]);
        assert!(names(false).is_empty());
        fs::remove_dir_all(&homes).unwrap();
    }

    #[test]
    fn a_process_is_found_by_name_and_owner() {
        // This test's own process, as /proc shows it.
        let proc = Path::new("/proc");
        let me = fs::read_to_string("/proc/self/comm").unwrap();
        let owned = dir("owner");
        fs::create_dir_all(&owned).unwrap();
        assert!(runs_as_owner_of(proc, me.trim_end(), &owned));
        assert!(!runs_as_owner_of(proc, "no-such-program", &owned));
        fs::remove_dir_all(&owned).unwrap();
    }
}
