//! Who may change the machine, and the password that proves it (roadmap
//! M6.5; ADR-007's levels). doas runs `edel` as root for the admin group with
//! no doas password (`features/machine/etc/doas.d/edel.conf`), so `edel`
//! itself asks for what can break the machine or open it. Root started by
//! doas, which names the person who asked in `DOAS_USER`, must be in the
//! group `admin`; for an install, an update, a rollback or a change of root's
//! ssh keys, that person's own password is asked too. Root with no
//! `DOAS_USER` (a root login, the boot services, CI) is asked nothing. Not
//! root, nothing is asked here: a command that needs root fails later with
//! the message to run it with doas. The stick's account `live` may make the
//! computer's first person with no password while nobody has an account
//! (M6.12, `live_may_set_up`), as the Welcome does.
//!
//! The password comes from standard input with `--password-stdin` (Settings
//! writes it there), else from the terminal with echo off. It is checked by
//! the lock screen's own helper (`edel::password`), so a wrong one changes
//! nothing. No polkit: the admin's password is asked here, by `edel`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use edel::i18n::{tr, trf};
use edel::password::{self, Checked, MAX_BYTES, Password};
use edel::places;
use edel::settings;
use edel::users::{self, ADMIN_GROUP};

use crate::machine;
use crate::{
    Commands, ImageCommands, PresetCommands, ReleaseCommands, SettingsCommands, ThemeCommands,
};

/// The terminal a person types a password on, when there is one.
const TERMINAL: &str = "/dev/tty";

/// What a command needs of the person who runs it through doas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Only reads, or changes nothing: anyone doas lets run `edel`.
    Anyone,
    /// Changes the machine: an administrator.
    Admin,
    /// Changes what can break the machine or open it: an administrator, who
    /// also types their own password.
    Password,
}

/// What `edel` does before a command runs as root through doas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Run the command.
    Allow,
    /// Ask the password of the person who ran doas, then run the command.
    AskPassword,
    /// Refuse, with the message to show.
    Refuse(String),
}

/// What `command` needs (the table in the roadmap's M6.5 step). Where a
/// command both reads and writes, the write decides; unsure, it asks more.
pub fn need(command: &Commands) -> Need {
    match command {
        Commands::Update { check: true, .. } => Need::Anyone,
        Commands::Update { .. } | Commands::Rollback => Need::Password,
        Commands::Install { plan: true, .. } | Commands::Install { list: true, .. } => Need::Anyone,
        // It mounts other disks, read-only, to read one file.
        Commands::Install {
            find_settings: true,
            ..
        } => Need::Admin,
        Commands::Install { .. } => Need::Password,
        Commands::Status => Need::Anyone,
        Commands::Restart | Commands::Shutdown | Commands::Suspend => Need::Admin,
        // A profile is how hard the machine works, not what can break or
        // open it, and the login feature's rule lets the people at the
        // machine (group seat) set it with no password, so no administrator
        // is asked for it (M7.8).
        Commands::Power { .. } => Need::Anyone,
        Commands::Boot { .. } => Need::Admin,
        Commands::Report { esp } => {
            if *esp {
                Need::Admin
            } else {
                Need::Anyone
            }
        }
        Commands::Release { command } => match command {
            ReleaseCommands::Verify { .. } => Need::Anyone,
            _ => Need::Admin,
        },
        Commands::Settings { command } => match command {
            None
            | Some(
                SettingsCommands::Get { .. }
                | SettingsCommands::Diff { .. }
                | SettingsCommands::Export
                | SettingsCommands::Check { .. },
            ) => Need::Anyone,
            Some(
                SettingsCommands::Set { .. }
                | SettingsCommands::Reset { .. }
                | SettingsCommands::Apply { .. }
                | SettingsCommands::Import { .. }
                | SettingsCommands::FirstBoot { .. },
            ) => Need::Admin,
        },
        Commands::Image { command } => match command {
            ImageCommands::Check { .. } => Need::Anyone,
            ImageCommands::Build(_) => Need::Admin,
        },
        Commands::Theme { command } => match command {
            ThemeCommands::List => Need::Anyone,
            _ => Need::Admin,
        },
        Commands::Preset { command } => match command {
            PresetCommands::List => Need::Anyone,
            _ => Need::Admin,
        },
        // It only reads the signed list and this machine's add-ons.
        Commands::Addon { .. } => Need::Anyone,
        // A web app is the person's own files, in their own home.
        Commands::WebApp { .. } => Need::Anyone,
    }
}

