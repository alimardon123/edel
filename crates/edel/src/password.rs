//! The password check every part of Edel OS asks with (roadmap M6.5, moved
//! here from shell-ui's lock screen, ADR-010): the lock and login screens,
//! and `edel`, which asks the administrator's password before what can break
//! the machine. A password is typed into a [`Password`], which wipes its
//! bytes when it is dropped or cleared. [`check`] asks the system's own
//! helper, Linux-PAM's `unix_chkpwd`, whether it is the person's, so no part
//! reads `/etc/shadow` itself. It uses only std and `places`, so the desktop
//! parts take it without the `cli` feature.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The most bytes a password may hold: long enough for any passphrase, short
/// enough to bound memory.
pub const MAX_BYTES: usize = 1024;

/// A password being typed: its bytes, wiped when it is dropped or cleared.
#[derive(Default)]
pub struct Password(Vec<u8>);

impl Password {
    /// A password of `bytes`, the first [`MAX_BYTES`] of them. The caller
    /// wipes its own copy of `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Password {
        Password(bytes[..bytes.len().min(MAX_BYTES)].to_vec())
    }

    /// Adds `text` at the end, unless that would pass [`MAX_BYTES`].
    pub fn push(&mut self, text: &str) {
        if self.0.len() + text.len() <= MAX_BYTES {
            self.0.extend_from_slice(text.as_bytes());
        }
    }

    /// Takes the last letter back.
    pub fn pop(&mut self) {
        let Ok(text) = std::str::from_utf8(&self.0) else {
            return self.clear();
        };
        let keep = text.char_indices().last().map_or(0, |(i, _)| i);
        self.0[keep..].fill(0);
        self.0.truncate(keep);
    }

    /// Wipes the password and empties it.
    pub fn clear(&mut self) {
        self.0.fill(0);
        self.0.clear();
    }

    /// How many letters it holds.
    pub fn letters(&self) -> usize {
        std::str::from_utf8(&self.0).map_or(0, |t| t.chars().count())
    }

    /// The password as text, for greetd's message (M5.10b).
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Takes the bytes out, leaving it empty.
    pub fn take(&mut self) -> Password {
        Password(std::mem::take(&mut self.0))
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        self.clear();
    }
}

/// What the helper said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Right,
    Wrong,
    /// No helper could run: why.
    NoChecker(String),
}

/// Where the password helper may lie, the first that exists: Alpine puts
/// `unix_chkpwd` in `/sbin`, other systems in `/usr/sbin`.
pub fn checker() -> Option<PathBuf> {
    crate::places::PASSWORD_CHECKERS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// How long a wrong password waits before the next try.
pub const WRONG_WAIT: Duration = Duration::from_secs(2);

/// Asks `helper` whether `password` is `user`'s: it reads the password,
/// ended by a NUL byte, on its standard input and exits 0 when it is
/// right. Runs on a thread of its own, never the drawing one; a wrong
/// answer comes back after `WRONG_WAIT`.
pub fn check(helper: &Path, user: &str, password: Password) -> Checked {
    let child = Command::new(helper)
        .arg(user)
        .arg("nonull")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Checked::NoChecker(format!("{}: {e}", helper.display())),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&password.0);
        let _ = stdin.write_all(&[0]);
    }
    drop(password);
    match child.wait() {
        Ok(status) if status.success() => Checked::Right,
        Ok(_) => {
            std::thread::sleep(WRONG_WAIT);
            Checked::Wrong
        }
        Err(e) => Checked::NoChecker(format!("{}: {e}", helper.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_counts_letters_takes_one_back_and_wipes_itself() {
        let mut p = Password::default();
        p.push("pä");
        p.push("ss");
        assert_eq!(p.letters(), 4);
        p.pop();
        assert_eq!(p.letters(), 3);
        assert_eq!(p.0, "päs".as_bytes());
        let taken = p.take();
        assert_eq!(p.letters(), 0);
        assert_eq!(taken.letters(), 3);
        p.push("x");
        p.clear();
        assert!(p.0.is_empty());
    }

    #[test]
    fn a_password_is_capped_at_1024_bytes() {
        let mut p = Password::default();
        p.push(&"a".repeat(1000));
        p.push(&"b".repeat(100));
        assert_eq!(p.0.len(), 1000, "a push past the cap adds nothing");
        assert_eq!(Password::from_bytes(&[b'x'; 1500]).0.len(), MAX_BYTES);
        assert_eq!(Password::from_bytes(b"open").text(), Some("open"));
    }

    #[test]
    fn a_missing_helper_is_said_and_never_unlocks() {
        let got = check(
            Path::new("/nonexistent/unix_chkpwd"),
            "ci",
            Password::default(),
        );
        assert!(matches!(got, Checked::NoChecker(_)), "{got:?}");
    }

    #[test]
    fn the_helper_answers_right_by_its_exit_and_reads_the_password() {
        let dir = std::env::temp_dir().join(format!("edel-password-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("chk");
        // Right when the bytes before the NUL are "open".
        std::fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = ci ] && [ \"$2\" = nonull ] || exit 9\n[ \"$(tr -d '\\000')\" = open ]\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut right = Password::default();
        right.push("open");
        assert_eq!(check(&helper, "ci", right), Checked::Right);
        let mut wrong = Password::default();
        wrong.push("shut");
        let started = std::time::Instant::now();
        assert_eq!(check(&helper, "ci", wrong), Checked::Wrong);
        assert!(started.elapsed() >= WRONG_WAIT);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
