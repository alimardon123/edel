//! Running `edel` for the Updates and System pages (M5.8c): the page does
//! what the command line does, by running the command, so a button and
//! `edel update --check` are one level (ADR-008) and a failure reads the
//! same in both, in `edel`'s own words (`docs/MESSAGES.md`).
//!
//! The commands that change the machine need root until `doas` arrives
//! (M6.5, which will make this module run them through it). Until then
//! a person's click on one reads `edel`'s own refusal, as it is.

use std::process::{Command, Stdio};

use edel::i18n::trf;

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

/// Restarts the machine, which is `reboot` (there is no `edel` command for
/// it); the same call on a page's Restart now and a person's own hand.
/// Like the commands that change the machine, it needs root until `doas`
/// (M6.5), and its refusal is shown as it is.
pub fn reboot() -> Outcome {
    run("reboot", &[])
}

/// Whether Settings runs as root, which only a test or an appliance does:
/// the Updates page words a refusal differently for a person, who cannot
/// install until `doas` (M6.5).
pub fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
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
    fn the_command_line_is_edel_and_its_words() {
        assert_eq!(line(&["update", "--check"]), "edel update --check");
        assert_eq!(line(&["rollback"]), "edel rollback");
    }
}
