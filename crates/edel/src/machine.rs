//! `edel settings` (roadmap M2.2, M2.3, M5.25a): makes this machine match
//! its settings file, describes the machine as one, and reads and changes
//! the file. Apply runs at every boot from the `edel-settings` service,
//! seeding the file on the first, and on demand. It applies the sections that need no network:
//! the hostname, users, their ssh keys and developer mode. It adds and
//! changes, and never deletes a user (ADR-006).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt, chown};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::apps::{self, FLATHUB_REMOTE};
use edel::i18n::{tr, trf};
use edel::key_help::{self, DecidedBy};
use edel::panel_edit;
use edel::places;
use edel::presets;
use edel::services;
use edel::settings::{self, SettingsFile, User};
use edel::theme_file;
use edel::themes;
use edel::users::ADMIN_GROUP;

use crate::boot::GRUB_PREFIX;
use crate::privilege;
use crate::release::os_release_value;
use crate::update::{Disk, Lock, io_failure, run};

/// The machine's settings file as it is found now: by its name, or by a
/// name it had before (`edel::places`), until apply renames it.
fn machine_file() -> PathBuf {
    places::found(&places::machine_settings())
}
/// Developer mode is on while this file exists (ADR-007; M7.1 acts on it).
const DEVELOPER_FLAG: &str = places::DEVELOPER_FLAG;
const FIRST_PASSWORDS: &str = places::FIRST_PASSWORDS;
/// What this machine changed in `/etc`: a file here differs from the slot's.
const ETC_UPPER: &str = places::ETC_UPPER;
/// A volume with this label holding the settings file seeds a first boot.
const SEED_LABEL: &str = "EDEL-SEED";
/// The EFI system partition's number on the running disk.
const ESP_PARTITION: u32 = 1;
/// The login shell of a user whose entry names none, on an image without
/// bash.
pub const DEFAULT_SHELL: &str = "/bin/sh";
/// The login shell where the completion feature brings bash (M5.26).
const BASH: &str = "/bin/bash";

/// New people's login shell on this machine: bash where it is, so tab
/// completes `edel`, else busybox's shell.
pub fn default_shell() -> &'static str {
    if Path::new(BASH).exists() {
        BASH
    } else {
        DEFAULT_SHELL
    }
}
/// The group seatd lets use the screen and input (the `seat` feature,
/// M4.1); only images with a seat have it.
const SEAT_GROUP: &str = "seat";
/// The account greetd runs the greeter as, which needs the seat too.
const GREETER: &str = "greetd";
/// The most of a user's `authorized_keys` apply and export read.
const KEYS_MAX: u64 = 1 << 20;
/// Writes `~/.ssh/authorized_keys` from standard input. It runs as the
/// user, so nothing in the user's home can send it elsewhere.
const WRITE_KEYS: &str = "umask 077 && mkdir -p \"$HOME/.ssh\" && chmod 700 \"$HOME/.ssh\" && \
    cat >\"$HOME/.ssh/authorized_keys.edel-new\" && \
    chmod 600 \"$HOME/.ssh/authorized_keys.edel-new\" && \
    mv -f \"$HOME/.ssh/authorized_keys.edel-new\" \"$HOME/.ssh/authorized_keys\"";

/// One line of `/etc/passwd`.
#[derive(Debug, PartialEq)]
pub(crate) struct Account {
    pub(crate) name: String,
    uid: u32,
    gid: u32,
    pub(crate) home: String,
    shell: String,
}

pub(crate) fn accounts(passwd: &str) -> Vec<Account> {
    passwd
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() != 7 {
                return None;
            }
            Some(Account {
                name: f[0].into(),
                uid: f[2].parse().ok()?,
                gid: f[3].parse().ok()?,
                home: f[5].into(),
                shell: f[6].into(),
            })
        })
        .collect()
}

/// A person's account, not a system one: root, or a uid from 1000 below
/// `nobody`'s.
pub(crate) fn is_person(account: &Account) -> bool {
    account.uid == 0 || (1000..65534).contains(&account.uid)
}

/// Whether `passwd` has a person who could log in at the greeter: an
/// account with a uid from 1000 below `nobody`'s, root aside.
pub fn has_person(passwd: &str) -> bool {
    accounts(passwd).iter().any(|a| a.uid != 0 && is_person(a))
}

/// Adds the account the desktop logs in by itself from a stick (M3.6,
/// `edel boot live`): a system account, so the settings file never lists
/// it and apply and export leave it alone, with the shell greetd starts
/// the session through, no password (`*`) and the seat. The greeter gets
/// the seat too, as `apply` gives it on a machine with a settings file,
/// which a stick has not: without it, logging out of the live session
/// left a greeter that could not open the screen.
pub fn add_live_account(name: &str) -> Result<()> {
    if !accounts(&fs::read_to_string("/etc/passwd")?)
        .iter()
        .any(|a| a.name == name)
    {
        let home = format!("/home/{name}");
        run(Command::new("adduser").args(["-S", "-D", "-s", default_shell(), "-h", &home, name]))?;
        unlock(name)?;
    }
    let greeter = accounts(&fs::read_to_string("/etc/passwd")?)
        .iter()
        .any(|a| a.name == GREETER);
    let group = fs::read_to_string("/etc/group")?;
    for who in [name, GREETER] {
        if (who == name || greeter)
            && members(&group, SEAT_GROUP).is_some_and(|m| !m.iter().any(|m| m == who))
        {
            run(Command::new("addgroup").args([who, SEAT_GROUP]))?;
        }
    }
    Ok(())
}

/// The members of `group` in `/etc/group`, or `None` without the group.
pub(crate) fn members(group_file: &str, group: &str) -> Option<Vec<String>> {
    group_file.lines().find_map(|line| {
        let f: Vec<&str> = line.split(':').collect();
        (f.len() == 4 && f[0] == group).then(|| {
            f[3].split(',')
                .filter(|m| !m.is_empty())
                .map(String::from)
                .collect()
        })
    })
}

/// `text` with field `field` of `name`'s line replaced by `change`, which
/// returns `None` to leave it; `None` when no line changed.
fn with_field(
    text: &str,
    name: &str,
    field: usize,
    change: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut changed = false;
    let mut out = String::new();
    for line in text.lines() {
        let mut f: Vec<String> = line.split(':').map(String::from).collect();
        if f.len() > field && f[0] == name {
            if let Some(new) = change(&f[field]) {
                f[field] = new;
                changed = true;
            }
        }
        out.push_str(&f.join(":"));
        out.push('\n');
    }
    changed.then_some(out)
}

/// `/etc/shadow` with `name`'s password changed from adduser's `!`, which
/// OpenSSH treats as a locked account, to `*`: no password, and key logins
/// work. A real hash, locked or not, is left alone.
fn shadow_unlocked(shadow: &str, name: &str) -> Option<String> {
    with_field(shadow, name, 1, |pw| {
        (!pw.is_empty() && pw.bytes().all(|b| b == b'!')).then(|| "*".to_string())
    })
}

/// `/etc/passwd` with `name`'s login shell set to `shell`.
fn passwd_with_shell(passwd: &str, name: &str, shell: &str) -> Option<String> {
    with_field(passwd, name, 6, |old| {
        (old != shell).then(|| shell.to_string())
    })
}

/// Replaces a file through a new file and a rename, keeping its mode and
/// owner, so a power cut leaves the old or the new account file.
fn replace(path: &Path, text: &str) -> Result<()> {
    let meta = fs::metadata(path).with_context(|| could_not_read(path))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, text)?;
    fs::set_permissions(&new, meta.permissions())?;
    chown(&new, Some(meta.uid()), Some(meta.gid()))?;
    fs::File::open(&new)?.sync_all()?;
    fs::rename(&new, path).with_context(|| {
        trf(
            "could not replace {path}",
            &[("path", &path.display().to_string())],
        )
    })
}

/// `could not read PATH`, the start of a failure to read a file.
fn could_not_read(path: &Path) -> String {
    trf(
        "could not read {path}",
        &[("path", &path.display().to_string())],
    )
}

/// `could not write PATH`, the start of a failure to write a file.
fn could_not_write(path: &Path) -> String {
    trf(
        "could not write {path}",
        &[("path", &path.display().to_string())],
    )
}

/// One change apply makes; `edel settings diff` lists them without making them.
#[derive(Debug, PartialEq)]
enum Change {
    /// Write `/etc/hostname`
    Hostname(String),
    /// The file names no hostname: drop this machine's copy of
    /// `/etc/hostname`, so the slot's shows again (ADR-008, defaults)
    HostnameDefault,
    /// Point `/etc/localtime` at the zone's file and write `/etc/timezone`
    Timezone(String),
    /// The file names no zone: take this machine's `/etc/localtime` and
    /// `/etc/timezone` out, so the clock is UTC again
    TimezoneDefault,
    AddUser {
        name: String,
        shell: String,
    },
    Shell {
        name: String,
        shell: String,
    },
    /// adduser's `!` becomes `*`, so key logins work
    Unlock(String),
    AdminGroup,
    Admin {
        name: String,
        on: bool,
    },
    Keys {
        name: String,
        keys: Vec<String>,
    },
    Developer(bool),
    /// Add a service to its runlevel, or take it out (M6.10)
    Service(services::Step),
    /// Give these people the password `edel install` asked for, then
    /// delete `places::FIRST_PASSWORDS` (M6.6b)
    Passwords(Vec<String>),
    /// May use the screen and input: a person in the file, or the greeter
    Seat(String),
    /// Flatpak apps from Flathub for an admin person, installed as that
    /// person with `flatpak --user` (M6.1)
    Flatpak {
        name: String,
        ids: Vec<String>,
    },
    /// Write `/etc/xdg/mimeapps.list` with these default apps; an empty
    /// text removes the file (M6.10)
    MimeDefaults(String),
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Hostname(h) => write!(f, "network.hostname: set to {h}"),
            Change::HostnameDefault => write!(f, "network.hostname: back to the release's"),
            Change::Timezone(z) => write!(f, "region.timezone: set to {z}"),
            Change::TimezoneDefault => write!(f, "region.timezone: back to the release's (UTC)"),
            Change::AddUser { name, shell } => write!(f, "users.{name}: add, with shell {shell}"),
            Change::Shell { name, shell } => write!(f, "users.{name}.login_shell: set to {shell}"),
            Change::Unlock(name) => write!(f, "users.{name}: allow key logins"),
            Change::AdminGroup => write!(f, "group {ADMIN_GROUP}: add"),
            Change::Admin { name, on } => {
                write!(f, "users.{name}.admin: {}", if *on { "on" } else { "off" })
            }
            Change::Keys { name, keys } => {
                write!(f, "users.{name}.ssh_keys: write {} keys", keys.len())
            }
            Change::Service(step) if step.on => write!(
                f,
                "services: start {} and add it to the {} runlevel",
                step.service, step.level
            ),
            Change::Service(step) => write!(
                f,
                "services: stop {} and take it out of the {} runlevel",
                step.service, step.level
            ),
            Change::Passwords(names) if names.is_empty() => {
                write!(
                    f,
                    "remove the passwords edel install left, as nobody in the file has one"
                )
            }
            Change::Passwords(names) => write!(
                f,
                "give {} the password edel install asked for",
                names.join(", ")
            ),
            Change::Developer(on) => {
                write!(
                    f,
                    "system.developer_mode: {}",
                    if *on { "on" } else { "off" }
                )
            }
            Change::Seat(name) => write!(f, "group {SEAT_GROUP}: add {name}"),
            Change::Flatpak { name, ids } => {
                write!(f, "apps.installed for {name}: install {}", ids.join(", "))
            }
            Change::MimeDefaults(text) if text.is_empty() => {
                write!(f, "remove {} (no default apps set)", places::MIME_DEFAULTS)
            }
            Change::MimeDefaults(_) => {
                write!(f, "write {} (the default apps)", places::MIME_DEFAULTS)
            }
        }
    }
}