/// The decision for a run as user `euid`, with `doas_user` as `DOAS_USER`
/// (the person who ran doas, if any), `is_admin` whether that person is in
/// the admin group, and what the command `need`s. Pure, so the table is
/// tested without root.
pub fn decide(euid: u32, doas_user: Option<&str>, is_admin: bool, need: Need) -> Decision {
    let user = match (euid, doas_user) {
        (0, Some(user)) => user,
        _ => return Decision::Allow,
    };
    match (need, is_admin) {
        (Need::Anyone, _) => Decision::Allow,
        (_, false) => Decision::Refuse(trf(
            "{user} is not in the group {group}, so edel will not change the system for them; ask an administrator to make them one",
            &[("user", user), ("group", ADMIN_GROUP)],
        )),
        (Need::Admin, true) => Decision::Allow,
        (Need::Password, true) => Decision::AskPassword,
    }
}

/// Whether `--password-stdin` was given: the password is read from standard
/// input, not the terminal.
static FROM_STDIN: AtomicBool = AtomicBool::new(false);

/// Whether the person who ran doas may install with no administrator and
/// no password (M6.6a): the stick's own account (`places::LIVE_USER`, which
/// has no password to type and is in no group) on a system that runs from
/// a stick. Whoever sits at a stick's desktop can start any system from it
/// anyway, so asking proves nothing; on an installed disk the rule is off.
pub fn live_may_install(doas_user: Option<&str>, from_a_stick: bool, installing: bool) -> bool {
    installing && from_a_stick && doas_user == Some(places::LIVE_USER)
}

/// Whether the person who ran doas may make the computer's first person
/// with no administrator and no password (M6.12): the account `live`
/// (`places::LIVE_USER`), which logs in by itself on a computer nobody has
/// an account on, while that is still so, for `edel settings first-boot
/// --user`, which Settings' Welcome runs. Whoever sits at such a computer
/// could start any system on it, so asking proves nothing; once a person
/// has an account the rule is off.
pub fn live_may_set_up(doas_user: Option<&str>, has_person: bool, first_person: bool) -> bool {
    first_person && !has_person && doas_user == Some(places::LIVE_USER)
}

/// Runs before `command`, once, after the command line is read: the
/// unsigned-install rule, the stick's install rule, the first person's rule
/// (M6.12), then the rules of [`decide`].
pub fn gate(command: &Commands, password_stdin: bool) -> Result<()> {
    FROM_STDIN.store(password_stdin, Ordering::Relaxed);
    if let Commands::Update { unsigned: true, .. } = command {
        developer_mode_needed()?;
    }
    let doas = std::env::var(places::AS_ROOT_USER).ok();
    let installing = matches!(command, Commands::Install { .. });
    if users::current() == Some(0)
        && live_may_install(
            doas.as_deref(),
            installing && edel::install::from_a_stick(),
            installing,
        )
    {
        return Ok(());
    }
    let first_person = matches!(
        command,
        Commands::Settings {
            command: Some(SettingsCommands::FirstBoot { user: Some(_), .. })
        }
    );
    if first_person
        && users::current() == Some(0)
        && live_may_set_up(
            doas.as_deref(),
            machine::has_person(&fs::read_to_string("/etc/passwd").unwrap_or_default()),
            first_person,
        )
    {
        return Ok(());
    }
    enforce(need(command), None)
}

/// Asks the password of the administrator who ran doas, before a change
/// that `what` names (such as "changing root's ssh keys"). Nothing is asked
/// when `edel` runs as root with no doas, or as anyone but root.
pub fn confirm(what: &str) -> Result<()> {
    enforce(Need::Password, Some(what))
}

