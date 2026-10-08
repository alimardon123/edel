//! Running another program for a person's page (roadmap M5.8a): the one
//! place `edel::network` and `edel::bluetooth` start `nmcli` and
//! `bluetoothctl`, so both wait the same way and neither can hang a page.
//! A program that does not answer within its time is killed, because
//! `bluetoothctl` waits for ever for a `bluetoothd` that is not running.
//! Std only.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a finished program left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Done {
    /// Its exit status; none when it was killed, by its time or a signal.
    pub status: Option<i32>,
    /// Whether its time ran out and it was killed.
    pub timed_out: bool,
    /// What it printed on standard output.
    pub out: String,
    /// What it printed on standard error.
    pub err: String,
}

impl Done {
    /// Whether it exited with status 0 in time.
    pub fn ok(&self) -> bool {
        self.status == Some(0) && !self.timed_out
    }
}

/// Runs `program` with `args` and no input, waiting at most `time`; the
/// error is the system's, such as `NotFound` for a program that is not
/// installed.
pub fn run(program: &str, args: &[String], time: Duration) -> std::io::Result<Done> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Read both pipes while it runs, so a chatty program never fills one
    // and waits for us.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let mine = kept.clone();
        let thread = std::thread::spawn(move || {
            let Some(mut pipe) = pipe else { return };
            let mut chunk = [0u8; 4096];
            while let Ok(n @ 1..) = pipe.read(&mut chunk) {
                if let Ok(mut bytes) = mine.lock() {
                    bytes.extend_from_slice(&chunk[..n]);
                }
            }
        });
        (kept, thread)
    };
    let (out, out_thread) = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let (err, err_thread) = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait()? {
            Some(status) => break (status.code(), false),
            None if started.elapsed() >= time => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // The pipes close with the program. A child it started may hold one
    // open after the program is gone (a killed `sh` and its `sleep`), so
    // wait a moment for the rest and then keep what came.
    let gave_up = Instant::now() + Duration::from_millis(300);
    while !(out_thread.is_finished() && err_thread.is_finished()) && Instant::now() < gave_up {
        std::thread::sleep(Duration::from_millis(5));
    }
    let text = |kept: &Mutex<Vec<u8>>| {
        kept.lock()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    Ok(Done {
        status,
        timed_out,
        out: text(&out),
        err: text(&err),
    })
}

/// `program` and `args` as one line to type, each word quoted when the
/// shell would split or change it: what Copy as command shows.
pub fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A word as the shell takes it whole: unchanged when it holds only
/// letters, digits and `_@%+=:,./-`, else in single quotes.
fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, time: Duration) -> Done {
        run("sh", &["-c".into(), script.into()], time).unwrap()
    }

    #[test]
    fn what_a_program_printed_and_how_it_ended_come_back() {
        let done = sh("echo out; echo trouble >&2; exit 3", Duration::from_secs(5));
        assert_eq!(done.status, Some(3));
        assert!(!done.timed_out && !done.ok());
        assert_eq!(done.out, "out\n");
        assert_eq!(done.err, "trouble\n");
        assert!(sh("true", Duration::from_secs(5)).ok());
    }

    #[test]
    fn a_program_that_does_not_answer_in_time_is_killed() {
        let started = Instant::now();
        let done = sh("echo before; sleep 30", Duration::from_millis(300));
        assert!(done.timed_out && !done.ok());
        assert_eq!(done.status, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(done.out, "before\n");
    }

    #[test]
    fn a_program_that_is_not_there_is_the_systems_error() {
        let error = run("edel-no-such-program", &[], Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn a_lot_of_output_does_not_stall_the_program() {
        let done = sh(
            "head -c 300000 /dev/zero | tr '\\0' x",
            Duration::from_secs(10),
        );
        assert!(done.ok());
        assert_eq!(done.out.len(), 300_000);
    }

    #[test]
    fn a_command_line_quotes_what_the_shell_would_split() {
        let args: Vec<String> = ["device", "wifi", "connect", "Café Wi-Fi", "it's"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            command_line("nmcli", &args),
            "nmcli device wifi connect 'Café Wi-Fi' 'it'\\''s'"
        );
        assert_eq!(command_line("nmcli", &["".into()]), "nmcli ''");
    }
}