/// What apply compares the file with, read from the machine.
#[derive(Default)]
struct Machine {
    hostname: String,
    /// Whether this machine has its own `/etc/hostname`
    hostname_set: bool,
    /// The zone this machine's own `/etc/localtime` names, if it has one
    timezone: Option<String>,
    passwd: String,
    group: String,
    /// `None` when this user may not read it
    shadow: Option<String>,
    /// `~/.ssh/authorized_keys` of each named user that has one
    keys: BTreeMap<String, String>,
    /// The shells the file names that are programs on this machine
    shells: BTreeSet<String>,
    /// New people's login shell here ([`default_shell`])
    default_shell: String,
    developer: bool,
    /// The Flatpak apps each admin person has (M6.1), by login name
    flatpak: BTreeMap<String, BTreeSet<String>>,
    /// The features this machine has, for `[services]` (M6.10)
    features: Vec<(String, edel::features::Feature)>,
    /// The features the image left off (`EDEL_SERVICES_OFF`)
    services_off: Vec<String>,
    /// The services in OpenRC's runlevels now
    runlevels: BTreeSet<services::Entry>,
    /// The system's `mimeapps.list` (M6.10), `None` when the file is missing
    mime_defaults: Option<String>,
    /// `places::FIRST_PASSWORDS`, which `edel install` leaves (M6.6b)
    first_passwords: Option<String>,
}

impl Machine {
    fn read(file: &SettingsFile) -> Result<Machine> {
        let passwd = fs::read_to_string("/etc/passwd")?;
        let mut keys = BTreeMap::new();
        let mut flatpak = BTreeMap::new();
        // Asked only when the file lists apps, so a machine without any
        // never starts flatpak (M6.1).
        let lists_apps = file.apps.installed.as_ref().is_some_and(|l| !l.is_empty());
        for account in accounts(&passwd) {
            if lists_apps
                && file
                    .users
                    .get(&account.name)
                    .is_some_and(|u| u.admin == Some(true))
            {
                flatpak.insert(account.name.clone(), installed_apps(&account));
            }
            if file.users.contains_key(&account.name) {
                if let Some(text) = read_keys(&account) {
                    keys.insert(account.name, text);
                }
            }
        }
        let shells = file
            .users
            .values()
            .filter_map(|u| u.login_shell.clone())
            .filter(|s| {
                fs::metadata(s).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
            .collect();
        Ok(Machine {
            hostname: fs::read_to_string("/etc/hostname").unwrap_or_default(),
            hostname_set: Path::new(ETC_UPPER).join("hostname").exists(),
            timezone: own_timezone(),
            passwd,
            group: fs::read_to_string("/etc/group")?,
            shadow: fs::read_to_string("/etc/shadow").ok(),
            keys,
            shells,
            default_shell: default_shell().to_string(),
            developer: Path::new(DEVELOPER_FLAG).exists(),
            flatpak,
            features: services::installed(),
            services_off: services::off_by_default(
                &fs::read_to_string("/usr/lib/os-release").unwrap_or_default(),
            ),
            runlevels: services::enabled_in(Path::new("/etc/runlevels")),
            mime_defaults: fs::read_to_string(places::MIME_DEFAULTS).ok(),
            first_passwords: fs::read_to_string(FIRST_PASSWORDS).ok(),
        })
    }
}

/// The people of `users` that `places::FIRST_PASSWORDS`' `text` has a
/// password for, in the file's order.
fn first_password_names<'a>(text: &str, users: impl Iterator<Item = &'a String>) -> Vec<String> {
    let users: BTreeSet<&String> = users.collect();
    text.lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| users.iter().any(|u| u.as_str() == *name))
        .map(|(name, _)| name.to_string())
        .collect()
}

/// The `NAME:HASH` lines of `text` for `names`, as `chpasswd -e` takes
/// them; a line whose hash is not a crypt hash is left out.
fn first_password_lines(text: &str, names: &[String]) -> String {
    text.lines()
        .filter(|line| {
            line.split_once(':').is_some_and(|(name, hash)| {
                names.iter().any(|n| n == name) && hash.starts_with('$') && !hash.contains(':')
            })
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// Runs `cmd` with `input` on its standard input, failing on a non-zero
/// exit; the input is not shown in the message.
fn run_with_input(cmd: &mut Command, input: &str) -> Result<()> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {:?}", cmd.get_program()))?;
    child
        .stdin
        .take()
        .context("no input")?
        .write_all(input.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!("{:?} failed with {status}", cmd.get_program());
    }
    Ok(())
}

/// The zone this machine's own `/etc/localtime` link names (`region.timezone`):
/// None when the overlay's upper layer holds no link of ours, or the link
/// points somewhere other than the zone files.
fn own_timezone() -> Option<String> {
    fs::symlink_metadata(Path::new(ETC_UPPER).join("localtime")).ok()?;
    zone_of_target(&fs::read_link(places::LOCALTIME).ok()?)
}

/// The zone a link to `target` names: the target without the zone files'
/// folder in front of it, None when it is not inside that folder.
fn zone_of_target(target: &Path) -> Option<String> {
    let prefix = format!("{}/", places::ZONEINFO_DIR);
    target
        .to_str()?
        .strip_prefix(&prefix)
        .filter(|zone| !zone.is_empty())
        .map(str::to_string)
}

/// The changes that make `machine` match `file`, and notes on what is left
/// alone. Pure, so diff and apply agree and tests need no machine.
fn plan(file: &SettingsFile, machine: &Machine) -> (Vec<Change>, Vec<String>) {
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    match &file.network.hostname {
        Some(h) if machine.hostname.trim() != h => changes.push(Change::Hostname(h.clone())),
        None if machine.hostname_set => changes.push(Change::HostnameDefault),
        _ => {}
    }
    match &file.region.timezone {
        Some(z) if machine.timezone.as_deref() != Some(z) => {
            changes.push(Change::Timezone(z.clone()))
        }
        None if machine.timezone.is_some() => changes.push(Change::TimezoneDefault),
        _ => {}
    }
    let all = accounts(&machine.passwd);
    let mut admin_group = members(&machine.group, ADMIN_GROUP).is_some();
    let seated = members(&machine.group, SEAT_GROUP);
    let mut seat = Vec::new();
    for (name, user) in &file.users {
        let account = all.iter().find(|a| &a.name == name);
        // A shell the machine lacks would lock the user out: OpenSSH and
        // login refuse a shell that does not exist. The current one stays.
        let shell = match user.login_shell.as_deref() {
            Some(s) if !machine.shells.contains(s) => {
                notes.push(format!(
                    "kept users.{name}.login_shell as it is: {s} is not a program on this machine"
                ));
                account.map_or(machine.default_shell.as_str(), |a| a.shell.as_str())
            }
            Some(s) => s,
            None => &machine.default_shell,
        }
        .to_string();
        let is_admin =
            members(&machine.group, ADMIN_GROUP).is_some_and(|m| m.iter().any(|m| m == name));
        match account {
            Some(account) if !is_person(account) => {
                notes.push(format!("left users.{name} alone: it is a system account"));
                continue;
            }
            Some(account) => {
                if account.shell != shell {
                    changes.push(Change::Shell {
                        name: name.clone(),
                        shell,
                    });
                }
                if machine
                    .shadow
                    .as_deref()
                    .and_then(|s| shadow_unlocked(s, name))
                    .is_some()
                {
                    changes.push(Change::Unlock(name.clone()));
                }
            }
            None => changes.push(Change::AddUser {
                name: name.clone(),
                shell,
            }),
        }
        seat.push(name.clone());
        let admin = user.admin == Some(true);
        if admin && !admin_group {
            changes.push(Change::AdminGroup);
            admin_group = true;
        }
        if admin != is_admin {
            changes.push(Change::Admin {
                name: name.clone(),
                on: admin,
            });
        }
        // The Flatpak apps the file lists, for each admin person: the ones
        // they lack, in one change (M6.1). Nothing is ever uninstalled.
        if admin {
            let have = machine.flatpak.get(name);
            let ids: BTreeSet<String> = file
                .apps
                .installed
                .iter()
                .flatten()
                .filter(|id| !have.is_some_and(|h| h.contains(*id)))
                .cloned()
                .collect();
            if !ids.is_empty() {
                changes.push(Change::Flatpak {
                    name: name.clone(),
                    ids: ids.into_iter().collect(),
                });
            }
        }
        // Absent ssh_keys leaves the file alone: keys a person added by
        // hand are theirs.
        if let Some(keys) = &user.ssh_keys {
            let keys: Vec<String> = keys.iter().filter(|k| !k.contains('\n')).cloned().collect();
            if machine.keys.get(name) != Some(&keys_text(&keys)) {
                changes.push(Change::Keys {
                    name: name.clone(),
                    keys,
                });
            }
        }
    }
    // The default apps (M6.10). A file this release did not write is the
    // administrator's: never overwritten or removed. Ours is rewritten
    // when it differs, and removed when the file sets no MIME type.
    let mime_now = machine.mime_defaults.as_deref();
    let wanted = apps::mime_defaults(&settings::default_app_roles(&file.default_apps));
    let theirs = mime_now.is_some_and(|text| !text.starts_with(apps::MIME_COMMENT));
    if !theirs && mime_now.unwrap_or("") != wanted {
        changes.push(Change::MimeDefaults(wanted));
    }
    // The passwords edel install asked for, given once the people exist
    // (they are added above), then the file goes (M6.6b).
    if let Some(text) = &machine.first_passwords {
        changes.push(Change::Passwords(first_password_names(
            text,
            file.users.keys(),
        )));
    }
    let developer = file.system.developer_mode == Some(true);
    if developer != machine.developer {
        changes.push(Change::Developer(developer));
    }
    // Switchable features on or off (M6.10); a name the machine cannot
    // switch is a note, as apply is lenient on a machine.
    for problem in services::problems(&file.services, &machine.features) {
        notes.push(problem);
    }
    for step in services::plan(
        &machine.features,
        &file.services,
        &machine.services_off,
        &machine.runlevels,
    ) {
        changes.push(Change::Service(step));
    }
    // On a desktop (M4.2b), every person in the file and the greeter may
    // use the screen and input; machines without a seat have no group.
    if let Some(seated) = seated {
        if all.iter().any(|a| a.name == GREETER) {
            seat.push(GREETER.to_string());
        }
        for name in seat {
            if !seated.contains(&name) {
                changes.push(Change::Seat(name));
            }
        }
    }
    (changes, notes)
}

fn keys_text(keys: &[String]) -> String {
    keys.iter().map(|k| format!("{k}\n")).collect()
}

fn find_account(name: &str) -> Result<Account> {
    accounts(&fs::read_to_string("/etc/passwd")?)
        .into_iter()
        .find(|a| a.name == name)
        .with_context(|| trf("{name} is not in /etc/passwd", &[("name", name)]))
}

/// `~/.ssh/authorized_keys` of `account`, read without trusting the home,
/// which the user owns: never through a symlink, never from a FIFO or a
/// device, only a regular file the user owns and at most `KEYS_MAX` long.
/// Anything else counts as no file, so apply writes the keys, as the user.
fn read_keys(account: &Account) -> Option<String> {
    let path = Path::new(&account.home).join(".ssh/authorized_keys");
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != account.uid || meta.len() > KEYS_MAX {
        eprintln!(
            "edel settings: {}",
            trf(
                "ignored {path}: not a regular file of {user} up to 1 MiB",
                &[
                    ("path", &path.display().to_string()),
                    ("user", &account.name)
                ]
            )
        );
        return None;
    }
    let mut text = String::new();
    file.take(KEYS_MAX).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Writes `account`'s `~/.ssh/authorized_keys` as that user, never as root:
/// the user owns the home and every path in it, so a symlink there could
/// send root's writes, chmods and chowns to any file (WRITE_KEYS).
fn write_keys(account: &Account, text: &str) -> Result<()> {
    let mut child = Command::new("/bin/sh")
        .args(["-c", WRITE_KEYS])
        .env_clear()
        .env("HOME", &account.home)
        .env("PATH", "/usr/bin:/bin")
        .uid(account.uid)
        .gid(account.gid)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| {
            trf(
                "could not start a shell as {user}",
                &[("user", &account.name)],
            )
        })?;
    let written = child
        .stdin
        .take()
        .context(tr("no input to the shell"))?
        .write_all(text.as_bytes());
    let status = child.wait()?;
    if !status.success() || written.is_err() {
        bail!(
            "{}",
            trf(
                "could not write {home}/.ssh/authorized_keys as {user}; check that the home directory exists and {user} owns it",
                &[("home", &account.home), ("user", &account.name)]
            )
        );
    }
    Ok(())
}

fn unlock(name: &str) -> Result<()> {
    if let Some(text) = shadow_unlocked(&fs::read_to_string("/etc/shadow")?, name) {
        replace(Path::new("/etc/shadow"), &text)?;
    }
    Ok(())
}

/// Makes one change. With `boot`, the `hostname` service, which runs
/// next, sets the running hostname.
fn execute(change: &Change, boot: bool) -> Result<()> {
    match change {
        Change::Hostname(h) => {
            fs::write("/etc/hostname", format!("{h}\n"))?;
            if !boot {
                run(Command::new("hostname").arg(h))?;
            }
        }
        Change::HostnameDefault => {
            // The overlay's upper directory changes under it, which it
            // tolerates; dropping cached names makes the slot's file show
            // at once instead of at the next boot.
            fs::remove_file(Path::new(ETC_UPPER).join("hostname"))?;
            let _ = fs::write("/proc/sys/vm/drop_caches", "2");
            let release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
            if let (false, Some(h)) = (boot, os_release_value(&release, "EDEL_HOSTNAME")) {
                run(Command::new("hostname").arg(h))?;
            }
        }
        Change::Timezone(zone) => {
            let link = Path::new(places::LOCALTIME);
            match fs::remove_file(link) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).with_context(|| could_not_write(link)),
            }
            std::os::unix::fs::symlink(format!("{}/{zone}", places::ZONEINFO_DIR), link)
                .with_context(|| could_not_write(link))?;
            let name = Path::new(places::TIMEZONE_FILE);
            fs::write(name, format!("{zone}\n")).with_context(|| could_not_write(name))?;
        }
        Change::TimezoneDefault => {
            // As the hostname's default: the overlay's upper directory
            // changes under it, and dropping cached names shows the slot's
            // files at once.
            for file in ["localtime", "timezone"] {
                let upper = Path::new(ETC_UPPER).join(file);
                match fs::remove_file(&upper) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err).with_context(|| could_not_write(&upper)),
                }
            }
            let _ = fs::write("/proc/sys/vm/drop_caches", "2");
        }
        Change::AddUser { name, shell } => {
            run(Command::new("adduser").args(["-D", "-s", shell, name]))?;
            unlock(name)?;
        }
        Change::Shell { name, shell } => {
            if let Some(text) = passwd_with_shell(&fs::read_to_string("/etc/passwd")?, name, shell)
            {
                replace(Path::new("/etc/passwd"), &text)?;
            }
        }
        Change::Unlock(name) => unlock(name)?,
        Change::AdminGroup => run(Command::new("addgroup").args(["-S", ADMIN_GROUP]))?,
        Change::Admin { name, on: true } => {
            run(Command::new("addgroup").args([name, ADMIN_GROUP]))?
        }
        Change::Admin { name, on: false } => {
            run(Command::new("delgroup").args([name, ADMIN_GROUP]))?
        }
        Change::Keys { name, keys } => write_keys(&find_account(name)?, &keys_text(keys))?,
        Change::Developer(true) => {
            let flag = Path::new(DEVELOPER_FLAG);
            fs::create_dir_all(flag.parent().unwrap_or(Path::new("/")))?;
            fs::write(flag, "")?;
        }
        Change::Developer(false) => fs::remove_file(DEVELOPER_FLAG)?,
        // At boot only the runlevel changes: OpenRC starts the default
        // runlevel after this boot service, so what was added starts then
        // and what was taken out never does. Later, it happens at once.
        Change::Service(step) => {
            let (verb, now) = if step.on {
                ("add", "start")
            } else {
                ("del", "stop")
            };
            if step.on {
                run(Command::new("rc-update").args([verb, &step.service, &step.level]))?;
            }
            if !boot {
                let status = Command::new("rc-service")
                    .args([&step.service, now])
                    .status()?;
                if step.on && !status.success() {
                    bail!(
                        "{}",
                        trf(
                            "{service} did not start ({status}); rc-service {service} start says why",
                            &[("service", &step.service), ("status", &status.to_string())]
                        )
                    );
                }
            }
            if !step.on {
                run(Command::new("rc-update").args([verb, &step.service, &step.level]))?;
            }
        }
        Change::Passwords(names) => {
            let text = fs::read_to_string(FIRST_PASSWORDS)
                .with_context(|| could_not_read(Path::new(FIRST_PASSWORDS)))?;
            let lines = first_password_lines(&text, names);
            if !lines.is_empty() {
                run_with_input(Command::new("chpasswd").arg("-e"), &lines)?;
            }
            fs::remove_file(FIRST_PASSWORDS)?;
        }
        Change::Seat(name) => run(Command::new("addgroup").args([name, SEAT_GROUP]))?,
        Change::Flatpak { name, ids } => install_apps(&find_account(name)?, ids)?,
        Change::MimeDefaults(text) => {
            let path = Path::new(places::MIME_DEFAULTS);
            if text.is_empty() {
                fs::remove_file(path).with_context(|| could_not_write(path))?;
            } else {
                write_whole(path, text.as_bytes())?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o644))
                    .with_context(|| could_not_write(path))?;
            }
        }
    }
    Ok(())
}