/// Applies [`decide`] to this process: reads what it needs, and bails with
/// the refusal. `note` says what the password is for, shown before the
/// prompt.
fn enforce(need: Need, note: Option<&str>) -> Result<()> {
    let euid = users::current().unwrap_or(u32::MAX);
    let doas = std::env::var(places::AS_ROOT_USER)
        .ok()
        .filter(|u| !u.is_empty());
    let is_admin = match (euid, doas.as_deref()) {
        (0, Some(user)) => admin(user)?,
        _ => false,
    };
    match decide(euid, doas.as_deref(), is_admin, need) {
        Decision::Allow => Ok(()),
        Decision::Refuse(message) => bail!("{message}"),
        Decision::AskPassword => {
            let user = doas.unwrap_or_default();
            if let Some(what) = note {
                println!(
                    "edel settings: {}",
                    trf("{what} needs your password", &[("what", what)])
                );
            }
            ask(&user)
        }
    }
}

/// Whether `user` is in the admin group, as `/etc/group` says. Root is
/// always an administrator.
fn admin(user: &str) -> Result<bool> {
    if user == "root" {
        return Ok(true);
    }
    let group = fs::read_to_string("/etc/group")
        .with_context(|| tr("could not read /etc/group to see who may change the system"))?;
    Ok(machine::members(&group, ADMIN_GROUP)
        .is_some_and(|members| members.iter().any(|m| m == user)))
}

/// Refuses an unsigned install unless developer mode is on (ADR-007): the
/// flag file the setting `system.developer_mode` writes.
fn developer_mode_needed() -> Result<()> {
    if Path::new(places::DEVELOPER_FLAG).exists() {
        return Ok(());
    }
    bail!(
        "{}",
        tr(
            "--unsigned needs developer mode, which is off; turn it on with edel settings set system.developer_mode=true, or leave out --unsigned"
        )
    )
}

/// Reads `user`'s password (from standard input or the terminal) and checks
/// it. A wrong one, or no way to check, bails and changes nothing.
fn ask(user: &str) -> Result<()> {
    let password = if FROM_STDIN.load(Ordering::Relaxed) {
        read_stdin()?
    } else {
        read_terminal(user)?
    };
    let Some(helper) = password::checker() else {
        bail!(
            "{}",
            tr(
                "could not check the password: no password helper (unix_chkpwd, part of linux-pam) is installed; install linux-pam, then run it again"
            )
        );
    };
    match password::check(&helper, user, password) {
        Checked::Right => Ok(()),
        Checked::Wrong => bail!(
            "{}",
            trf(
                "wrong password for {user}; nothing was changed",
                &[("user", user)]
            )
        ),
        Checked::NoChecker(why) => bail!(
            "{}",
            trf(
                "could not check the password: {why}; nothing was changed",
                &[("why", &why)]
            )
        ),
    }
}

/// Reads one password line from standard input, `--password-stdin`.
fn read_stdin() -> Result<Password> {
    read_password_line(io::stdin().lock()).context(tr(
        "could not read the password from standard input; give it on one line",
    ))
}

/// The password from the first line of `reader`, at most [`MAX_BYTES`] of it
/// and without the newline. The line's bytes are wiped after the copy.
pub fn read_password_line(reader: impl BufRead) -> io::Result<Password> {
    let mut line = Vec::new();
    // One more than the cap, so a longer line is cut where it must be.
    reader
        .take((MAX_BYTES + 2) as u64)
        .read_until(b'\n', &mut line)?;
    let password = password_from_line(&line);
    line.fill(0);
    Ok(password)
}

/// `line` without its newline (`\n` or `\r\n`) as a password.
pub fn password_from_line(line: &[u8]) -> Password {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    Password::from_bytes(line)
}

/// Turns the terminal's echo off while it reads `user`'s password, and turns
/// it back on when the guard is dropped, on every path, errors and panics
/// included. Interrupts are off too, so Ctrl-C arrives as a character and
/// cancels the read instead of ending the program with echo off.
struct Echo {
    tty: File,
    saved: String,
}

