//! What the lock screen's power menu does (M5.10c): each choice, its
//! icon and name, the `edel` command it runs (`edel restart|shutdown|
//! suspend`, which run the system's program through doas) and whether an
//! update waits for a restart. No drawing. The command runs on a thread of
//! its own (`lock_card.rs`), never the drawing one. The login feature ships
//! the doas rule that lets the seat group run exactly the three programs
//! with `-n`, so nothing asks for a password. Where `edel` is not
//! installed, as on another distribution, the choice is asked of logind
//! over the system bus instead (`org.freedesktop.login1`, which
//! systemd-logind and elogind both serve), so the desktop's power menu
//! needs nothing of Edel OS (M6.17, M5.15).

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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

/// How long a suspend or a shut down holds the others off after its command
/// returns: the computer wakes or goes down, and the lid's and the button's
/// news that came with it is not acted on again.
const GUARD: Duration = Duration::from_secs(3);

/// Runs `choice` on a thread of its own, so the drawing never waits for it,
/// logging `edel-shell-ui: WHOpower: NAME` (`who` is empty in the session,
/// `greeter: ` at the login screen). A suspend or a shut down holds the
/// others off for `GUARD` after its command returns (M7.8): while `busy`
/// is set, another is logged as ignored and not run.
pub fn spawn(choice: Choice, busy: &Arc<AtomicBool>, who: &str) {
    let guarded = matches!(choice, Choice::Suspend | Choice::ShutDown);
    if guarded && busy.swap(true, Ordering::SeqCst) {
        eprintln!(
            "edel-shell-ui: {who}power: {} ignored, one is already under way",
            choice.name()
        );
        return;
    }
    eprintln!("edel-shell-ui: {who}power: {}", choice.name());
    let held = Arc::clone(busy);
    let spawned = std::thread::Builder::new()
        .name("power".into())
        .spawn(move || {
            if let Err(e) = run(choice) {
                eprintln!("edel-shell-ui: {e}");
            }
            if guarded {
                std::thread::sleep(GUARD);
                held.store(false, Ordering::SeqCst);
            }
        });
    if let Err(e) = spawned {
        if guarded {
            busy.store(false, Ordering::SeqCst);
        }
        eprintln!(
            "edel-shell-ui: {}",
            messages::power_failed(choice.name(), e)
        );
    }
}

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

    /// The `edel` command the choice runs; `edel` runs the system's
    /// program, through doas when the seat is not root, so nothing asks
    /// for a password (M5.10c).
    pub fn command(self) -> [&'static str; 2] {
        [crate::EDEL, self.name()]
    }

    /// The logind method the choice calls where `edel` is not installed;
    /// each takes one argument, whether polkit may ask for a password,
    /// which the power menu never lets it (M6.17).
    pub fn logind_method(self) -> &'static str {
        match self {
            Choice::Suspend => "Suspend",
            Choice::Restart => "Reboot",
            Choice::ShutDown => "PowerOff",
        }
    }
}

/// How the power menu carries a choice out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// `edel`'s command, on Edel OS.
    Edel,
    /// logind's method over the system bus, anywhere else.
    Logind,
}

/// The way this machine has: `edel` when it is installed, else logind.
pub fn way(edel_installed: bool) -> Way {
    if edel_installed {
        Way::Edel
    } else {
        Way::Logind
    }
}

/// Whether an update waits for a restart: `edel update` and `edel rollback`
/// leave the file, and `/run` forgets it at the next boot (M5.10c).
pub fn restart_waiting() -> bool {
    Path::new(edel::places::RESTART_PENDING).exists()
}

/// Carries `choice` out the way this machine has and waits for it. Ok when
/// it succeeds, else the message saying what failed.
pub fn run(choice: Choice) -> Result<(), String> {
    match way(crate::on_path(crate::EDEL)) {
        Way::Edel => run_edel(choice),
        Way::Logind => run_logind(choice),
    }
}

/// Asks logind for `choice` over the system bus, never letting polkit ask
/// for a password, so a person allowed it gets it and anyone else a no.
fn run_logind(choice: Choice) -> Result<(), String> {
    let failed = |e: zbus::Error| messages::power_failed_logind(choice.name(), e);
    let bus = zbus::blocking::Connection::system().map_err(failed)?;
    bus.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        choice.logind_method(),
        &(false,),
    )
    .map_err(failed)?;
    Ok(())
}

/// Runs `choice`'s `edel` command and waits for it. Ok when it succeeds,
/// else the message saying what failed, with the exit status and the
/// first line of what the command wrote to its standard error.
fn run_edel(choice: Choice) -> Result<(), String> {
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
    fn each_command_is_the_edel_command_of_its_name() {
        for choice in CHOICES {
            let command = choice.command();
            assert_eq!(command[0], "edel");
            assert_eq!(command[1], choice.name());
        }
        assert_eq!(Choice::Restart.command(), ["edel", "restart"]);
        assert_eq!(Choice::ShutDown.command(), ["edel", "shutdown"]);
        assert_eq!(Choice::Suspend.command(), ["edel", "suspend"]);
    }

    #[test]
    fn without_edel_the_menu_asks_logind() {
        assert_eq!(way(true), Way::Edel);
        assert_eq!(way(false), Way::Logind);
        let methods: Vec<&str> = CHOICES.iter().map(|c| c.logind_method()).collect();
        assert_eq!(methods, ["Suspend", "Reboot", "PowerOff"]);
    }
}
