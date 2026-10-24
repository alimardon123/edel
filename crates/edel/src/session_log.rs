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

/// The frame telemetry's numbers from one line the compositor logged:
/// frames, render p50 and p99 in milliseconds, and idle frames (M8.1). The
/// writer is `edel-compositor`'s telemetry `Summary`, whose `Display` prints
/// `frames N, render p50 X ms, p99 Y ms, idle frames Z`; the line may carry
/// the `edel-compositor: ` prefix or be the summary itself. The rest must
/// parse exactly, so any other line gives None.
pub fn frame_summary(line: &str) -> Option<(u64, f64, f64, u64)> {
    let line = line.trim_end();
    let text = line.strip_prefix("edel-compositor: ").unwrap_or(line);
    let rest = text.strip_prefix("frames ")?;
    let (frames, rest) = rest.split_once(", render p50 ")?;
    let (p50, rest) = rest.split_once(" ms, p99 ")?;
    let (p99, idle) = rest.split_once(" ms, idle frames ")?;
    let ms = |text: &str| text.parse::<f64>().ok().filter(|v| v.is_finite());
    Some((frames.parse().ok()?, ms(p50)?, ms(p99)?, idle.parse().ok()?))
}

/// One person's session logs.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub name: String,
    pub dir: PathBuf,
    /// Who owns the person's home: their logs must be theirs, so a link or
    /// a hard link they planted to another's file is never read (M6.17).
    /// None when the home cannot be read, and then no log is read.
    pub owner: Option<u32>,
}

/// The owner of the home `dir` (a person's state folder) lies in: the
/// folder [`places::STATE_IN_HOME`] above it, not followed if a link.
fn home_owner(dir: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let depth = Path::new(places::STATE_IN_HOME).components().count();
    let home = dir.ancestors().nth(depth)?;
    let meta = fs::symlink_metadata(home).ok()?;
    meta.is_dir().then(|| meta.uid())
}

/// `person`'s log `name` in their state folder, read safely as root (M6.17,
/// the review of 2026-10-13): only a regular file, never through a link,
/// the file opened the same one looked at, and owned by the person, so a
/// link or a hard link to another's file (`/etc/shadow`) is not read.
pub fn read_log(person: &Person, name: &str) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let owner = person.owner?;
    let path = person.dir.join(name);
    let seen = fs::symlink_metadata(&path).ok()?;
    if !seen.file_type().is_file() {
        return None;
    }
    let mut file = File::open(&path).ok()?;
    let opened = file.metadata().ok()?;
    if (opened.dev(), opened.ino()) != (seen.dev(), seen.ino()) || opened.uid() != owner {
        return None;
    }
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    Some(text)
}

/// The people whose logs a command may read: the one running it, and,
/// for root, everyone with a home under `homes` that holds a log.
pub fn people(homes: &Path, root: bool) -> Vec<Person> {
    let mut found = Vec::new();
    if let Some(dir) = places::person_state_dir() {
        let name = std::env::var("USER").unwrap_or_else(|_| "you".into());
        // The one running it: their own files, wherever XDG_STATE_HOME
        // puts the folder; it reads nothing they could not read anyway.
        let owner = crate::users::current();
        found.push(Person { name, dir, owner });
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
                let owner = home_owner(&dir);
                found.push(Person { name, dir, owner });
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
    let text = read_log(person, name)?;
    (!ended(&text)).then(|| person.dir.join(name))
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

    #[test]
    fn a_log_read_as_root_is_never_a_link_nor_anothers_file() {
        // M6.17: a person could plant a link (or a hard link) named
        // session.log to /etc/shadow, which `edel report` run as root
        // would read and print.
        use std::os::unix::fs::{MetadataExt, symlink};
        let home = std::env::temp_dir().join(format!("edel-log-links-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let dir = home.join(places::STATE_IN_HOME);
        fs::create_dir_all(&dir).unwrap();
        let me = fs::metadata(&home).unwrap().uid();
        assert_eq!(home_owner(&dir), Some(me));
        let person = |owner| Person {
            name: "ci".into(),
            dir: dir.clone(),
            owner,
        };
        let log = dir.join(places::SESSION_LOG);
        fs::write(&log, "ready\n").unwrap();
        assert_eq!(
            read_log(&person(Some(me)), places::SESSION_LOG).as_deref(),
            Some("ready\n")
        );
        // Owned by another than the home's owner: not read.
        assert_eq!(read_log(&person(Some(me + 1)), places::SESSION_LOG), None);
        // No owner known: not read.
        assert_eq!(read_log(&person(None), places::SESSION_LOG), None);
        // A link to a secret: not followed.
        let secret = home.join("secret");
        fs::write(&secret, "root:$6$hash\n").unwrap();
        fs::remove_file(&log).unwrap();
        symlink(&secret, &log).unwrap();
        assert_eq!(read_log(&person(Some(me)), places::SESSION_LOG), None);
        assert_eq!(ended_badly(&person(Some(me)), false), None);
        // A folder in its place: not read.
        fs::remove_file(&log).unwrap();
        fs::create_dir(&log).unwrap();
        assert_eq!(read_log(&person(Some(me)), places::SESSION_LOG), None);
        // A home that is itself a link has no owner to trust.
        let linked = std::env::temp_dir().join(format!("edel-log-linked-{}", std::process::id()));
        let _ = fs::remove_file(&linked);
        symlink(&home, &linked).unwrap();
        assert_eq!(home_owner(&linked.join(places::STATE_IN_HOME)), None);
        fs::remove_file(&linked).unwrap();
        fs::remove_dir_all(&home).unwrap();
    }

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
            owner: Some(std::os::unix::fs::MetadataExt::uid(
                &fs::metadata(&dir).unwrap(),
            )),
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
    fn a_frame_line_gives_its_numbers_and_nothing_else() {
        assert_eq!(
            frame_summary(
                "edel-compositor: frames 300, render p50 1.20 ms, p99 3.40 ms, idle frames 0"
            ),
            Some((300, 1.2, 3.4, 0))
        );
        assert_eq!(
            frame_summary("frames 6, render p50 3.00 ms, p99 9.00 ms, idle frames 1\n"),
            Some((6, 3.0, 9.0, 1))
        );
        assert_eq!(
            frame_summary("edel-compositor: output Virtual-1 ready"),
            None
        );
        assert_eq!(
            frame_summary("edel-compositor: frames 3, render p50 x ms, p99 1.00 ms, idle frames 0"),
            None
        );
        assert_eq!(
            frame_summary(
                "edel-compositor: frames 3, render p50 1.00 ms, p99 1.00 ms, idle frames 0 extra"
            ),
            None
        );
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
