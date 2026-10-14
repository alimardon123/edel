//! The first start (roadmap M6.12): when the desktop opens Settings'
//! Welcome. `edel boot live` logs the account `live` in by itself on a USB
//! stick and on a computer nobody has an account on (live.rs), and that
//! session opens the Welcome, which the compositor starts (`due`). On a
//! stick the Welcome leaves the stamp `places::WELCOME_SEEN` when it
//! closes, so it opens once there; on a computer nobody has an account on
//! it leaves none, so it opens at every start until it has made a person,
//! after which live no longer logs in.

use std::fs;
use std::io;
use std::path::Path;

use crate::places;

/// The login name of `uid` in `passwd`, the text of `/etc/passwd`: field 0
/// of the first line whose field 2 is `uid`. Lines that do not parse are
/// skipped.
pub fn name_of(passwd: &str, uid: u32) -> Option<&str> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let id = fields.nth(1)?;
        (id.parse::<u32>().ok()? == uid).then_some(name)
    })
}

/// Whether the Welcome opens now: the session is `uid`, the account
/// `places::LIVE_USER`, and the Welcome has not been seen (`seen`).
pub fn due(passwd: &str, uid: u32, seen: bool) -> bool {
    !seen && name_of(passwd, uid) == Some(places::LIVE_USER)
}

/// Whether the Welcome's stamp is in `dir`, a person's state directory.
pub fn seen_in(dir: &Path) -> bool {
    dir.join(places::WELCOME_SEEN).exists()
}

/// Whether the Welcome's stamp is in the running person's state directory.
/// False when there is none, so the Welcome opens.
pub fn seen() -> bool {
    places::person_state_dir().is_some_and(|dir| seen_in(&dir))
}

/// Leaves the Welcome's stamp in `dir`, making the folder when it is not
/// there yet.
pub fn mark_seen_in(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(places::WELCOME_SEEN), "")
}

/// Leaves the Welcome's stamp in the running person's state directory.
/// Fails when neither `XDG_STATE_HOME` nor `HOME` is set.
pub fn mark_seen() -> io::Result<()> {
    let dir = places::person_state_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no state directory: neither XDG_STATE_HOME nor HOME is set",
        )
    })?;
    mark_seen_in(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\n\
live:x:100:101::/home/live:/bin/bash\n\
ali:x:1000:1000::/home/ali:/bin/sh\n";

    #[test]
    fn finds_the_name_of_a_uid_in_passwd() {
        assert_eq!(name_of(PASSWD, 100), Some("live"));
        assert_eq!(name_of(PASSWD, 4242), None);
    }

    #[test]
    fn skips_lines_that_do_not_parse_when_finding_a_name() {
        let text = "broken line\nlive:x:notanumber:0::/:/bin/sh\nlive:x:100:101::/:/bin/sh\n";
        assert_eq!(name_of(text, 100), Some("live"));
    }

    #[test]
    fn opens_the_welcome_for_live_until_it_is_seen() {
        assert!(due(PASSWD, 100, false));
        assert!(!due(PASSWD, 100, true));
    }

    #[test]
    fn never_opens_the_welcome_for_anyone_else() {
        assert!(!due(PASSWD, 1000, false));
        assert!(!due(PASSWD, 0, false));
    }

    #[test]
    fn marking_the_welcome_seen_makes_the_folder_and_the_stamp() {
        let root =
            std::env::temp_dir().join(format!("edel-first-start-marks-{}", std::process::id()));
        let dir = root.join("state").join("edel");
        assert!(!seen_in(&dir));
        mark_seen_in(&dir).unwrap();
        assert!(seen_in(&dir));
        fs::remove_dir_all(&root).unwrap();
    }
}