impl Echo {
    /// Saves the terminal's settings and turns echo and interrupts off.
    fn off(tty: &File) -> Result<Echo> {
        let saved = stty(tty, &["-g"]).with_context(|| {
            tr("could not read the terminal's settings, so the password is not asked")
        })?;
        let echo = Echo {
            tty: tty.try_clone()?,
            saved: saved.trim().to_string(),
        };
        // Keys one at a time (no line buffering), so Ctrl-C cancels at once;
        // read_terminal handles Backspace and Ctrl-U itself.
        stty(tty, &["-echo", "-isig", "-icanon", "min", "1", "time", "0"]).with_context(|| {
            tr("could not turn off the terminal's echo, so the password is not asked")
        })?;
        Ok(echo)
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        let restored = self
            .tty
            .try_clone()
            .ok()
            .and_then(|tty| stty(&tty, &[self.saved.as_str()]).ok());
        if restored.is_none() {
            // A terminal that did not take its settings back: the usual
            // ones, which a shell sets again.
            if let Ok(tty) = self.tty.try_clone() {
                let _ = stty(&tty, &["echo", "icanon", "isig"]);
            }
            eprintln!(
                "{}",
                tr("warning: could not turn the terminal's echo back on; run stty echo to fix it")
            );
        }
    }
}

/// Runs `stty` with `args` on the terminal, returning what it printed.
fn stty(tty: &File, args: &[&str]) -> io::Result<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(Stdio::from(tty.try_clone()?))
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "stty exited with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Prompts on the terminal with echo off and reads one line. Ctrl-C cancels.
fn read_terminal(user: &str) -> Result<Password> {
    read_secret(&trf("Password for {user}: ", &[("user", user)]))
}

/// Shows `prompt` on the terminal and reads one line with echo off, as a
/// password is read; `edel install` asks the new person's password with it
/// (M6.6b). Ctrl-C cancels.
pub fn read_secret(prompt: &str) -> Result<Password> {
    let no_terminal = || {
        tr(
            "could not read the password: there is no terminal to ask on; run it in a terminal, or give the password on standard input with --password-stdin",
        )
    };
    let mut tty = File::open(TERMINAL).with_context(no_terminal)?;
    let mut shown = OpenOptions::new()
        .write(true)
        .open(TERMINAL)
        .with_context(no_terminal)?;
    let _echo = Echo::off(&tty)?;
    write!(shown, "{prompt}")?;
    shown.flush()?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    let mut cancelled = false;
    loop {
        match tty.read(&mut byte)? {
            0 => break,
            _ if byte[0] == b'\n' || byte[0] == b'\r' => break,
            // Ctrl-C with interrupts off.
            _ if byte[0] == 3 => {
                cancelled = true;
                break;
            }
            // Backspace (DEL or Ctrl-H) takes the last byte back, a whole
            // UTF-8 letter's continuation bytes with it; Ctrl-U empties it.
            _ if byte[0] == 0x7f || byte[0] == 0x08 => {
                while line.pop().is_some_and(|b| b & 0xc0 == 0x80) {}
            }
            _ if byte[0] == 0x15 => {
                line.fill(0);
                line.clear();
            }
            _ => {
                if line.len() <= MAX_BYTES + 1 {
                    line.push(byte[0]);
                }
            }
        }
    }
    writeln!(shown)?;
    if cancelled {
        line.fill(0);
        bail!("{}", tr("the password was not given; nothing was changed"));
    }
    let password = password_from_line(&line);
    line.fill(0);
    Ok(password)
}

