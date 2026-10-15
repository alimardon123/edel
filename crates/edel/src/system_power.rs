//! `edel restart`, `edel shutdown` and `edel suspend` (M5.10c): the power
//! menu's three actions, one way for the lock screen, Settings and people
//! at a terminal. Each runs the base's own program (`edel::base`).
//!
//! Root runs the program directly. Anyone else runs it through `doas -n`,
//! which the login feature's rule file lets group `seat` run with no
//! password and no arguments (`etc/doas.d/edel-power.conf`). The names
//! are the Settings app's, so the command line reads the same (ADR-008).

use std::process::Command;

use anyhow::{Result, bail};

use edel::i18n::{tr, trf};

use edel::base::{POWEROFF, REBOOT, SUSPEND};

/// One of the three power actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Restart,
    ShutDown,
    Suspend,
}

impl Action {
    /// The system program this action runs.
    pub fn program(self) -> &'static str {
        match self {
            Action::Restart => REBOOT,
            Action::ShutDown => POWEROFF,
            Action::Suspend => SUSPEND,
        }
    }
}

/// The command line to run: the program alone as root, else `doas -n`
/// with the program, so no password is asked and no argument is passed.
pub fn command(action: Action, root: bool) -> Vec<&'static str> {
    if root {
        vec![action.program()]
    } else {
        vec![edel::places::AS_ROOT, "-n", action.program()]
    }
}

/// Runs the action, its output going to this terminal, and waits for the
/// program to finish. A failure says what to do; a non-root failure also
/// names the login feature, which holds the doas rules.
pub fn run(action: Action) -> Result<()> {
    let root = edel::users::current() == Some(0);
    let argv = command(action, root);
    let mut process = Command::new(argv[0]);
    process.args(&argv[1..]);
    let hint = if root {
        ""
    } else {
        tr("; the power rules come with the login feature (doas), so check that it is installed")
    };
    let status = match process.status() {
        Ok(status) => status,
        Err(err) => {
            let why = err.to_string();
            let message = match action {
                Action::Restart => trf("could not restart: {why}", &[("why", &why)]),
                Action::ShutDown => trf("could not shut down: {why}", &[("why", &why)]),
                Action::Suspend => trf("could not suspend: {why}", &[("why", &why)]),
            };
            bail!("{message}{hint}");
        }
    };
    if !status.success() {
        let program = action.program();
        let exit = status.to_string();
        let message = match action {
            Action::Restart => trf(
                "could not restart: {program} exited with {status}",
                &[("program", program), ("status", &exit)],
            ),
            Action::ShutDown => trf(
                "could not shut down: {program} exited with {status}",
                &[("program", program), ("status", &exit)],
            ),
            Action::Suspend => trf(
                "could not suspend: {program} exited with {status}",
                &[("program", program), ("status", &exit)],
            ),
        };
        bail!("{message}{hint}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Action, command};

    #[test]
    fn root_runs_each_program_directly() {
        assert_eq!(command(Action::Restart, true), ["/sbin/reboot"]);
        assert_eq!(command(Action::ShutDown, true), ["/sbin/poweroff"]);
        assert_eq!(command(Action::Suspend, true), ["/usr/sbin/zzz"]);
    }

    #[test]
    fn anyone_else_runs_each_program_through_doas() {
        assert_eq!(
            command(Action::Restart, false),
            [edel::places::AS_ROOT, "-n", "/sbin/reboot"]
        );
        assert_eq!(
            command(Action::ShutDown, false),
            [edel::places::AS_ROOT, "-n", "/sbin/poweroff"]
        );
        assert_eq!(
            command(Action::Suspend, false),
            [edel::places::AS_ROOT, "-n", "/usr/sbin/zzz"]
        );
        for action in [Action::Restart, Action::ShutDown, Action::Suspend] {
            let argv = command(action, false);
            assert_eq!(&argv[..2], [edel::places::AS_ROOT, "-n"]);
            assert_eq!(argv.len(), 3, "no arguments are passed");
        }
    }
}