/// `program` with `args`, run as `account` in its own home, with only its
/// name, home and path as environment, as `su` gives them and `write_keys`
/// runs its shell: root starts it, and the person owns everything it
/// writes (M6.1).
pub(crate) fn as_user<S: AsRef<OsStr>>(account: &Account, program: &str, args: &[S]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .env("HOME", &account.home)
        .env("USER", &account.name)
        .env("LOGNAME", &account.name)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(&account.home)
        .uid(account.uid)
        .gid(account.gid);
    cmd
}

/// The Flatpak apps `account` has, from `flatpak --user list` run as that
/// person (M6.1). A machine with no flatpak, or a list that fails, has none.
fn installed_apps(account: &Account) -> BTreeSet<String> {
    let listed = as_user(
        account,
        "flatpak",
        &["--user", "list", "--app", "--columns=application"],
    )
    .stderr(Stdio::null())
    .output();
    match listed {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(String::from)
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// The `flatpak` argument lists that add Flathub and then install each of
/// `ids`, the remote first (M6.1). Pure, so tests read the one constant.
fn flatpak_commands(ids: &[String]) -> Vec<Vec<String>> {
    let mut commands = vec![
        [
            "--user",
            "remote-add",
            "--if-not-exists",
            "flathub",
            FLATHUB_REMOTE,
        ]
        .map(String::from)
        .to_vec(),
    ];
    for id in ids {
        commands.push(
            [
                "--user",
                "install",
                "--noninteractive",
                "flathub",
                id.as_str(),
            ]
            .map(String::from)
            .to_vec(),
        );
    }
    commands
}

/// Adds Flathub for `account` and installs each of `ids` it lacks, as that
/// person (M6.1). One app that fails does not stop the others; the error
/// names those that failed, and the next apply tries them again.
pub(crate) fn install_apps(account: &Account, ids: &[String]) -> Result<()> {
    let mut commands = flatpak_commands(ids).into_iter();
    let remote = commands.next().unwrap_or_default();
    run(&mut as_user(account, "flatpak", &remote)).with_context(|| {
        trf(
            "could not add Flathub for {user}; check the network and run edel settings apply again",
            &[("user", &account.name)],
        )
    })?;
    let mut failed = Vec::new();
    for (command, id) in commands.zip(ids) {
        if run(&mut as_user(account, "flatpak", &command)).is_err() {
            failed.push(id.as_str());
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    bail!(
        "{}",
        trf(
            "could not install {apps} from Flathub for {user}; check the network and run edel settings apply again, the other apps were installed",
            &[("apps", &failed.join(", ")), ("user", &account.name)]
        )
    )
}

/// What apply says in place of `change` when it runs at boot (M6.1): the
/// boot never waits on a download, so Flatpak apps wait for an apply that
/// may use the network (M7.2b). None for every other change.
fn deferred(change: &Change, boot: bool) -> Option<String> {
    match change {
        Change::Flatpak { name, .. } if boot => Some(trf(
            "flatpak apps for {user} wait for the network; edel settings apply installs them",
            &[("user", name)],
        )),
        _ => None,
    }
}

/// The file to apply or diff, read leniently, with its problems and the
/// keys this release skips printed first; `None` when there is none.
fn load(file: Option<&Path>, seed_if_missing: bool) -> Result<Option<settings::Read>> {
    let machine = machine_file();
    let path = file.unwrap_or(&machine);
    if file.is_none() && !path.exists() {
        match seed_if_missing.then(|| seed(path)).transpose()?.flatten() {
            Some(from) => println!(
                "edel settings: {}",
                trf(
                    "seeded {path} from {from}",
                    &[("path", &path.display().to_string()), ("from", &from)]
                )
            ),
            None => {
                println!(
                    "edel settings: {}",
                    tr("no settings file, so nothing to apply")
                );
                return Ok(None);
            }
        }
    }
    let read = settings::read_on_machine(path)?;
    for problem in &read.problems {
        println!(
            "edel settings: {}",
            trf("left out {problem}", &[("problem", &problem.to_string())])
        );
    }
    for key in &read.later {
        println!(
            "edel settings: {}",
            trf("skipped {key}: not supported yet", &[("key", key.as_str())])
        );
    }
    Ok(Some(read))
}

/// Gives the machine's settings file its name when it still has a former
/// one (`edel::places::FORMER_SETTINGS`), with the `.v<N>` beside it, and
/// makes `/etc/edel/`'s link to it, where admins look first.
fn settle_names() -> Result<()> {
    let machine = places::machine_settings();
    let found = machine_file();
    if found != machine {
        for format in 1..=settings::FORMAT {
            let old = settings::versioned(&found, format);
            if old.exists() {
                fs::rename(&old, settings::versioned(&machine, format))?;
            }
        }
        fs::rename(&found, &machine).with_context(|| {
            trf(
                "could not rename {from} to {to}",
                &[
                    ("from", &found.display().to_string()),
                    ("to", &machine.display().to_string()),
                ],
            )
        })?;
        println!(
            "edel settings: {}",
            trf(
                "renamed {from} to {to}",
                &[
                    ("from", &found.display().to_string()),
                    ("to", &machine.display().to_string())
                ]
            )
        );
    }
    let link = places::etc_settings();
    if fs::read_link(&link).ok().as_deref() != Some(machine.as_path()) {
        fs::create_dir_all(places::ETC_DIR)?;
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&machine, &link).with_context(|| {
            trf(
                "could not link {link} to {to}",
                &[
                    ("link", &link.display().to_string()),
                    ("to", &machine.display().to_string()),
                ],
            )
        })?;
        println!(
            "edel settings: {}",
            trf(
                "linked {link} to {to}",
                &[
                    ("link", &link.display().to_string()),
                    ("to", &machine.display().to_string())
                ]
            )
        );
    }
    Ok(())
}

/// `edel settings apply`: applies the machine's own settings file, which
/// is seeded first when it is missing.
pub fn apply(boot: bool) -> Result<()> {
    apply_file(None, boot)
}

/// `edel settings import FILE`: applies FILE, which then becomes the
/// machine's settings file, so the next boot keeps it.
pub fn import(file: &Path) -> Result<()> {
    apply_file(Some(file), false)
}

fn apply_file(file: Option<&Path>, boot: bool) -> Result<()> {
    let _lock = Lock::take("settings", "edel settings")?;
    // A file by a former name is renamed first, so it is read and kept by
    // the name this release writes; a failure here never stops the apply.
    if let Err(err) = settle_names() {
        eprintln!("warning: {err:#}");
    }
    let Some(read) = load(file, true)? else {
        return Ok(());
    };
    let (changes, notes) = plan(&read.file, &Machine::read(&read.file)?);
    for note in &notes {
        println!("edel settings: {note}");
    }
    // Root's ssh keys: an import compares the two files, apply the plan (M6.5).
    let root_keys = match file {
        Some(path) => privilege::root_keys_change(
            &fs::read_to_string(machine_file()).unwrap_or_default(),
            &fs::read_to_string(path).unwrap_or_default(),
        ),
        None => changes
            .iter()
            .any(|c| matches!(c, Change::Keys { name, .. } if name == "root")),
    };
    if root_keys {
        privilege::confirm("changing root's ssh keys")?;
    }
    // One change that fails never stops the others (Reliable): each is
    // reported, and apply fails at the end.
    let mut failed = 0;
    for change in &changes {
        if let Some(note) = deferred(change, boot) {
            println!("edel settings: {note}");
            continue;
        }
        match execute(change, boot) {
            Ok(()) => println!("edel settings: {change}"),
            Err(err) => {
                failed += 1;
                eprintln!(
                    "edel settings: {}",
                    trf(
                        "could not apply {change}: {why}",
                        &[
                            ("change", &change.to_string()),
                            ("why", &format!("{err:#}"))
                        ]
                    )
                );
            }
        }
    }
    let machine = places::machine_settings();
    if let Some(path) = file.filter(|f| !same_file(f, &machine)) {
        keep_as_machine_file(path, &machine)?;
        println!(
            "edel settings: {}",
            trf(
                "{path} is now this machine's settings file",
                &[("path", &path.display().to_string())]
            )
        );
    } else if changes.is_empty() {
        println!("edel settings: {}", tr("nothing to change"));
    }
    if failed > 0 {
        bail!(
            "{}",
            trf(
                "{failed} of {total} changes could not be applied, listed above; the others were applied, and edel settings diff shows what is left",
                &[
                    ("failed", &failed.to_string()),
                    ("total", &changes.len().to_string())
                ]
            )
        );
    }
    Ok(())
}

/// Whether `a` and `b` name the same file, however they are spelled.
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Writes `text` to `path` through a new file and a rename, so a power cut
/// leaves the old file or the new one, never an empty one.
fn write_whole(path: &Path, text: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap_or(Path::new("/")))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    let mut out = fs::File::create(&new).with_context(|| could_not_write(&new))?;
    out.write_all(text)?;
    out.sync_all()?;
    fs::rename(&new, path).with_context(|| could_not_write(path))?;
    Ok(())
}

/// Makes the applied `from` this machine's file `to`. A file in a newer
/// format brings the `.v<N>` this release read, so the next boot reads it
/// too (ADR-008).
fn keep_as_machine_file(from: &Path, to: &Path) -> Result<()> {
    let text = fs::read(from).with_context(|| could_not_read(from))?;
    if settings::format(&String::from_utf8_lossy(&text)).is_ok_and(|f| f > settings::FORMAT) {
        let older = settings::versioned(from, settings::FORMAT);
        let older_text = fs::read(&older).with_context(|| could_not_read(&older))?;
        write_whole(&settings::versioned(to, settings::FORMAT), &older_text)?;
    }
    write_whole(to, &text).with_context(|| {
        trf(
            "could not save {path}",
            &[("path", &to.display().to_string())],
        )
    })
}

/// `edel settings diff [FILE]`: what apply would change, one `change:` line
/// each, changing nothing. Returns whether there is anything to change.
pub fn diff(file: Option<&Path>) -> Result<bool> {
    let Some(read) = load(file, false)? else {
        return Ok(false);
    };
    let (changes, notes) = plan(&read.file, &Machine::read(&read.file)?);
    for note in &notes {
        println!("edel settings: {note}");
    }
    for change in &changes {
        println!("change: {change}");
    }
    Ok(!changes.is_empty())
}

/// Edits the machine's settings file through `change`, or for a file in a
/// newer format the `.v<N>` beside it this release reads (ADR-008,
/// writers). Never applies it.
fn edit(what: &str, keys: &[&str], change: impl Fn(&str) -> Result<String>) -> Result<()> {
    edit_as("edel settings", what, keys, change)
}

/// [`edit`] for the command `command` (such as `edel theme`), which its
/// messages name, as the command the person ran is named (MESSAGES.md).
fn edit_as(
    command: &str,
    what: &str,
    keys: &[&str],
    change: impl Fn(&str) -> Result<String>,
) -> Result<()> {
    let written = write_edit(command, keys, change)?;
    report_written(command, what, &written);
    Ok(())
}

/// What [`write_edit`] wrote: the machine's file, the file it wrote (a newer
/// format's `.v<N>` beside it when there is one), whether the desktop
/// follows it at once, and the newer format a file had, if any.
struct Written {
    machine: String,
    path: PathBuf,
    desktop: bool,
    newer: Option<i64>,
}

/// Says what was written, one line, and the newer-format note when there is one.
fn report_written(command: &str, what: &str, written: &Written) {
    println!(
        "{command}: {}",
        trf(
            "{what} in {path}; {then}",
            &[
                ("what", what),
                ("path", &written.path.display().to_string()),
                (
                    "then",
                    if written.desktop {
                        tr("the desktop follows it at once")
                    } else {
                        tr("edel settings apply applies it")
                    }
                )
            ]
        )
    );
    note_newer(command, written);
}

/// Says that the machine's file is a newer format and the change went to
/// the `.v<N>` this release reads, when it did.
fn note_newer(command: &str, written: &Written) {
    if let Some(format) = written.newer {
        println!(
            "{command}: {}",
            trf(
                "{path} is format {format}, so the change applies to this release only",
                &[("path", &written.machine), ("format", &format.to_string())]
            )
        );
    }
}

/// Changes the machine's settings file as `change` makes it from its text,
/// through a file beside it that is renamed over it, under the settings
/// lock; the caller says what was written.
fn write_edit(
    command: &str,
    keys: &[&str],
    change: impl Fn(&str) -> Result<String>,
) -> Result<Written> {
    let _lock = Lock::take("settings", command)?;
    write_edit_at(command, &machine_file(), keys, change)
}

/// [`write_edit`] for the settings file `machine`, the machine's or a
/// person's own (M6.11). A person's takes no lock: the lock lives in
/// `/run/edel`, which only root writes; the write is the same rename.
fn write_edit_at(
    command: &str,
    machine: &Path,
    keys: &[&str],
    change: impl Fn(&str) -> Result<String>,
) -> Result<Written> {
    let shown = machine.display().to_string();
    let mut path = machine.to_path_buf();
    let mut newer = None;
    if let Ok(text) = fs::read_to_string(machine) {
        let format = settings::format(&text).with_context(|| could_not_read(machine))?;
        if format > settings::FORMAT {
            path = settings::versioned(machine, settings::FORMAT);
            newer = Some(format);
        }
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) if newer.is_none() => format!("format = {}\n", settings::FORMAT),
        Err(_) => bail!(
            "{}",
            trf(
                "{path} is format {format}, newer than this release, and there is no {older} beside it to change",
                &[
                    ("path", &shown),
                    ("format", &newer.unwrap_or_default().to_string()),
                    ("older", &path.display().to_string())
                ]
            )
        ),
    };
    let edited = change(&text)?;
    // A change of root's ssh keys is the one a person must confirm (M6.5).
    if privilege::root_keys_change(&text, &edited) {
        privilege::confirm("changing root's ssh keys")?;
    }
    for problem in settings::read(&edited)?.problems {
        println!(
            "{command}: {}",
            trf(
                "kept, not used by this release: {problem}",
                &[("problem", &problem.to_string())]
            )
        );
    }
    let doing = could_not_write(&path);
    fs::create_dir_all(machine.parent().unwrap_or(Path::new("/")))
        .map_err(|err| io_failure(err, &doing))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, &edited).map_err(|err| io_failure(err, &doing))?;
    fs::File::open(&new)
        .and_then(|file| file.sync_all())
        .map_err(|err| io_failure(err, &doing))?;
    fs::rename(&new, &path).map_err(|err| io_failure(err, &doing))?;
    let desktop = keys.iter().all(|k| desktop_follows(k));
    Ok(Written {
        machine: shown,
        path,
        desktop,
        newer,
    })
}