/// Whether the change to root's ssh keys that `before` and `after` settings
/// files describe is one (the rule of `edel settings set|reset|import`).
pub fn root_keys_change(before: &str, after: &str) -> bool {
    let keys = |text: &str| {
        settings::read(text)
            .ok()
            .and_then(|read| read.file.users.get("root").and_then(|u| u.ssh_keys.clone()))
    };
    keys(before) != keys(after)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::Cli;

    fn need_of(args: &[&str]) -> Need {
        let cli = Cli::try_parse_from(args).unwrap();
        need(&cli.command)
    }

    /// Each command's need, by its shape (the table of M6.5's step).
    #[test]
    fn each_command_needs_what_the_table_says() {
        let cases: &[(&[&str], Need)] = &[
            (&["edel", "update", "--check", "r.toml"], Need::Anyone),
            (&["edel", "update", "r.toml"], Need::Password),
            (&["edel", "update"], Need::Password),
            (&["edel", "rollback"], Need::Password),
            (
                &[
                    "edel",
                    "install",
                    "/dev/sda",
                    "--settings",
                    "s.toml",
                    "--plan",
                ],
                Need::Anyone,
            ),
            (
                &[
                    "edel",
                    "install",
                    "/dev/sda",
                    "--settings",
                    "s.toml",
                    "--yes",
                ],
                Need::Password,
            ),
            (&["edel", "install", "--find-settings"], Need::Admin),
            (&["edel", "status"], Need::Anyone),
            (&["edel", "restart"], Need::Admin),
            (&["edel", "shutdown"], Need::Admin),
            (&["edel", "suspend"], Need::Admin),
            (&["edel", "boot", "guard"], Need::Admin),
            (&["edel", "report"], Need::Anyone),
            (&["edel", "report", "--esp"], Need::Admin),
            (
                &["edel", "release", "verify", "--keys", "k", "f"],
                Need::Anyone,
            ),
            (&["edel", "release", "sign", "--key", "k", "f"], Need::Admin),
            (&["edel", "settings"], Need::Anyone),
            (&["edel", "settings", "get"], Need::Anyone),
            (&["edel", "settings", "diff"], Need::Anyone),
            (&["edel", "settings", "export"], Need::Anyone),
            (&["edel", "settings", "check", "f"], Need::Anyone),
            (
                &["edel", "settings", "set", "network.hostname=x"],
                Need::Admin,
            ),
            (
                &["edel", "settings", "reset", "network.hostname"],
                Need::Admin,
            ),
            (&["edel", "settings", "apply"], Need::Admin),
            (&["edel", "settings", "import", "f"], Need::Admin),
            (
                &["edel", "settings", "first-boot", "--console"],
                Need::Admin,
            ),
            (
                &[
                    "edel",
                    "settings",
                    "first-boot",
                    "--user",
                    "ali",
                    "--new-password-stdin",
                    "region.timezone=Europe/Berlin",
                ],
                Need::Admin,
            ),
            (
                &["edel", "image", "check", "images/edel.toml"],
                Need::Anyone,
            ),
            (&["edel", "image", "build", "images/edel.toml"], Need::Admin),
            (&["edel", "theme", "list"], Need::Anyone),
            (&["edel", "theme", "save", "mine"], Need::Admin),
            (&["edel", "theme", "export", "mine"], Need::Admin),
            (&["edel", "power", "profile"], Need::Anyone),
            (&["edel", "power", "profile", "balanced"], Need::Anyone),
            (&["edel", "preset", "list"], Need::Anyone),
            (&["edel", "preset", "delete", "mine"], Need::Admin),
            (&["edel", "web-app", "add", "example.org"], Need::Anyone),
            (&["edel", "web-app", "list"], Need::Anyone),
            (&["edel", "web-app", "remove", "example.org"], Need::Anyone),
        ];
        for (args, want) in cases {
            assert_eq!(need_of(args), *want, "{args:?}");
        }
    }

    #[test]
    fn only_the_sticks_own_account_installs_without_a_password_and_only_from_a_stick() {
        assert!(live_may_install(Some("live"), true, true));
        assert!(
            !live_may_install(Some("live"), false, true),
            "an installed disk"
        );
        assert!(
            !live_may_install(Some("live"), true, false),
            "another command"
        );
        assert!(!live_may_install(Some("ali"), true, true), "a person");
        assert!(
            !live_may_install(None, true, true),
            "a root login decides as before"
        );
        let rule = include_str!("../../../features/login/etc/doas.d/edel-live.conf");
        for command in ["edel", "/usr/bin/edel"] {
            let line = format!(
                "permit nopass {} as root cmd {command}\n",
                places::LIVE_USER
            );
            assert!(
                rule.contains(&line),
                "the login feature's rule names {line}"
            );
        }
    }

    #[test]
    fn first_boot_takes_either_console_or_a_user_with_a_password_line() {
        let refused: [&[&str]; 3] = [
            &["edel", "settings", "first-boot"],
            &[
                "edel",
                "settings",
                "first-boot",
                "--console",
                "--user",
                "ali",
            ],
            &["edel", "settings", "first-boot", "--user", "ali"],
        ];
        for args in refused {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
        let accepted: [&[&str]; 2] = [
            &["edel", "settings", "first-boot", "--console"],
            &[
                "edel",
                "settings",
                "first-boot",
                "--user",
                "ali",
                "--new-password-stdin",
            ],
        ];
        for args in accepted {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn only_live_makes_the_first_person_and_only_while_nobody_has_an_account() {
        assert!(live_may_set_up(Some("live"), false, true));
        assert!(
            !live_may_set_up(Some("live"), true, true),
            "a person has an account"
        );
        assert!(
            !live_may_set_up(Some("live"), false, false),
            "another command"
        );
        assert!(!live_may_set_up(Some("ali"), false, true), "a person");
        assert!(!live_may_set_up(None, false, true), "a root login");
    }

    #[test]
    fn a_reader_is_let_through_for_anyone_and_a_root_login_asks_nothing() {
        for need in [Need::Anyone, Need::Admin, Need::Password] {
            assert_eq!(decide(1000, None, false, need), Decision::Allow, "not root");
            assert_eq!(decide(0, None, false, need), Decision::Allow, "root login");
        }
        assert_eq!(
            decide(1000, Some("ci"), false, Need::Password),
            Decision::Allow
        );
    }

    #[test]
    fn doas_asks_the_admin_their_password_for_what_can_break_the_machine() {
        assert_eq!(decide(0, Some("ci"), true, Need::Anyone), Decision::Allow);
        assert_eq!(decide(0, Some("ci"), true, Need::Admin), Decision::Allow);
        assert_eq!(
            decide(0, Some("ci"), true, Need::Password),
            Decision::AskPassword
        );
    }

    #[test]
    fn a_person_outside_the_admin_group_is_refused_by_name() {
        assert_eq!(
            decide(0, Some("guest"), false, Need::Anyone),
            Decision::Allow
        );
        match decide(0, Some("guest"), false, Need::Admin) {
            Decision::Refuse(message) => {
                assert!(
                    message.starts_with("guest is not in the group admin"),
                    "{message}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            decide(0, Some("guest"), false, Need::Password),
            Decision::Refuse(_)
        ));
    }

    #[test]
    fn a_password_line_loses_its_newline_and_is_capped() {
        let text = |p: &Password| p.text().unwrap_or_default().to_string();
        assert_eq!(text(&password_from_line(b"open\n")), "open");
        assert_eq!(text(&password_from_line(b"open\r\n")), "open");
        assert_eq!(text(&password_from_line(b"open")), "open");
        assert_eq!(text(&password_from_line(b"a b\n")), "a b");
        assert_eq!(password_from_line(&[b'x'; 2000]).letters(), MAX_BYTES);
        assert_eq!(password_from_line(b"\n").letters(), 0);
    }

    #[test]
    fn the_stdin_reader_takes_one_line_and_stops_there() {
        let read = read_password_line(&b"edel-ci-test\nignored\n"[..]).unwrap();
        assert_eq!(read.text(), Some("edel-ci-test"));
        let empty = read_password_line(&b""[..]).unwrap();
        assert_eq!(empty.letters(), 0);
        let long = vec![b'y'; MAX_BYTES * 3];
        assert_eq!(read_password_line(&long[..]).unwrap().letters(), MAX_BYTES);
    }

    #[test]
    fn a_change_of_root_s_ssh_keys_is_seen_and_nothing_else() {
        let none = "format = 1\n";
        let one = "format = 1\n[users.root]\nssh_keys = [\"ssh-ed25519 AAAA ops@edel\"]\n";
        let other = "format = 1\n[users.root]\nssh_keys = [\"ssh-ed25519 BBBB ops@edel\"]\n";
        let ci = "format = 1\n[users.ci]\nssh_keys = [\"ssh-ed25519 AAAA ci@edel\"]\n";
        assert!(root_keys_change(none, one), "a key added");
        assert!(root_keys_change(one, none), "a key removed");
        assert!(root_keys_change(one, other), "a key replaced");
        assert!(!root_keys_change(one, one), "the same keys");
        assert!(!root_keys_change(none, none));
        assert!(!root_keys_change(none, ci), "another person's keys");
        assert!(!root_keys_change("", none), "an empty file has no keys");
    }
}
