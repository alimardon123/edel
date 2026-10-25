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
}