/// `edel settings set KEY=VALUE...`: every assignment is checked before
/// any is written, and all are written at once, or none.
pub fn set(assignments: &[String]) -> Result<()> {
    let mut pairs = Vec::new();
    for assignment in assignments {
        pairs.push(split_assignment(assignment)?);
    }
    // A feature this machine cannot switch is refused before anything is
    // written (M6.10); a build host with no features checks nothing here.
    let installed = services::installed();
    if !installed.is_empty() {
        let wanted: BTreeMap<String, bool> = pairs
            .iter()
            .filter_map(|(k, _, _)| k.strip_prefix("services."))
            .map(|name| (name.to_string(), true))
            .collect();
        if let Some(problem) = services::problems(&wanted, &installed).into_iter().next() {
            bail!("{problem}");
        }
    }
    let keys: Vec<&str> = pairs.iter().map(|(k, _, _)| *k).collect();
    let what = trf("set {keys}", &[("keys", &keys.join(", "))]);
    let typed = format!("set {}", assignments.join(" "));
    let change = |text: &str| -> Result<String> {
        let mut text = text.to_string();
        for (key, how, value) in &pairs {
            text = match how {
                Assign::Set => settings::set(&text, key, value)?,
                Assign::Add => settings::edit_list(&text, key, value, true)?,
                Assign::Remove => settings::edit_list(&text, key, value, false)?,
            };
        }
        Ok(text)
    };
    match target_for(is_root(), &keys, &typed)? {
        Target::Machine => edit(&what, &keys, change),
        Target::Person => edit_person(&what, &keys, change),
    }
}

/// What one assignment of `edel settings set` does to its key (M6.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assign {
    /// `KEY=VALUE`
    Set,
    /// `KEY+=VALUE`: adds VALUE to a list
    Add,
    /// `KEY-=VALUE`: removes VALUE from a list
    Remove,
}

/// Splits `KEY=VALUE`, `KEY+=VALUE` or `KEY-=VALUE` (M6.11) into the key, what
/// the assignment does and the value.
fn split_assignment(assignment: &str) -> Result<(&str, Assign, &str)> {
    let (left, value) = assignment.split_once('=').with_context(|| {
        trf(
            "{assignment}: write KEY=VALUE, such as network.hostname=lab-1",
            &[("assignment", &format!("{assignment:?}"))],
        )
    })?;
    let left = left.trim();
    let (key, how) = if let Some(key) = left.strip_suffix('+') {
        (key, Assign::Add)
    } else if let Some(key) = left.strip_suffix('-') {
        (key, Assign::Remove)
    } else {
        (left, Assign::Set)
    };
    Ok((key.trim(), how, value.trim()))
}

/// Whether this process runs as root: the effective user, as
/// `users::current` reads it (M6.11).
fn is_root() -> bool {
    edel::users::current() == Some(0)
}

/// Where `edel settings set` and `reset` write (M6.11). Root changes the
/// machine's file, as it always has. Anyone else changes their own file,
/// for the keys the desktop follows; a machine key needs an administrator,
/// and the refusal repeats what they typed (`command`, after `edel settings`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Machine,
    Person,
}

fn target_for(is_root: bool, keys: &[&str], command: &str) -> Result<Target> {
    if is_root {
        return Ok(Target::Machine);
    }
    let machine_keys: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|key| !desktop_follows(key))
        .collect();
    if machine_keys.is_empty() {
        return Ok(Target::Person);
    }
    bail!(
        "{}",
        trf(
            "{keys}: these are the machine's settings, so changing them needs an administrator: {as_root} edel settings {command}",
            &[
                ("keys", &machine_keys.join(", ")),
                ("as_root", places::AS_ROOT),
                ("command", command)
            ]
        )
    );
}

/// [`edit`] for the person's own settings file (M6.11), which the desktop
/// reads over the machine's for the keys it follows.
fn edit_person(what: &str, keys: &[&str], change: impl Fn(&str) -> Result<String>) -> Result<()> {
    let file = places::person_settings().with_context(|| {
        tr("HOME is not set, so there is no folder for the person's settings file; set HOME and try again")
    })?;
    let written = write_edit_at("edel settings", &places::found(&file), keys, change)?;
    report_written("edel settings", what, &written);
    Ok(())
}

/// Whether the desktop follows `key` at once, reading the person's own
/// file over the machine's, as it does the layout, screen, look and
/// shortcut keys (M4.5, M4.6, M5.5c, M5.13a), the apps the apps widget pins
/// (M5.31d), notifications (M5.9b), default and startup apps (M6.10), the
/// language, keyboard and input method (M5.24a, M5.21a), and own themes and
/// presets; `edel settings apply` applies the rest, from the machine's file.
fn desktop_follows(key: &str) -> bool {
    key == panel_edit::PINNED
        || edel::app_permissions::find(key).is_some()
        || ["region.language", "region.keyboard", "region.input_method"].contains(&key)
        || [
            "layout.",
            "panels.",
            "workspaces.",
            "displays.",
            "appearance.",
            "lock_screen.",
            "shortcuts.",
            "notifications.",
            "default_apps.",
            "startup.",
            "themes.",
            "presets.",
        ]
        .iter()
        .any(|section| key.starts_with(section))
}

