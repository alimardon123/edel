//! Running `edel` for the Updates and System pages (M5.8c): the page does
//! what the command line does, by running the command, so a button and
//! `edel update --check` are one level (ADR-008) and a failure reads the
//! same in both, in `edel`'s own words (`docs/MESSAGES.md`).
//!
//! The commands that change the machine run as root through `doas`
//! (M6.5): `edel_as_admin` gives the administrator's password to
//! `edel --password-stdin`, which checks it before it changes anything.
//! The page asks for that password in its own dialog (`ask.rs`).

use std::io::Write;
use std::process::{Command, Stdio};

use edel::i18n::trf;

use crate::actions;

/// What a run of `edel` gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Whether it exited with status 0.
    pub ok: bool,
    /// Its exit status, none when it did not start or was killed.
    pub code: Option<i32>,
    /// What it printed on standard output, without the final newline.
    pub out: String,
    /// What it printed on standard error, without the final newline.
    pub err: String,
}

impl Outcome {
    /// What to show a person: the output on success, else the message,
    /// else whatever was printed.
    pub fn shown(&self) -> String {
        let (first, second) = if self.ok {
            (&self.out, &self.err)
        } else {
            (&self.err, &self.out)
        };
        if first.is_empty() { second } else { first }.clone()
    }
}

/// Runs `edel` with `args`, waiting for it; call it from a thread of its
/// own (`gio::spawn_blocking`), never from the one that draws.
pub fn edel(args: &[&str]) -> Outcome {
    run("edel", args)
}

/// Restarts the machine: `edel restart` (M5.10c), the same command the lock
/// screen's power menu runs. The same call on a page's Restart now and a
/// person's own hand. It goes through doas when the person is not root (M6.5),
/// and its refusal is shown as it is.
pub fn restart() -> Outcome {
    edel(actions::RESTART.args)
}

/// Whether Settings runs as root, which only a test or an appliance does;
/// a person's Settings runs the commands through doas instead (M6.5).
pub fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

/// Runs `doas -n edel ARGS --password-stdin` (M6.5): doas lets the
/// administrators run `edel` as root, and `edel` checks `password` before
/// it changes what can break the machine. The password goes in on one line,
/// the pipe is closed after it, and the copy held here is cleared. Call it
/// from a thread of its own.
pub fn edel_as_admin(args: &[&str], password: String) -> Outcome {
    let mut all = vec!["-n", "edel"];
    all.extend_from_slice(args);
    all.push("--password-stdin");
    with_password(edel::places::AS_ROOT, &all, password)
}

/// Runs `doas -n edel ARGS` (M6.6a), or `edel ARGS` when Settings itself
/// runs as root, giving `input` and a newline on standard input when there
/// is some, as the installer page hands the new person's password to
/// `edel install --new-password-stdin`. On a stick doas lets its own
/// account run `edel` with no password, and `edel` decides what it may do
/// (`privilege.rs`). Call it from a thread of its own.
pub fn edel_as_root(args: &[&str], input: Option<String>) -> Outcome {
    let mut all = Vec::new();
    let program = if is_root() {
        "edel"
    } else {
        all.extend_from_slice(&["-n", "edel"]);
        edel::places::AS_ROOT
    };
    all.extend_from_slice(args);
    match input {
        Some(input) => with_password(program, &all, input),
        None => run(program, &all),
    }
}

/// Changes a machine key as root, then applies the settings file when that
/// worked, so the desktop follows (`edel settings apply`): `words` run
/// through `edel_as_root`, and the second outcome is none when the change
/// was refused. The Region and Location pages call it on a thread of their
/// own, and show a refusal in the row's line.
pub fn set_and_apply_as_root(words: Vec<String>) -> (Outcome, Option<Outcome>) {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let set = edel_as_root(&args, None);
    let apply = set.ok.then(|| edel_as_root(actions::APPLY.args, None));
    (set, apply)
}

/// Runs `program` with `args`, writing `password` and a newline to its
/// standard input and then closing it, and waits for it.
fn with_password(program: &str, args: &[&str], mut password: String) -> Outcome {
    let started = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match started {
        Ok(child) => child,
        Err(why) => {
            password.clear();
            return Outcome {
                ok: false,
                code: None,
                out: String::new(),
                err: trf(
                    "could not run {program}: {why}; its rule for edel comes with the machine feature",
                    &[("program", program), ("why", &why.to_string())],
                ),
            };
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(password.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    password.clear();
    match child.wait_with_output() {
        Ok(output) => Outcome {
            ok: output.status.success(),
            code: output.status.code(),
            out: text(&output.stdout),
            err: text(&output.stderr),
        },
        Err(why) => Outcome {
            ok: false,
            code: None,
            out: String::new(),
            err: trf(
                "could not finish {program}: {why}",
                &[("program", program), ("why", &why.to_string())],
            ),
        },
    }
}

fn run(program: &str, args: &[&str]) -> Outcome {
    let started = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output();
    match started {
        Ok(output) => Outcome {
            ok: output.status.success(),
            code: output.status.code(),
            out: text(&output.stdout),
            err: text(&output.stderr),
        },
        Err(why) => Outcome {
            ok: false,
            code: None,
            out: String::new(),
            err: trf(
                "could not run edel: {why}; the edel tool should be in /usr/bin",
                &[("why", &why.to_string())],
            ),
        },
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_end().to_string()
}

/// The command line that does what `edel(args)` does, for Copy as command.
pub fn line(args: &[&str]) -> String {
    let words: Vec<&str> = std::iter::once("edel")
        .chain(args.iter().copied())
        .collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(ok: bool, out: &str, err: &str) -> Outcome {
        Outcome {
            ok,
            code: Some(i32::from(!ok)),
            out: out.into(),
            err: err.into(),
        }
    }

    #[test]
    fn a_success_shows_its_output_and_a_failure_its_message() {
        assert_eq!(outcome(true, "running: 1", "").shown(), "running: 1");
        assert_eq!(outcome(true, "", "warning: x").shown(), "warning: x");
        assert_eq!(
            outcome(false, "", "edel update: refused").shown(),
            "edel update: refused"
        );
        assert_eq!(outcome(false, "change: x", "").shown(), "change: x");
        assert_eq!(outcome(false, "", "").shown(), "");
    }

    #[test]
    fn a_program_that_does_not_start_says_so() {
        let gone = run("edel-no-such-program", &["status"]);
        assert!(!gone.ok && gone.code.is_none() && gone.out.is_empty());
        assert!(gone.err.starts_with("could not run edel: "), "{}", gone.err);
        let status = run("sh", &["-c", "echo hello; echo trouble >&2; exit 3"]);
        assert_eq!(
            (
                status.ok,
                status.code,
                status.out.as_str(),
                status.err.as_str()
            ),
            (false, Some(3), "hello", "trouble")
        );
    }

    #[test]
    fn the_password_goes_in_on_one_line_and_the_program_sees_it() {
        let got = with_password(
            "sh",
            &["-c", "read line && echo \"got $line\" && exit 3", "sh"],
            "open".into(),
        );
        assert_eq!(
            (got.ok, got.code, got.out.as_str()),
            (false, Some(3), "got open")
        );
        let missing = with_password("edel-no-such-doas", &[], "open".into());
        assert!(!missing.ok && missing.err.starts_with("could not run edel-no-such-doas"));
    }

    #[test]
    fn the_command_line_is_edel_and_its_words() {
        assert_eq!(line(&["update", "--check"]), "edel update --check");
        assert_eq!(line(&["rollback"]), "edel rollback");
    }
}
