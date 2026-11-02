//! The exit statuses `edel` ends with besides 0 (done) and 1 (failed), so
//! Settings tells apart the causes people meet without reading words that
//! are translated (the M8.15 messages audit's row 44). The numbers are
//! sysexits.h's, which other tools use for the same causes.

use std::fmt;

/// The command refused because the person may not change the system
/// (sysexits.h's EX_NOPERM).
pub const REFUSED: u8 = 77;
/// A download failed, so the network or the server could not be reached
/// (sysexits.h's EX_UNAVAILABLE).
pub const OFFLINE: u8 = 69;
/// The password typed for doas, or the check of the privilege, was wrong, so
/// nothing changed and Settings asks again (sysexits.h's EX_NOUSER).
pub const WRONG_PASSWORD: u8 = 67;
/// The reader of `edel`'s output went away first, as `grep -q` or `head`
/// does once it has seen enough: what a shell reports for a program a
/// closed pipe stops (128 and SIGPIPE's 13).
pub const CLOSED_PIPE: u8 = 141;

/// Ends `edel` quietly with [`CLOSED_PIPE`] when its output's reader has
/// gone, in place of the panic Rust's `println!` raises there ("failed
/// printing to stdout: Broken pipe"). A hook rather than SIGPIPE's default,
/// so a child that closes its stdin early still comes back as an error to
/// the command writing to it, never as `edel` killed halfway through.
pub fn quiet_on_closed_pipe() {
    let others = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let words = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        if words.is_some_and(closed_pipe) {
            std::process::exit(i32::from(CLOSED_PIPE));
        }
        others(info);
    }));
}

/// Whether a panic's words are the print macros' for a reader gone away.
fn closed_pipe(words: &str) -> bool {
    words.starts_with("failed printing to std") && words.contains("Broken pipe")
}

/// A failure that ends `edel` with `code`; its words are `message`.
/// Used as an error or as an error's context, so `main` finds it anywhere
/// in the chain.
#[derive(Debug)]
pub struct Coded {
    pub code: u8,
    pub message: String,
}

impl Coded {
    /// A failure that ends `edel` with `code`, saying `message`.
    pub fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Coded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Coded {}

/// The exit status for `err`: a [`Coded`] anywhere in its chain gives its
/// code, else 1.
pub fn code_of(err: &anyhow::Error) -> u8 {
    err.downcast_ref::<Coded>().map_or(1, |c| c.code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, anyhow};

    #[test]
    fn a_coded_context_or_error_gives_its_code_and_others_give_one() {
        let offline = Err::<(), _>(std::io::Error::other("x"))
            .with_context(|| Coded::new(OFFLINE, "could not download"))
            .unwrap_err();
        assert_eq!(code_of(&offline), 69);
        assert_eq!(format!("{offline:#}"), "could not download: x");
        assert_eq!(code_of(&anyhow!("x")), 1);
        let refused = anyhow::Error::new(Coded::new(REFUSED, "no")).context("outer");
        assert_eq!(code_of(&refused), 77);
        assert_eq!(format!("{refused:#}"), "outer: no");
        let wrong = anyhow::Error::new(Coded::new(WRONG_PASSWORD, "wrong password for ci"));
        assert_eq!(code_of(&wrong), 67);
        assert_eq!(format!("{wrong:#}"), "wrong password for ci");
    }

    #[test]
    fn only_a_print_to_a_closed_pipe_ends_quietly() {
        assert!(closed_pipe(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(closed_pipe(
            "failed printing to stderr: Broken pipe (os error 32)"
        ));
        assert!(!closed_pipe(
            "failed printing to stdout: No space left on device (os error 28)"
        ));
        assert!(!closed_pipe("called `Option::unwrap()` on a `None` value"));
    }
}