/// `edel settings reset KEY...`: removes each key the file holds, so the
/// release decides it again; refused only when none is there.
pub fn reset(keys: &[String]) -> Result<()> {
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let what = trf("reset {keys}", &[("keys", &keys.join(", "))]);
    let typed = format!("reset {}", keys.join(" "));
    let change = |text: &str| settings::unset_all(text, &keys);
    match target_for(is_root(), &keys, &typed)? {
        Target::Machine => edit(&what, &keys, change),
        Target::Person => edit_person(&what, &keys, change),
    }
}

/// The machine's settings file as it is, or the release's empty one, for
/// the theme and preset commands to read.
fn theme_text() -> String {
    fs::read_to_string(machine_file())
        .unwrap_or_else(|_| format!("format = {}\n", settings::FORMAT))
}

/// `edel theme list`: the built-in themes, then the person's own, one a
/// line with its name, its title and what it is (`edel theme` in the
/// command table).
pub fn theme_list() -> Result<()> {
    let text = theme_text();
    let file = settings::read(&text)
        .with_context(|| could_not_read(&machine_file()))?
        .file;
    for row in themes::rows(&file) {
        let mut marks = Vec::new();
        if row.built_in {
            marks.push(tr("(built in)"));
        }
        if row.in_use {
            marks.push(tr("(in use)"));
        }
        let line = format!("{:<10} {:<16} {}", row.name, row.title, marks.join(" "));
        println!("{}", line.trim_end());
    }
    Ok(())
}

/// `edel theme save NAME`: keeps the theme in use as a theme of the
/// person's own, one write, and the screen stays the same.
pub fn theme_save(name: &str) -> Result<()> {
    let what = trf("saved the theme {name}", &[("name", name)]);
    edit_as("edel theme", &what, &["appearance.theme"], |text| {
        themes::save_text(text, name)
    })
}

/// `edel theme delete NAME`: removes one of the person's own themes, unless
/// it is the theme in use.
pub fn theme_delete(name: &str) -> Result<()> {
    let what = trf("deleted the theme {name}", &[("name", name)]);
    edit_as("edel theme", &what, &["appearance.theme"], |text| {
        themes::delete_text(text, name)
    })
}

/// `edel theme reset`: takes the person's own changes off the theme in use,
/// so it shows as it was made; says so and writes nothing when there are
/// none.
pub fn theme_reset() -> Result<()> {
    let text = theme_text();
    if themes::reset_text(&text)?.is_none() {
        println!(
            "edel theme: {}",
            tr("nothing to reset; the theme in use shows as it was made")
        );
        return Ok(());
    }
    let what = tr("reset the theme in use");
    edit_as("edel theme", what, &["appearance.theme"], |text| {
        Ok(themes::reset_text(text)?.unwrap_or_else(|| text.to_string()))
    })
}

/// `edel theme export NAME [FILE]`: writes the theme NAME, built in or the
/// person's own, as one `.edeltheme` file with its pictures in it, NAME.edeltheme
/// in this folder when no FILE is given. A file that is there is never
/// written over, and a theme that would not read back is not written at all.
pub fn theme_export(name: &str, file: Option<&Path>) -> Result<()> {
    let text = theme_text();
    let read = settings::read(&text)
        .with_context(|| could_not_read(&machine_file()))?
        .file;
    let Some(theme) = themes::lookup(name, &read.themes) else {
        bail!(
            "{}",
            trf(
                "there is no theme called {name}; edel theme list shows them",
                &[("name", name)]
            )
        );
    };
    let target = file
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{name}.{}", theme_file::EXTENSION)));
    let shown = target.display().to_string();
    theme_file::export(name, &theme, &target)?;
    println!(
        "edel theme: {}",
        trf(
            "exported the theme {name} to {file}",
            &[("name", name), ("file", &shown)]
        )
    );
    Ok(())
}

/// `edel theme import FILE`: checks a `.edeltheme` file, which writes nothing
/// when it is refused, copies its pictures into the machine's themes folder
/// (`places::MACHINE_THEMES_DIR`, as the theme goes into the machine's
/// settings file, which every person reads) under the theme's name, and adds
/// the theme to the settings file. The
/// theme in use stays as it is: the line it prints says how to choose it.
pub fn theme_import(file: &Path) -> Result<()> {
    let theme = theme_file::read(file)?;
    // The plan checks the theme against the file first, so a refused theme
    // writes no picture; the pictures come before the settings file names them.
    let plan = theme_file::plan(theme, &theme_text(), Path::new(places::MACHINE_THEMES_DIR))?;
    plan.install()?;
    let written = write_edit("edel theme", &[], |text| plan.add(text))?;
    println!("{}", import_line(&plan.title, &plan.name, plan.pictures));
    note_newer("edel theme", &written);
    Ok(())
}

/// What `edel theme import` prints when a theme is added (its pictures
/// counted in words, and how to choose it).
fn import_line(title: &str, name: &str, pictures: usize) -> String {
    if pictures == 1 {
        trf(
            "Added theme {title} as {name}, with 1 picture. Choose it with: edel settings set appearance.theme={name}",
            &[("title", title), ("name", name)],
        )
    } else {
        trf(
            "Added theme {title} as {name}, with {count} pictures. Choose it with: edel settings set appearance.theme={name}",
            &[
                ("title", title),
                ("name", name),
                ("count", &pictures.to_string()),
            ],
        )
    }
}

/// `edel preset list`: the built-in presets, then the person's own, one a
/// line with its name, its title, its base and what it is (`edel preset` in
/// the command table).
pub fn preset_list() -> Result<()> {
    let file = settings::read(&theme_text())
        .with_context(|| could_not_read(&machine_file()))?
        .file;
    for row in presets::rows(&file) {
        let mut marks = Vec::new();
        if row.built_in {
            marks.push(tr("(built in)"));
        }
        if row.in_use {
            marks.push(tr("(in use)"));
        }
        let line = format!("{:<12} {:<16} {}", row.name, row.title, marks.join(" "));
        println!("{}", line.trim_end());
    }
    Ok(())
}

/// `edel preset save NAME`: keeps the layout in use as a preset of the
/// person's own, one write, and the screen stays the same.
pub fn preset_save(name: &str) -> Result<()> {
    let what = trf("saved the preset {name}", &[("name", name)]);
    edit_as("edel preset", &what, &["layout.preset"], |text| {
        presets::save_text(text, name)
    })
}

/// `edel preset delete NAME`: removes one of the person's own presets, unless
/// it is the preset in use.
pub fn preset_delete(name: &str) -> Result<()> {
    let what = trf("deleted the preset {name}", &[("name", name)]);
    edit_as("edel preset", &what, &["layout.preset"], |text| {
        presets::delete_text(text, name)
    })
}

/// A page's name as people type it: its title in lowercase, `_` for a
/// space, such as `default_apps` (ADR-008's same names decision).
fn page_word(page: &settings::Page) -> String {
    page.title.to_lowercase().replace(' ', "_").replace('-', "")
}

/// `edel settings` alone: the pages under their groups, in the Settings
/// app's order (M5.32b).
pub fn pages() {
    print!("{}", pages_text());
}

/// What `edel settings` prints: each group's title alone on a line, its
/// pages under it, and a blank line between groups (the groups are
/// `settings::GROUPS`, as the sidebar shows them).
fn pages_text() -> String {
    let mut text = format!(
        "{}\n\n",
        tr("Settings, page by page, as the Settings app shows them:")
    );
    let width = settings::PAGES
        .iter()
        .map(|p| page_word(p).len())
        .max()
        .unwrap_or(0);
    let mut shown = 0;
    for group in settings::GROUPS {
        let pages: Vec<&settings::Page> = settings::PAGES
            .iter()
            .filter(|p| p.group == group.name)
            .collect();
        if pages.is_empty() {
            continue;
        }
        if shown > 0 {
            text.push('\n');
        }
        shown += 1;
        text.push_str(&format!("{}\n", tr(group.title)));
        for page in pages {
            text.push_str(&format!(
                "  {:width$}  {}: {}\n",
                page_word(page),
                tr(page.title),
                tr(page.about)
            ));
        }
    }
    text.push_str(&format!(
        "\n{}\n",
        tr(
            "edel settings get PAGE shows a page's settings and where each comes from;\nedel settings set KEY=VALUE changes one, and reset KEY gives it back to the release."
        )
    ));
    text
}

/// Every value in `table`, by its dotted key; a list is one value.
fn flatten(table: &toml::Table, prefix: &str, out: &mut Vec<(String, toml::Value)>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => flatten(inner, &key, out),
            _ if key == "format" => {}
            _ => out.push((key, value.clone())),
        }
    }
}

/// The values a settings file at `path` sets, by key; none when there is
/// no file or it cannot be read.
fn values_in(path: Option<PathBuf>) -> Vec<(String, toml::Value)> {
    let mut out = Vec::new();
    let file = path
        .filter(|p| p.exists())
        .and_then(|p| settings::read_on_machine(&p).ok());
    if let Some(toml::Value::Table(table)) = file.and_then(|r| toml::Value::try_from(r.file).ok()) {
        flatten(&table, "", &mut out);
    }
    out
}

/// One line of `edel settings get` (M6.11): a key, its value and where the
/// value comes from. A key no file sets shows the default that applies.
struct Row {
    key: String,
    value: toml::Value,
    /// Where the value comes from, in words; empty when no line says, so the
    /// key is shown as not set.
    from: String,
    /// The default that applies, as its key's line writes it, for a key no
    /// file sets.
    default: Option<String>,
}

/// The layout preset in use: the person's `layout.preset`, else the
/// machine's, else the release's (M6.11).
fn preset_in_use(person: &[(String, toml::Value)], machine: &[(String, toml::Value)]) -> String {
    person
        .iter()
        .chain(machine.iter())
        .find(|(k, _)| k.as_str() == "layout.preset")
        .and_then(|(_, v)| v.as_str())
        .unwrap_or(presets::DEFAULT)
        .to_string()
}

/// Where a key's default comes from, in words, for `edel settings get` (M6.11):
/// the preset in use for a preset's key, else the layer that decides it. Empty
/// when there is no line for the key.
fn layer_label(line: Option<&key_help::Help>, preset: &str) -> String {
    let Some(line) = line else {
        return String::new();
    };
    match line.from {
        DecidedBy::Preset => trf("preset {name}", &[("name", preset)]),
        from => trf("{word} default", &[("word", from.word())]),
    }
}

