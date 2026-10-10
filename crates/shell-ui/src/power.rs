//! What the lock screen's power menu does (M5.10c): each choice, its
//! icon and name, the command it runs through doas and whether an update
//! waits for a restart. No drawing. The command runs on a thread of its
//! own (`lock_card.rs`), never the drawing one. The login feature ships
//! the doas rule that lets the seat group run exactly these commands with
//! `-n`, so nothing asks for a password.

use std::path::Path;
use std::process::{Command, Stdio};

use edel::i18n::n_;

use crate::messages;

/// One power choice of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Suspend,
    Restart,
    ShutDown,
}

/// The menu's rows, top to bottom (M5.10c).
pub const CHOICES: [Choice; 3] = [Choice::Suspend, Choice::Restart, Choice::ShutDown];

impl Choice {
    /// The row's words, passed through `tr` where they are shown.
    pub fn label(self) -> &'static str {
        match self {
            Choice::Suspend => n_("Suspend"),
            Choice::Restart => n_("Restart"),
            Choice::ShutDown => n_("Shut down"),
        }
    }

    /// The icon in `design/icons/`.
    pub fn icon(self) -> &'static str {
        match self {
            Choice::Suspend => "moon",
            Choice::Restart => "restart",
            Choice::ShutDown => "power",
        }
    }

    /// The name logs and the places line use.
    pub fn name(self) -> &'static str {
        match self {
            Choice::Suspend => "suspend",
            Choice::Restart => "restart",
            Choice::ShutDown => "shutdown",
        }
    }

    /// The command, run with doas and `-n`, so it never asks for a
    /// password (M5.10c).
    pub fn command(self) -> [&'static str; 3] {
        match self {
            Choice::Suspend => ["doas", "-n", "/usr/libexec/edel-suspend"],
            Choice::Restart => ["doas", "-n", "/sbin/reboot"],
            Choice::ShutDown => ["doas", "-n", "/sbin/poweroff"],
        }
    }
}

/// Whether an update waits for a restart: `edel update` and `edel rollback`
/// leave the file, and `/run` forgets it at the next boot (M5.10c).
pub fn restart_waiting() -> bool {
    Path::new(edel::places::RESTART_PENDING).exists()
}

/// Runs `choice`'s command and waits for it. Ok when it succeeds, else the
/// message saying what failed, with the exit status and the first line of
/// what the command wrote to its standard error.
pub fn run(choice: Choice) -> Result<(), String> {
    let [program, args @ ..] = choice.command();
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(e) => return Err(messages::power_failed(choice.name(), e)),
    };
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(e) => return Err(messages::power_failed(choice.name(), e)),
    };
    if output.status.success() {
        return Ok(());
    }
    let first = String::from_utf8_lossy(&output.stderr)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let why = if first.is_empty() {
        output.status.to_string()
    } else {
        format!("{} ({first})", output.status)
    };
    Err(messages::power_failed(choice.name(), why))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_choice_has_its_name_icon_and_words() {
        let names: Vec<&str> = CHOICES.iter().map(|c| c.name()).collect();
        assert_eq!(names, ["suspend", "restart", "shutdown"]);
        let icons: Vec<&str> = CHOICES.iter().map(|c| c.icon()).collect();
        assert_eq!(icons, ["moon", "restart", "power"]);
        let labels: Vec<&str> = CHOICES.iter().map(|c| c.label()).collect();
        assert_eq!(labels, ["Suspend", "Restart", "Shut down"]);
    }

    #[test]
    fn each_command_runs_through_doas_without_asking() {
        assert_eq!(
            Choice::Suspend.command(),
            ["doas", "-n", "/usr/libexec/edel-suspend"]
        );
        assert_eq!(Choice::Restart.command(), ["doas", "-n", "/sbin/reboot"]);
        assert_eq!(Choice::ShutDown.command(), ["doas", "-n", "/sbin/poweroff"]);
        for choice in CHOICES {
            let command = choice.command();
            assert_eq!(command[0], "doas");
            assert_eq!(command[1], "-n");
        }
    }
}
