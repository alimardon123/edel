//! `edel restart`, `edel shutdown` and `edel suspend` (M5.10c): the power
//! menu's three actions, one way for the lock screen, Settings and people
//! at a terminal. Each runs the base's own program (`edel::base`). Also
//! `edel power profile` (M7.8), the kernel's power profile, and battery care
//! (M7.16e): `edel power charge-full` for a trip, and `edel boot
//! battery-care`, which zzz runs after every resume.
//!
//! Root runs the program directly. Anyone else runs it through `doas -n`,
//! which the login feature's rule file lets group `seat` run with no
//! password and no arguments (`etc/doas.d/edel-power.conf`). The names
//! are the Settings app's, so the command line reads the same (ADR-008).

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use edel::base::{POWEROFF, REBOOT, SUSPEND};
use edel::i18n::{tr, trf};
use edel::settings::{self, ON_BATTERY};
use edel::{battery_care, places, profile, users};

use crate::machine;

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

/// `edel power profile [NAME]` (M7.8): without NAME, shows the power profile
/// the kernel uses and the ones it offers; with NAME, one of
/// `power.on_battery`'s values, sets it. Setting needs root: anyone else runs
/// the same command as root through doas, which the login feature's rule
/// lets the people at the machine run with no password.
pub fn profile(name: Option<&str>) -> Result<()> {
    let Some(name) = name else {
        match profile::now() {
            Some((now, offered)) => println!("profile: {now} (offered: {})", offered.join(" ")),
            None => println!(
                "{}",
                tr("this computer has no power profiles, so there is none to show")
            ),
        }
        return Ok(());
    };
    let names = settings::choices(ON_BATTERY);
    if !names.contains(&name) {
        let quoted = format!("{name:?}");
        let list = names.join(", ");
        bail!(trf(
            "unknown power profile {name}; use one of: {list}",
            &[("name", &quoted), ("list", &list)]
        ));
    }
    if users::current() == Some(0) {
        let kernel = profile::set(name)?;
        println!(
            "{}",
            trf(
                "power profile {name} ({kernel})",
                &[("name", name), ("kernel", &kernel)]
            )
        );
        return Ok(());
    }
    let argv = [
        places::AS_ROOT,
        "-n",
        places::EDEL_BIN,
        "power",
        "profile",
        name,
    ];
    let hint =
        tr("; the power rules come with the login feature (doas), so check that it is installed");
    let status = match Command::new(argv[0]).args(&argv[1..]).status() {
        Ok(status) => status,
        Err(err) => {
            let why = err.to_string();
            let message = trf("could not set the power profile: {why}", &[("why", &why)]);
            bail!("{message}{hint}");
        }
    };
    if !status.success() {
        let exit = status.to_string();
        let message = trf(
            "could not set the power profile: edel exited with {status}",
            &[("status", &exit)],
        );
        bail!("{message}{hint}");
    }
    Ok(())
}

/// `edel power charge-full` (M7.16e): charges to 100 percent once, for a
/// trip. The mark it leaves stops battery care's limit until the computer is
/// unplugged after a sleep, or the next start. Setting the limit needs root:
/// anyone else runs the same command as root through doas, which the login
/// feature's rule lets the people at the machine run with no password.
pub fn charge_full() -> Result<()> {
    let dir = Path::new(battery_care::POWER_SUPPLY);
    if !battery_care::offered(dir) {
        println!(
            "{}",
            tr("this laptop offers no charge limit, so it always charges to 100 percent")
        );
        return Ok(());
    }
    if users::current() != Some(0) {
        let argv = [
            places::AS_ROOT,
            "-n",
            places::EDEL_BIN,
            "power",
            "charge-full",
        ];
        let hint = tr(
            "; the power rules come with the login feature (doas), so check that it is installed",
        );
        let status = match Command::new(argv[0]).args(&argv[1..]).status() {
            Ok(status) => status,
            Err(err) => {
                let why = err.to_string();
                let message = trf("could not charge fully: {why}", &[("why", &why)]);
                bail!("{message}{hint}");
            }
        };
        if !status.success() {
            let exit = status.to_string();
            let message = trf(
                "could not charge fully: edel exited with {status}",
                &[("status", &exit)],
            );
            bail!("{message}{hint}");
        }
        return Ok(());
    }
    let mark = Path::new(places::CHARGE_FULL_ONCE);
    if let Some(folder) = mark.parent() {
        fs::create_dir_all(folder).with_context(|| {
            trf(
                "could not make {path}; run as root",
                &[("path", &folder.display().to_string())],
            )
        })?;
    }
    fs::write(mark, "").with_context(|| {
        trf(
            "could not write {path}; run as root",
            &[("path", &mark.display().to_string())],
        )
    })?;
    battery_care::set(dir, 100).map_err(anyhow::Error::msg)?;
    println!(
        "{}",
        tr(
            "charging to 100 percent once; the charge limit comes back after a sleep once unplugged, or at the next start"
        )
    );
    Ok(())
}

/// `edel boot battery-care` (M7.16e): sets the charge limit the settings
/// want, as zzz runs it after every resume (some laptops forget the limit
/// when they sleep) and at boot. Once the computer is unplugged, a trip's
/// full charge is over, so its mark goes. Always one line: the limit it set,
/// or that this computer offers none.
pub fn battery_care() -> Result<()> {
    let dir = Path::new(battery_care::POWER_SUPPLY);
    let mark = Path::new(places::CHARGE_FULL_ONCE);
    if mark.exists() && !battery_care::plugged_in(dir) {
        match fs::remove_file(mark) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    trf(
                        "could not remove {path}; run as root",
                        &[("path", &mark.display().to_string())],
                    )
                });
            }
        }
    }
    if !battery_care::offered(dir) {
        println!(
            "{}",
            tr("edel boot battery-care: this computer offers no charge limit")
        );
        return Ok(());
    }
    let percent = battery_care::wanted(machine::battery_care_on(), mark.exists());
    battery_care::set(dir, percent).map_err(anyhow::Error::msg)?;
    println!(
        "{}",
        trf(
            "edel boot battery-care: charge limit {percent} percent",
            &[("percent", &percent.to_string())]
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Action, command};

    #[test]
    fn the_login_feature_lets_the_seat_set_every_profile() {
        let rule = include_str!("../../../features/login/etc/doas.d/edel-power.conf");
        for name in edel::settings::choices(edel::settings::ON_BATTERY) {
            let line = format!(
                "permit nopass :seat as root cmd {} args power profile {name}\n",
                edel::places::EDEL_BIN
            );
            assert!(rule.contains(&line), "the power rules name {line}");
        }
        // M7.16e: Settings' Charge fully once button runs this for the seat.
        let charge = format!(
            "permit nopass :seat as root cmd {} args power charge-full\n",
            edel::places::EDEL_BIN
        );
        assert!(rule.contains(&charge), "the power rules name {charge}");
    }

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