/// `edel settings get [KEY|PAGE] [--toml]`: the settings with their
/// values and where each comes from: a person's own file for what the
/// desktop reads, then the machine's, else the release's default.
pub fn get(what: Option<&str>, as_toml: bool) -> Result<()> {
    let machine = values_in(Some(machine_file()));
    let person: Vec<(String, toml::Value)> =
        values_in(places::person_settings().map(|p| places::found(&p)))
            .into_iter()
            .filter(|(k, _)| desktop_follows(k))
            .collect();
    // The section asked for, by its page's word or its own name, or a key.
    let (section, key) = match what {
        None => (None, None),
        Some(w) => match settings::PAGES
            .iter()
            .find(|p| page_word(p) == w || p.section == w)
        {
            Some(page) => (Some(page.section), None),
            None if settings::KEYS.iter().any(|k| key_fits(k.path, w)) => (None, Some(w)),
            None if machine
                .iter()
                .chain(&person)
                .any(|(k, _)| k.starts_with(&format!("{w}."))) =>
            {
                (None, Some(w))
            }
            None => match settings::nearest_key(w) {
                Some(near) => bail!(
                    "{}",
                    trf(
                        "{name}: no such page or key; did you mean {near}?",
                        &[("name", w), ("near", &near)]
                    )
                ),
                None => bail!(
                    "{}",
                    trf(
                        "{name}: no such page or key; edel settings lists the pages",
                        &[("name", w)]
                    )
                ),
            },
        },
    };
    let wanted = |k: &str| match (section, key) {
        (Some(s), _) => k.starts_with(&format!("{s}.")),
        (_, Some(key)) => k == key || k.starts_with(&format!("{key}.")),
        _ => true,
    };
    // The person's value wins over the machine's for the same key.
    let mut rows: Vec<Row> = Vec::new();
    for (k, v) in &person {
        if wanted(k) {
            rows.push(Row {
                key: k.clone(),
                value: v.clone(),
                from: tr("your own file").to_string(),
                default: None,
            });
        }
    }
    for (k, v) in &machine {
        if wanted(k) && !rows.iter().any(|row| &row.key == k) {
            rows.push(Row {
                key: k.clone(),
                value: v.clone(),
                from: tr("this machine").to_string(),
                default: None,
            });
        }
    }
    if as_toml {
        let mut table = toml::Table::new();
        for row in &rows {
            insert_path(&mut table, &row.key, row.value.clone());
        }
        print!("{}", toml::to_string(&table)?);
        return Ok(());
    }
    // The keys a page has that nobody set: the value that applies and the
    // layer that decides it (M6.11), or nothing when no line says so.
    let preset = preset_in_use(&person, &machine);
    for entry in settings::KEYS.iter().filter(|k| !k.path.contains('*')) {
        if wanted(entry.path) && !rows.iter().any(|row| row.key == entry.path) {
            let line = key_help::find(entry.path);
            rows.push(Row {
                key: entry.path.to_string(),
                value: toml::Value::String(String::new()),
                from: layer_label(line, &preset),
                default: line.map(|line| {
                    // A preset's key shows the preset's own value (M6.11).
                    match line.from {
                        DecidedBy::Preset => {
                            presets::value_of(entry.path, &presets::named(Some(&preset)).0)
                                .unwrap_or_else(|| line.default.to_string())
                        }
                        _ => line.default.to_string(),
                    }
                }),
            });
        }
    }
    let width = rows.iter().map(|row| row.key.len()).max().unwrap_or(0);
    for page in settings::PAGES {
        let mut mine: Vec<&Row> = rows
            .iter()
            .filter(|row| row.key.split('.').next() == Some(page.section))
            .collect();
        if mine.is_empty() {
            // A page asked for by name that holds nothing in the file, such
            // as Sound, whose state is the sound system's own (M5.7b).
            if section == Some(page.section) && settings::no_keys(page.section) {
                println!(
                    "{}",
                    trf(
                        "{page} has no settings in the settings file, because it shows what the machine is doing now; open it in Settings, where each row can copy the command that does the same",
                        &[("page", tr(page.title))],
                    )
                );
            }
            continue;
        }
        mine.sort_by(|a, b| (a.from.is_empty(), &a.key).cmp(&(b.from.is_empty(), &b.key)));
        println!(
            "{} ({}): {}",
            tr(page.title),
            page_word(page),
            tr(page.about)
        );
        for row in mine {
            let later = settings::KEYS
                .iter()
                .any(|e| !e.supported && key_fits(e.path, &row.key));
            let shown = if row.from.is_empty() {
                tr("not set: the release decides").to_string()
            } else if let Some(default) = &row.default {
                format!("{default}  ({})", row.from)
            } else {
                format!("{}  ({})", row.value, row.from)
            };
            let later = if later {
                format!("  ({})", tr("not supported yet"))
            } else {
                String::new()
            };
            println!("  {:width$}  {shown}{later}", row.key);
            // The one key asked for says what it is, on the line below (M6.11).
            if key == Some(row.key.as_str()) {
                if let Some(line) = key_help::find(&row.key) {
                    println!("    {}", tr(line.help));
                }
            }
        }
        // Network's connections are not keys of the file either (M5.8a).
        if section == Some(page.section) && page.section == "network" {
            println!(
                "  {}",
                tr(
                    "the wired and Wi-Fi connections and their passwords are NetworkManager's own, kept by this machine and never in this file; open Network in Settings, where each row can copy the nmcli command that does the same"
                )
            );
        }
        println!();
    }
    Ok(())
}

/// Whether `key` is `pattern`, where `*` in the pattern stands for a name.
fn key_fits(pattern: &str, key: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (pattern.split('.').collect(), key.split('.').collect());
    a.len() == b.len() && a.iter().zip(&b).all(|(p, k)| *p == "*" || p == k)
}

/// Puts `value` at the dotted `key` in `table`, making tables on the way.
fn insert_path(table: &mut toml::Table, key: &str, value: toml::Value) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let Some(last) = parts.pop() else {
        return;
    };
    let mut at = table;
    for part in parts {
        let entry = at
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(inner) = entry else {
            return;
        };
        at = inner;
    }
    at.insert(last.to_string(), value);
}

/// Looks for a first settings file: on a volume labelled EDEL-SEED, then on
/// this disk's EFI system partition, then in the slot. Copies the first one
/// this release can read to `target` and says where it came from; one it
/// cannot read is reported and passed over, so a fixed seed is read at the
/// next boot instead of a broken copy staying for good. The partition is
/// found from the running disk, not by label: an installer stick and the
/// disk it installed both have an EDEL-ESP.
fn seed(target: &Path) -> Result<Option<String>> {
    let esp_dir = Path::new(GRUB_PREFIX.trim_start_matches('/'));
    // The first source that holds a file this release can read wins.
    let adopt = |text: Option<String>, from: &str| -> Result<Option<String>> {
        let Some(text) = text else {
            return Ok(None);
        };
        if let Err(err) = settings::read(&text) {
            println!(
                "edel settings: {}",
                trf(
                    "passed over the settings file on {from}: {why}",
                    &[("from", from), ("why", &format!("{err:#}"))]
                )
            );
            return Ok(None);
        }
        write_whole(target, text.as_bytes())?;
        Ok(Some(from.to_string()))
    };
    let volume =
        find_by_label(SEED_LABEL).and_then(|device| read_from(&device, Path::new(""), None));
    if let Some(from) = adopt(volume, &format!("the {SEED_LABEL} volume"))? {
        return Ok(Some(from));
    }
    let esp = Disk::find()
        .and_then(|disk| disk.device(ESP_PARTITION))
        .ok()
        .and_then(|device| read_from(&device, esp_dir, Some("vfat")));
    if let Some(from) = adopt(esp, "the EFI system partition")? {
        return Ok(Some(from));
    }
    let slot = places::found(&places::slot_settings());
    adopt(fs::read_to_string(&slot).ok(), &slot.display().to_string())
}

/// The device of the file system labelled `label`, if one is attached.
fn find_by_label(label: &str) -> Option<PathBuf> {
    let out = Command::new("findfs")
        .arg(format!("LABEL={label}"))
        .output()
        .ok()?;
    let device = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !device.is_empty()).then(|| PathBuf::from(device))
}

/// The settings file in the directory `inner` on `device`, by its name or
/// a former one, mounted read-only for as long as it takes to read it. `fs`
/// names the file system when it is known: early in boot the FAT driver is
/// not loaded yet, so mount cannot guess it.
fn read_from(device: &Path, inner: &Path, fs: Option<&str>) -> Option<String> {
    let Some(text) = with_mounted(device, fs, false, |dir| {
        fs::read_to_string(places::found(&places::settings_in(&dir.join(inner)))).ok()
    }) else {
        eprintln!(
            "warning: {}",
            trf(
                "cannot mount {device} to look for a settings file",
                &[("device", &device.display().to_string())]
            )
        );
        return None;
    };
    text
}

/// Runs `read` on `device` mounted read-only at `places::SEED_MOUNT`, then
/// unmounts it; None when it does not mount, else what `read` gave. FAT needs a charset the virt
/// kernel has, and other file systems refuse the option, so they get a
/// second try without it; `quiet` keeps mount's own complaints off the
/// console when many devices are tried.
pub(crate) fn with_mounted<T>(
    device: &Path,
    fs: Option<&str>,
    quiet: bool,
    read: impl FnOnce(&Path) -> T,
) -> Option<T> {
    let dir = Path::new(places::SEED_MOUNT);
    std::fs::create_dir_all(dir).ok()?;
    let mount = |kind: &str, options: &str| {
        let mut cmd = Command::new("mount");
        cmd.args(["-t", kind, "-o", options]).arg(device).arg(dir);
        if quiet {
            cmd.stderr(std::process::Stdio::null());
        }
        cmd.status().is_ok_and(|s| s.success())
    };
    let mounted = match fs {
        Some(kind) => mount(kind, "ro,iocharset=iso8859-1"),
        None => mount("vfat", "ro,iocharset=iso8859-1") || mount("auto", "ro"),
    };
    if !mounted {
        return None;
    }
    let result = read(dir);
    let _ = Command::new("umount").arg(dir).status();
    Some(result)
}

/// What `export` begins the file with: what it is, and how to use it.
pub const EXPORT_HEADER: &str = "\
# Edel OS settings: this machine as one file (ADR-006). Every line is a
# setting the Settings app shows; a line that is missing means the
# release's default. To set up another machine the same way, run
#   edel settings import THIS-FILE
# on it, or give it to the installer with edel install DISK --settings
# THIS-FILE. It never holds passwords or personal files.
";

/// `edel settings export`: prints this machine as a settings file. It
/// starts from the machine's file and replaces what apply owns with what
/// the machine has, writing no defaults (ADR-008).
pub fn export() -> Result<()> {
    let machine = machine_file();
    let mut file = match settings::read_on_machine(&machine) {
        Ok(read) => read.file,
        Err(_) if !machine.exists() => SettingsFile::default(),
        Err(err) => {
            eprintln!(
                "warning: {}",
                trf(
                    "{why}; exporting only what the machine has",
                    &[("why", &format!("{err:#}"))]
                )
            );
            SettingsFile::default()
        }
    };
    let hostname = Path::new(ETC_UPPER)
        .join("hostname")
        .exists()
        .then(|| fs::read_to_string("/etc/hostname"))
        .transpose()?;
    describe(
        &mut file,
        &fs::read_to_string("/etc/passwd")?,
        &fs::read_to_string("/etc/group")?,
        hostname.as_deref(),
        read_keys,
        Path::new(DEVELOPER_FLAG).exists(),
        default_shell(),
    );
    // The zone this machine's own link names (`region.timezone`).
    file.region.timezone = own_timezone();
    print!("{EXPORT_HEADER}\n{}", toml::to_string(&file)?);
    let changed = changed_files(Path::new(ETC_UPPER), Path::new("/etc"));
    if !changed.is_empty() {
        println!(
            "\n# {}",
            tr("Files this machine changed in /etc, kept on /data and not described above:")
        );
        for line in changed {
            println!("#   {line}");
        }
    }
    Ok(())
}

/// The files under the overlay's upper directory `dir`, named as they
/// appear under `shown`; a whiteout is a file this machine removed.
fn changed_files(dir: &Path, shown: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let (path, name) = (entry.path(), shown.join(entry.file_name()));
        match entry.file_type() {
            Ok(t) if t.is_dir() => found.extend(changed_files(&path, &name)),
            Ok(t) if t.is_char_device() => found.push(format!("{} (removed)", name.display())),
            Ok(_) => found.push(name.display().to_string()),
            Err(_) => {}
        }
    }
    found.sort();
    found
}

/// Sets the keys apply owns in `file` from the machine: the hostname when
/// this machine changed it, every person's account, and developer mode.
fn describe(
    file: &mut SettingsFile,
    passwd: &str,
    group: &str,
    hostname: Option<&str>,
    authorized_keys: impl Fn(&Account) -> Option<String>,
    developer: bool,
    default_shell: &str,
) {
    file.network.hostname = hostname.map(|h| h.trim().to_string());
    file.system.developer_mode = developer.then_some(true);
    let admins = members(group, ADMIN_GROUP).unwrap_or_default();
    file.users.clear();
    for account in accounts(passwd).iter().filter(|a| is_person(a)) {
        let keys: Option<Vec<String>> = authorized_keys(account).map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect()
        });
        let keys = keys.filter(|k| !k.is_empty());
        // root is listed only when it has keys: it exists on every machine.
        if account.uid == 0 && keys.is_none() {
            continue;
        }
        let user = User {
            admin: admins.contains(&account.name).then_some(true),
            ssh_keys: keys,
            login_shell: (account.shell != default_shell).then(|| account.shell.clone()),
        };
        file.users.insert(account.name.clone(), user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static PRESET_LINE: key_help::Help = key_help::Help {
        key: "layout.tiling",
        help: "tiling",
        default: "false",
        from: DecidedBy::Preset,
        belongs: key_help::Belongs::Person,
        retired: None,
    };

    static RELEASE_LINE: key_help::Help = key_help::Help {
        key: "network.hostname",
        help: "the name",
        default: "\"edel\"",
        from: DecidedBy::Release,
        belongs: key_help::Belongs::Device,
        retired: None,
    };

    static HARDWARE_LINE: key_help::Help = key_help::Help {
        key: "displays.main.scale",
        help: "the scale",
        default: "1",
        from: DecidedBy::Hardware,
        belongs: key_help::Belongs::Device,
        retired: None,
    };

    #[test]
    fn get_names_the_layer_that_gives_an_unset_default() {
        assert_eq!(layer_label(None, "classic"), "");
        assert_eq!(layer_label(Some(&PRESET_LINE), "hive"), "preset hive");
        assert_eq!(layer_label(Some(&RELEASE_LINE), "hive"), "release default");
        assert_eq!(
            layer_label(Some(&HARDWARE_LINE), "hive"),
            "hardware default"
        );
    }

    #[test]
    fn the_preset_in_use_is_the_persons_then_the_machines_then_the_release_s() {
        let text = |preset: &str| toml::Value::String(preset.to_string());
        let person = [("layout.preset".to_string(), text("mac-like"))];
        let machine = [("layout.preset".to_string(), text("hive"))];
        assert_eq!(preset_in_use(&person, &machine), "mac-like");
        assert_eq!(preset_in_use(&[], &machine), "hive");
        assert_eq!(preset_in_use(&[], &[]), presets::DEFAULT);
    }

    #[test]
    fn set_plus_and_minus_split_the_key_from_what_it_does() {
        assert_eq!(
            split_assignment("startup.apps+=org.mozilla.firefox").unwrap(),
            ("startup.apps", Assign::Add, "org.mozilla.firefox")
        );
        assert_eq!(
            split_assignment("startup.apps-=a=b").unwrap(),
            ("startup.apps", Assign::Remove, "a=b")
        );
        assert_eq!(
            split_assignment(" network.hostname = lab-1 ").unwrap(),
            ("network.hostname", Assign::Set, "lab-1")
        );
        assert!(split_assignment("network.hostname").is_err());
    }

    #[test]
    fn set_by_a_person_writes_their_own_file_only_for_the_keys_the_desktop_follows() {
        // Root always changes the machine's file, as it did before (M6.11).
        assert_eq!(
            target_for(true, &["network.hostname"], "set network.hostname=lab-1").unwrap(),
            Target::Machine
        );
        assert_eq!(
            target_for(true, &["layout.preset"], "set layout.preset=hive").unwrap(),
            Target::Machine
        );
        // A person's keys are the desktop's: their own file.
        assert_eq!(
            target_for(false, &["layout.preset"], "set layout.preset=hive").unwrap(),
            Target::Person
        );
        assert_eq!(
            target_for(false, &["appearance.mode"], "reset appearance.mode").unwrap(),
            Target::Person
        );
        // A machine key needs an administrator, and the refusal repeats the command.
        let error = target_for(false, &["network.hostname"], "set network.hostname=lab-1")
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!(
                "network.hostname: these are the machine's settings, so changing them needs an administrator: {} edel settings set network.hostname=lab-1",
                places::AS_ROOT
            )
        );
        // A mix of both is refused, naming only the machine's key.
        let mixed = target_for(
            false,
            &["layout.preset", "network.hostname"],
            "set layout.preset=hive network.hostname=lab-1",
        )
        .unwrap_err()
        .to_string();
        assert!(mixed.starts_with("network.hostname: these are"), "{mixed}");
    }

    #[test]
    fn settings_lists_the_five_groups_with_their_pages_under_them() {
        let text = pages_text();
        let lines: Vec<&str> = text.lines().collect();
        let headings: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| !l.starts_with(' ') && !l.is_empty())
            .collect();
        assert_eq!(
            headings,
            [
                "Settings, page by page, as the Settings app shows them:",
                "Desktop",
                "Connections",
                "Devices",
                "Personal",
                "System",
                "edel settings get PAGE shows a page's settings and where each comes from;",
                "edel settings set KEY=VALUE changes one, and reset KEY gives it back to the release.",
            ]
        );
        let under = |heading: &str| -> Vec<String> {
            lines
                .iter()
                .skip_while(|l| **l != heading)
                .skip(1)
                .take_while(|l| l.starts_with("  "))
                .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
                .collect()
        };
        assert_eq!(
            under("Desktop"),
            [
                "appearance",
                "layout",
                "panels",
                "workspaces",
                "lock_screen"
            ]
        );
        assert_eq!(under("Connections"), ["network", "bluetooth"]);
        assert_eq!(under("Devices"), ["displays", "sound", "power"]);
        assert_eq!(
            under("Personal"),
            [
                "notifications",
                "shortcuts",
                "region",
                "default_apps",
                "startup",
                "apps"
            ]
        );
        assert_eq!(
            under("System"),
            ["addons", "services", "users", "updates", "system"]
        );
    }

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\n\
        sshd:x:22:22:sshd:/dev/null:/sbin/nologin\n\
        ci:x:1000:1000:Linux User,,,:/home/ci:/bin/sh\n\
        ali:x:1001:1001:Linux User,,,:/home/ali:/bin/ash\n\
        nobody:x:65534:65534:nobody:/:/sbin/nologin\n";

    #[test]
    fn a_new_user_can_log_in_with_a_key() {
        let shadow =
            "root:*:19000:0:::::\nci:!:20000:0:99999:7:::\nali:$6$salt$hash:20000:0:99999:7:::\n";
        let fixed = shadow_unlocked(shadow, "ci").unwrap();
        assert!(fixed.contains("\nci:*:20000:0:99999:7:::\n"));
        assert_eq!(shadow_unlocked(&fixed, "ci"), None);
        assert_eq!(shadow_unlocked(shadow, "ali"), None);
        assert_eq!(
            shadow_unlocked("x:!!:1::::::\n", "x").unwrap(),
            "x:*:1::::::\n"
        );
    }

    /// An assignment without `=` is refused before anything is read or
    /// written, with the form it should have.
    #[test]
    fn set_without_a_value_says_how_to_write_one() {
        let err = set(&["network.hostname".to_string()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "\"network.hostname\": write KEY=VALUE, such as network.hostname=lab-1"
        );
    }

    #[test]
    fn sets_a_login_shell() {
        let changed = passwd_with_shell(PASSWD, "ali", "/bin/sh").unwrap();
        assert!(changed.contains("ali:x:1001:1001:Linux User,,,:/home/ali:/bin/sh\n"));
        assert_eq!(passwd_with_shell(&changed, "ali", "/bin/sh"), None);
        assert_eq!(passwd_with_shell(PASSWD, "nobody-else", "/bin/sh"), None);
    }

    #[test]
    fn reads_accounts_and_admins() {
        let all = accounts(PASSWD);
        let people: Vec<&str> = all
            .iter()
            .filter(|a| is_person(a))
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(people, ["root", "ci", "ali"]);
        let group = "root:x:0:root\nadmin:x:101:ci,ali\nci:x:1000:\n";
        assert_eq!(members(group, "admin").unwrap(), ["ci", "ali"]);
        assert_eq!(members(group, "ci").unwrap(), Vec::<String>::new());
        assert_eq!(members(group, "wheel"), None);
    }

    fn machine() -> Machine {
        Machine {
            hostname: "lab-1\n".into(),
            hostname_set: true,
            timezone: None,
            passwd: PASSWD.into(),
            group: "admin:x:101:ci\n".into(),
            shadow: Some("ci:*:1::::::\nali:!:1::::::\n".into()),
            keys: BTreeMap::from([("ci".into(), "ssh-ed25519 AAAA ci@edel\n".into())]),
            shells: BTreeSet::from(["/bin/ash".into()]),
            default_shell: DEFAULT_SHELL.into(),
            developer: false,
            flatpak: BTreeMap::new(),
            features: Vec::new(),
            services_off: Vec::new(),
            runlevels: BTreeSet::new(),
            mime_defaults: None,
            first_passwords: None,
        }
    }

    /// The MIME changes `plan` makes for a file's text and the machine's
    /// `mimeapps.list` (M6.10).
    fn mime_changes(text: &str, mime: Option<&str>) -> Vec<Change> {
        let file = settings::read(text).unwrap().file;
        let mut machine = machine();
        machine.mime_defaults = mime.map(str::to_string);
        plan(&file, &machine)
            .0
            .into_iter()
            .filter(|c| matches!(c, Change::MimeDefaults(_)))
            .collect()
    }

    const EDITOR_SET: &str = "format = 1\n[default_apps]\neditor = \"org.gnome.TextEditor\"\n";
    const OUR_MIME: &str = "# Written by edel settings apply from [default_apps] (M6.10); edit the settings file, not this file.\n[Default Applications]\ntext/plain=org.gnome.TextEditor.desktop;\n";

    #[test]
    fn default_apps_are_written_when_the_file_has_none() {
        let changes = mime_changes(EDITOR_SET, None);
        assert_eq!(changes, [Change::MimeDefaults(OUR_MIME.to_string())]);
        assert_eq!(
            changes[0].to_string(),
            "write /etc/xdg/mimeapps.list (the default apps)"
        );
    }

    #[test]
    fn default_apps_already_written_need_no_change() {
        assert!(mime_changes(EDITOR_SET, Some(OUR_MIME)).is_empty());
    }

    #[test]
    fn our_mime_file_is_removed_when_no_default_app_is_set() {
        let changes = mime_changes("format = 1\n", Some(OUR_MIME));
        assert_eq!(changes, [Change::MimeDefaults(String::new())]);
        assert_eq!(
            changes[0].to_string(),
            "remove /etc/xdg/mimeapps.list (no default apps set)"
        );
        assert!(
            mime_changes("format = 1\n", None).is_empty(),
            "no file and no default apps: nothing to do"
        );
    }

    #[test]
    fn someone_elses_mime_file_is_never_written_or_removed() {
        let theirs = "[Default Applications]\ntext/plain=vim.desktop;\n";
        assert!(mime_changes(EDITOR_SET, Some(theirs)).is_empty());
        assert!(mime_changes("format = 1\n", Some(theirs)).is_empty());
    }

    #[test]
    fn a_machine_that_matches_needs_no_change() {
        let file = settings::read(
            "format = 1\n[network]\nhostname = \"lab-1\"\n[users.ci]\nadmin = true\nssh_keys = [\"ssh-ed25519 AAAA ci@edel\"]\n[users.sshd]\nadmin = true\n",
        )
        .unwrap()
        .file;
        let (changes, notes) = plan(&file, &machine());
        assert_eq!(changes, []);
        assert_eq!(notes, ["left users.sshd alone: it is a system account"]);
    }

    #[test]
    fn plans_each_difference_once() {
        let file = settings::read(
            "format = 1\n[system]\ndeveloper_mode = true\n[users.ali]\nadmin = true\n[users.new]\nlogin_shell = \"/bin/ash\"\n",
        )
        .unwrap()
        .file;
        let (changes, _) = plan(&file, &machine());
        let shown: Vec<String> = changes.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            shown,
            [
                "network.hostname: back to the release's",
                "users.ali.login_shell: set to /bin/sh",
                "users.ali: allow key logins",
                "users.ali.admin: on",
                "users.new: add, with shell /bin/ash",
                "system.developer_mode: on",
            ]
        );
    }

    #[test]
    fn plans_a_zone_the_machine_has_no_link_for() {
        let file = settings::read("format = 1\n[region]\ntimezone = \"Asia/Tashkent\"\n")
            .unwrap()
            .file;
        let (changes, _) = plan(&file, &machine());
        assert!(changes.contains(&Change::Timezone("Asia/Tashkent".into())));
        assert!(
            changes
                .iter()
                .any(|c| { c.to_string() == "region.timezone: set to Asia/Tashkent" })
        );
    }

    #[test]
    fn plans_nothing_for_the_zone_already_linked() {
        let file = settings::read("format = 1\n[region]\ntimezone = \"Asia/Tashkent\"\n")
            .unwrap()
            .file;
        let mut machine = machine();
        machine.timezone = Some("Asia/Tashkent".into());
        let (changes, _) = plan(&file, &machine);
        assert!(!changes.iter().any(|c| matches!(c, Change::Timezone(_))));
    }

    #[test]
    fn plans_the_default_when_the_file_names_no_zone_and_a_link_is_ours() {
        let file = settings::read("format = 1\n").unwrap().file;
        let mut linked = machine();
        linked.timezone = Some("Asia/Tashkent".into());
        let (changes, _) = plan(&file, &linked);
        assert!(changes.contains(&Change::TimezoneDefault));
        let (changes, _) = plan(&file, &machine());
        assert!(!changes.contains(&Change::TimezoneDefault));
    }

    #[test]
    fn reads_the_zone_a_link_names_only_inside_the_zone_files() {
        assert_eq!(
            zone_of_target(Path::new("/usr/share/zoneinfo/Asia/Tashkent")).as_deref(),
            Some("Asia/Tashkent")
        );
        assert_eq!(zone_of_target(Path::new("/usr/share/zoneinfo/")), None);
        assert_eq!(
            zone_of_target(Path::new("/etc/elsewhere/Asia/Tashkent")),
            None
        );
    }

    #[test]
    fn each_admin_gets_the_flatpak_apps_it_lacks_in_one_change() {
        // Only the Flatpak changes are compared: the other users' changes
        // are the other tests' concern.
        fn flatpak(changes: &[Change]) -> Vec<Change> {
            changes
                .iter()
                .filter(|c| matches!(c, Change::Flatpak { .. }))
                .map(|c| match c {
                    Change::Flatpak { name, ids } => Change::Flatpak {
                        name: name.clone(),
                        ids: ids.clone(),
                    },
                    _ => unreachable!(),
                })
                .collect()
        }
        let file = settings::read(
            "format = 1\n[apps]\ninstalled = [\"io.mpv.Mpv\", \"org.gnome.Calculator\"]\n[users.ci]\nadmin = true\n[users.ali]\nadmin = false\n",
        )
        .unwrap()
        .file;
        let mut machine = machine();
        // ci has one of the two; ali is not an admin, so gets nothing.
        machine
            .flatpak
            .insert("ci".into(), BTreeSet::from(["io.mpv.Mpv".to_string()]));
        let (changes, _) = plan(&file, &machine);
        assert_eq!(
            flatpak(&changes),
            [Change::Flatpak {
                name: "ci".into(),
                ids: vec!["org.gnome.Calculator".into()],
            }]
        );
        assert_eq!(
            flatpak(&changes)[0].to_string(),
            "apps.installed for ci: install org.gnome.Calculator"
        );
        // All there: no change.
        machine.flatpak.insert(
            "ci".into(),
            BTreeSet::from(["io.mpv.Mpv".to_string(), "org.gnome.Calculator".to_string()]),
        );
        assert_eq!(flatpak(&plan(&file, &machine).0), []);
        // Nothing installed and a repeated id: one change, the id once.
        let file = settings::read(
            "format = 1\n[apps]\ninstalled = [\"io.mpv.Mpv\", \"io.mpv.Mpv\"]\n[users.ci]\nadmin = true\n",
        )
        .unwrap()
        .file;
        machine.flatpak.clear();
        assert_eq!(
            flatpak(&plan(&file, &machine).0),
            [Change::Flatpak {
                name: "ci".into(),
                ids: vec!["io.mpv.Mpv".into()],
            }]
        );
    }

    #[test]
    fn a_boot_apply_says_the_install_waits_for_the_network() {
        let change = Change::Flatpak {
            name: "ali".into(),
            ids: vec!["io.mpv.Mpv".into()],
        };
        assert_eq!(
            deferred(&change, true).as_deref(),
            Some("flatpak apps for ali wait for the network; edel settings apply installs them")
        );
        assert_eq!(deferred(&change, false), None);
        assert_eq!(deferred(&Change::AdminGroup, true), None);
    }

    #[test]
    fn flathub_is_added_by_the_one_remote_constant_before_each_install() {
        let commands = flatpak_commands(&["io.mpv.Mpv".to_string()]);
        assert_eq!(
            commands[0],
            [
                "--user",
                "remote-add",
                "--if-not-exists",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo",
            ]
        );
        assert_eq!(
            commands[1],
            [
                "--user",
                "install",
                "--noninteractive",
                "flathub",
                "io.mpv.Mpv"
            ]
        );
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn people_and_the_greeter_join_the_seat_where_there_is_one() {
        let file = settings::read("format = 1\n[users.ci]\n[users.new]\n[users.sshd]\n")
            .unwrap()
            .file;
        let mut desktop = machine();
        desktop
            .passwd
            .push_str("greetd:x:101:102:greetd:/var/lib/greetd:/sbin/nologin\n");
        desktop.group.push_str("seat:x:103:ci\n");
        let (changes, _) = plan(&file, &desktop);
        let seat: Vec<String> = changes
            .iter()
            .filter(|c| matches!(c, Change::Seat(_)))
            .map(|c| c.to_string())
            .collect();
        assert_eq!(seat, ["group seat: add new", "group seat: add greetd"]);
        // A server has no seat group, so nobody joins one.
        let (changes, _) = plan(&file, &machine());
        assert!(!changes.iter().any(|c| matches!(c, Change::Seat(_))));
    }

    /// A home in a temporary directory, owned by whoever runs the tests.
    fn home(test: &str) -> (PathBuf, Account) {
        let dir = std::env::temp_dir().join(format!("edel-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".ssh")).unwrap();
        let meta = fs::metadata(&dir).unwrap();
        let account = Account {
            name: "ci".into(),
            uid: meta.uid(),
            gid: meta.gid(),
            home: dir.display().to_string(),
            shell: DEFAULT_SHELL.into(),
        };
        (dir, account)
    }

    #[test]
    fn reads_keys_only_from_a_regular_file_the_user_owns() {
        let (dir, account) = home("read-keys");
        let keys = dir.join(".ssh/authorized_keys");
        fs::write(&keys, "ssh-ed25519 AAAA ci@edel\n").unwrap();
        assert_eq!(
            read_keys(&account).as_deref(),
            Some("ssh-ed25519 AAAA ci@edel\n")
        );
        let stranger = Account {
            uid: account.uid + 1,
            ..account
        };
        assert_eq!(read_keys(&stranger), None);
        let account = Account {
            uid: stranger.uid - 1,
            ..stranger
        };
        // A link to a file elsewhere, and a FIFO that never ends, both
        // count as no file: root must not read through them or hang.
        fs::remove_file(&keys).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", &keys).unwrap();
        assert_eq!(read_keys(&account), None);
        fs::remove_file(&keys).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(&keys)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(read_keys(&account), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_keys_as_the_user_with_private_modes() {
        let (dir, account) = home("write-keys");
        fs::remove_dir(dir.join(".ssh")).unwrap();
        write_keys(&account, "ssh-ed25519 AAAA ci@edel\n").unwrap();
        let keys = dir.join(".ssh/authorized_keys");
        assert_eq!(
            fs::read_to_string(&keys).unwrap(),
            "ssh-ed25519 AAAA ci@edel\n"
        );
        assert_eq!(
            fs::metadata(dir.join(".ssh")).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&keys).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!dir.join(".ssh/authorized_keys.edel-new").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_desktop_follows_shell_keys_and_apply_the_rest() {
        assert!(desktop_follows("layout.tiling"));
        assert!(desktop_follows("displays.eDP-1.scale"));
        assert!(!desktop_follows("network.hostname"));
        // The pins are followed at once, the rest of [apps] is not.
        assert!(desktop_follows("panels.pinned"));
        assert!(!desktop_follows("apps.installed"));
        // What the Settings app writes to the person's file, the desktop
        // reads from it, so a person's set and reset write it there too.
        for key in [
            "notifications.do_not_disturb",
            "default_apps.browser",
            "startup.apps",
            "region.language",
            "region.keyboard",
            "region.input_method",
        ] {
            assert!(desktop_follows(key), "{key}");
        }
        // The clock's zone and servers are the machine's, which apply sets.
        assert!(!desktop_follows("region.timezone"));
        assert!(!desktop_follows("region.time_servers"));
    }

    #[test]
    fn apply_does_nothing_for_the_pinned_apps() {
        // The pins need nothing applied, as panels.tray needs
        // nothing (M5.31d): the desktop reads them from the file itself.
        let file = settings::read("format = 1\n[panels]\npinned = [\"settings\", \"terminal\"]\n")
            .unwrap();
        let (changes, notes) = plan(&file.file, &Machine::default());
        assert!(changes.is_empty(), "{changes:?}");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn keeps_the_shell_when_the_file_names_one_the_machine_lacks() {
        let file = settings::read(
            "format = 1\n[network]\nhostname = \"lab-1\"\n[users.ci]\nlogin_shell = \"/bin/zsh\"\n[users.new]\nlogin_shell = \"/bin/zsh\"\n",
        )
        .unwrap()
        .file;
        let (changes, notes) = plan(&file, &machine());
        let shown: Vec<String> = changes.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            shown,
            ["users.ci.admin: off", "users.new: add, with shell /bin/sh"]
        );
        assert_eq!(
            notes,
            [
                "kept users.ci.login_shell as it is: /bin/zsh is not a program on this machine",
                "kept users.new.login_shell as it is: /bin/zsh is not a program on this machine",
            ]
        );
    }

    #[test]
    fn unset_admin_takes_admin_away() {
        let text = settings::unset(
            "format = 1\n[network]\nhostname = \"lab-1\"\n[users.ci]\nadmin = true\n",
            "users.ci.admin",
        )
        .unwrap();
        let (changes, _) = plan(&settings::read(&text).unwrap().file, &machine());
        assert_eq!(
            changes,
            [Change::Admin {
                name: "ci".into(),
                on: false
            }]
        );
    }

    #[test]
    fn keeps_an_applied_file_with_the_older_format_it_was_read_as() {
        let dir = std::env::temp_dir().join(format!("edel-keep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let from = dir.join("new.toml");
        fs::write(&from, "format = 9\n").unwrap();
        fs::write(dir.join("new.toml.v1"), "format = 1\n").unwrap();
        let to = places::settings_in(&dir.join("data"));
        keep_as_machine_file(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "format = 9\n");
        assert_eq!(
            fs::read_to_string(settings::versioned(
                &places::settings_in(&dir.join("data")),
                1
            ))
            .unwrap(),
            "format = 1\n"
        );
        // The same file by another name is left alone, never emptied.
        assert!(same_file(
            &to,
            &places::settings_in(&dir.join("data/../data"))
        ));
        assert!(!same_file(&to, &from));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lists_what_the_machine_changed_in_etc() {
        let dir = std::env::temp_dir().join(format!("edel-changed-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("ssh")).unwrap();
        fs::write(dir.join("hostname"), "x\n").unwrap();
        fs::write(dir.join("ssh/ssh_host_ed25519_key"), "").unwrap();
        assert_eq!(
            changed_files(&dir, Path::new("/etc")),
            ["/etc/hostname", "/etc/ssh/ssh_host_ed25519_key"]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn describes_the_machine_without_defaults() {
        let mut file = settings::read(
            "format = 1\n[appearance]\nmode = \"dark\"\n[users.gone]\nadmin = true\n",
        )
        .unwrap()
        .file;
        describe(
            &mut file,
            PASSWD,
            "admin:x:101:ci\n",
            Some("lab-1\n"),
            |a: &Account| {
                (a.home == "/home/ci").then(|| "# mine\nssh-ed25519 AAAA ci@edel\n".into())
            },
            false,
            DEFAULT_SHELL,
        );
        assert_eq!(file.network.hostname.as_deref(), Some("lab-1"));
        assert_eq!(file.appearance.mode.as_deref(), Some("dark"));
        assert_eq!(file.system.developer_mode, None);
        assert_eq!(file.users.keys().collect::<Vec<_>>(), ["ali", "ci"]);
        assert_eq!(file.users["ci"].admin, Some(true));
        assert_eq!(
            file.users["ci"].ssh_keys,
            Some(vec!["ssh-ed25519 AAAA ci@edel".into()])
        );
        assert_eq!(file.users["ci"].login_shell, None);
        assert_eq!(file.users["ali"].admin, None);
        assert_eq!(file.users["ali"].login_shell.as_deref(), Some("/bin/ash"));
        let text = toml::to_string(&file).unwrap();
        assert!(settings::read(&text).unwrap().problems.is_empty(), "{text}");
    }

    #[test]
    fn gives_the_install_passwords_to_the_people_in_the_file_only() {
        let text = "ali:$6$salt$hash\nmallory:$6$x$y\nbad:plain\n";
        let users = ["ali".to_string(), "bad".to_string()];
        let names = first_password_names(text, users.iter());
        assert_eq!(names, ["ali", "bad"]);
        assert_eq!(first_password_lines(text, &names), "ali:$6$salt$hash\n");
        assert!(first_password_names(text, [].iter()).is_empty());
    }
}
