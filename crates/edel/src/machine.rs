//! `edel settings` (roadmap M2.2, M2.3, M5.25a): makes this machine match
//! its settings file, describes the machine as one, and reads and changes
//! the file. Apply runs at every boot from the `edel-settings` service,
//! seeding the file on the first, and on demand. It applies the sections that need no network:
//! the hostname, users, their ssh keys and developer mode. It adds and
//! changes, and never deletes a user (ADR-006).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt, chown};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::places;
use edel::system::{self, SystemFile, User};

use crate::boot::GRUB_PREFIX;
use crate::release::os_release_value;
use crate::update::{Disk, Lock, run};

/// The machine's settings file as it is found now: by its name, or by a
/// name it had before (`edel::places`), until apply renames it.
fn machine_file() -> PathBuf {
    places::found(&places::machine_settings())
}
/// Developer mode is on while this file exists (ADR-007; M7.1 acts on it).
const DEVELOPER_FLAG: &str = "/data/edel/developer";
/// What this machine changed in `/etc`: a file here differs from the slot's.
const ETC_UPPER: &str = "/data/etc/upper";
/// A volume with this label holding the settings file seeds a first boot.
const SEED_LABEL: &str = "EDEL-SEED";
/// The EFI system partition's number on the running disk.
const ESP_PARTITION: u32 = 1;
/// The login shell of a user whose entry names none.
pub const DEFAULT_SHELL: &str = "/bin/sh";
/// Members of this group are admins.
const ADMIN_GROUP: &str = "admin";
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
struct Account {
    name: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

fn accounts(passwd: &str) -> Vec<Account> {
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
fn is_person(account: &Account) -> bool {
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
/// the session through, no password (`*`) and the seat.
pub fn add_live_account(name: &str) -> Result<()> {
    if !accounts(&fs::read_to_string("/etc/passwd")?)
        .iter()
        .any(|a| a.name == name)
    {
        let home = format!("/home/{name}");
        run(Command::new("adduser").args(["-S", "-D", "-s", DEFAULT_SHELL, "-h", &home, name]))?;
        unlock(name)?;
    }
    let group = fs::read_to_string("/etc/group")?;
    if members(&group, SEAT_GROUP).is_some_and(|m| !m.iter().any(|m| m == name)) {
        run(Command::new("addgroup").args([name, SEAT_GROUP]))?;
    }
    Ok(())
}

/// The members of `group` in `/etc/group`, or `None` without the group.
fn members(group_file: &str, group: &str) -> Option<Vec<String>> {
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
    let meta = fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, text)?;
    fs::set_permissions(&new, meta.permissions())?;
    chown(&new, Some(meta.uid()), Some(meta.gid()))?;
    fs::File::open(&new)?.sync_all()?;
    fs::rename(&new, path).with_context(|| format!("replacing {}", path.display()))
}

/// One change apply makes; `edel settings diff` lists them without making them.
#[derive(Debug, PartialEq)]
enum Change {
    /// Write `/etc/hostname`
    Hostname(String),
    /// The file names no hostname: drop this machine's copy of
    /// `/etc/hostname`, so the slot's shows again (ADR-008, defaults)
    HostnameDefault,
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
    /// May use the screen and input: a person in the file, or the greeter
    Seat(String),
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Hostname(h) => write!(f, "network.hostname: set to {h}"),
            Change::HostnameDefault => write!(f, "network.hostname: back to the release's"),
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
            Change::Developer(on) => {
                write!(
                    f,
                    "system.developer_mode: {}",
                    if *on { "on" } else { "off" }
                )
            }
            Change::Seat(name) => write!(f, "group {SEAT_GROUP}: add {name}"),
        }
    }
}

/// What apply compares the file with, read from the machine.
#[derive(Default)]
struct Machine {
    hostname: String,
    /// Whether this machine has its own `/etc/hostname`
    hostname_set: bool,
    passwd: String,
    group: String,
    /// `None` when this user may not read it
    shadow: Option<String>,
    /// `~/.ssh/authorized_keys` of each named user that has one
    keys: BTreeMap<String, String>,
    /// The shells the file names that are programs on this machine
    shells: BTreeSet<String>,
    developer: bool,
}

impl Machine {
    fn read(file: &SystemFile) -> Result<Machine> {
        let passwd = fs::read_to_string("/etc/passwd")?;
        let mut keys = BTreeMap::new();
        for account in accounts(&passwd) {
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
            passwd,
            group: fs::read_to_string("/etc/group")?,
            shadow: fs::read_to_string("/etc/shadow").ok(),
            keys,
            shells,
            developer: Path::new(DEVELOPER_FLAG).exists(),
        })
    }
}

/// The changes that make `machine` match `file`, and notes on what is left
/// alone. Pure, so diff and apply agree and tests need no machine.
fn plan(file: &SystemFile, machine: &Machine) -> (Vec<Change>, Vec<String>) {
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    match &file.network.hostname {
        Some(h) if machine.hostname.trim() != h => changes.push(Change::Hostname(h.clone())),
        None if machine.hostname_set => changes.push(Change::HostnameDefault),
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
                account.map_or(DEFAULT_SHELL, |a| a.shell.as_str())
            }
            Some(s) => s,
            None => DEFAULT_SHELL,
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
    let developer = file.system.developer_mode == Some(true);
    if developer != machine.developer {
        changes.push(Change::Developer(developer));
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
        .with_context(|| format!("{name} is not in /etc/passwd"))
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
            "edel settings: ignored {}: not a regular file of {} up to 1 MiB",
            path.display(),
            account.name
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
        .with_context(|| format!("starting a shell as {}", account.name))?;
    let written = child
        .stdin
        .take()
        .context("no input to the shell")?
        .write_all(text.as_bytes());
    let status = child.wait()?;
    if !status.success() || written.is_err() {
        bail!(
            "could not write {}/.ssh/authorized_keys as {}",
            account.home,
            account.name
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
        Change::Seat(name) => run(Command::new("addgroup").args([name, SEAT_GROUP]))?,
    }
    Ok(())
}

/// The file to apply or diff, read leniently, with its problems and the
/// keys this release skips printed first; `None` when there is none.
fn load(file: Option<&Path>, seed_if_missing: bool) -> Result<Option<system::Read>> {
    let machine = machine_file();
    let path = file.unwrap_or(&machine);
    if file.is_none() && !path.exists() {
        match seed_if_missing.then(|| seed(path)).transpose()?.flatten() {
            Some(from) => println!("edel settings: seeded {} from {from}", path.display()),
            None => {
                println!("edel settings: no settings file, so nothing to apply");
                return Ok(None);
            }
        }
    }
    let read = system::read_on_machine(path)?;
    for problem in &read.problems {
        println!("edel settings: left out {problem}");
    }
    for key in &read.later {
        println!("edel settings: skipped {key}: not supported yet");
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
        for format in 1..=system::FORMAT {
            let old = system::versioned(&found, format);
            if old.exists() {
                fs::rename(&old, system::versioned(&machine, format))?;
            }
        }
        fs::rename(&found, &machine)
            .with_context(|| format!("renaming {} to {}", found.display(), machine.display()))?;
        println!(
            "edel settings: renamed {} to {}",
            found.display(),
            machine.display()
        );
    }
    let link = places::etc_settings();
    if fs::read_link(&link).ok().as_deref() != Some(machine.as_path()) {
        fs::create_dir_all(places::ETC_DIR)?;
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&machine, &link)
            .with_context(|| format!("linking {} to {}", link.display(), machine.display()))?;
        println!(
            "edel settings: linked {} to {}",
            link.display(),
            machine.display()
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
    // One change that fails never stops the others (Reliable): each is
    // reported, and apply fails at the end.
    let mut failed = 0;
    for change in &changes {
        match execute(change, boot) {
            Ok(()) => println!("edel settings: {change}"),
            Err(err) => {
                failed += 1;
                eprintln!("edel settings: could not apply {change}: {err:#}");
            }
        }
    }
    let machine = places::machine_settings();
    if let Some(path) = file.filter(|f| !same_file(f, &machine)) {
        keep_as_machine_file(path, &machine)?;
        println!(
            "edel settings: {} is now this machine's settings file",
            path.display()
        );
    } else if changes.is_empty() {
        println!("edel settings: nothing to change");
    }
    if failed > 0 {
        bail!("{failed} of {} changes could not be applied", changes.len());
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
    let mut out = fs::File::create(&new).with_context(|| format!("writing {}", new.display()))?;
    out.write_all(text)?;
    out.sync_all()?;
    fs::rename(&new, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Makes the applied `from` this machine's file `to`. A file in a newer
/// format brings the `.v<N>` this release read, so the next boot reads it
/// too (ADR-008).
fn keep_as_machine_file(from: &Path, to: &Path) -> Result<()> {
    let text = fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    if system::format(&String::from_utf8_lossy(&text)).is_ok_and(|f| f > system::FORMAT) {
        let older = system::versioned(from, system::FORMAT);
        let older_text =
            fs::read(&older).with_context(|| format!("reading {}", older.display()))?;
        write_whole(&system::versioned(to, system::FORMAT), &older_text)?;
    }
    write_whole(to, &text).with_context(|| format!("saving {}", to.display()))
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
    let _lock = Lock::take("settings", "edel settings")?;
    let machine = machine_file();
    let shown = machine.display().to_string();
    let mut path = machine.clone();
    let mut newer = None;
    if let Ok(text) = fs::read_to_string(&machine) {
        let format = system::format(&text).with_context(|| format!("reading {shown}"))?;
        if format > system::FORMAT {
            path = system::versioned(&machine, system::FORMAT);
            newer = Some(format);
        }
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) if newer.is_none() => format!("format = {}\n", system::FORMAT),
        Err(_) => bail!(
            "{shown} is format {}, newer than this release, and there is no {} beside it to change",
            newer.unwrap_or_default(),
            path.display()
        ),
    };
    let edited = change(&text)?;
    for problem in system::read(&edited)?.problems {
        println!("edel settings: kept, not used by this release: {problem}");
    }
    fs::create_dir_all(machine.parent().unwrap_or(Path::new("/")))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, &edited)?;
    fs::File::open(&new)?.sync_all()?;
    fs::rename(&new, &path).with_context(|| format!("replacing {}", path.display()))?;
    let desktop = keys.iter().all(|k| desktop_follows(k));
    println!(
        "edel settings: {what} in {}; {}",
        path.display(),
        if desktop {
            "the desktop follows it at once"
        } else {
            "edel settings apply applies it"
        }
    );
    if let Some(format) = newer {
        println!(
            "edel settings: {shown} is format {format}, so the change applies to this release only"
        );
    }
    Ok(())
}

/// `edel settings set KEY=VALUE...`: every assignment is checked before
/// any is written, and all are written at once, or none.
pub fn set(assignments: &[String]) -> Result<()> {
    let mut pairs = Vec::new();
    for assignment in assignments {
        let (key, value) = assignment.split_once('=').with_context(|| {
            format!("{assignment:?}: write KEY=VALUE, such as network.hostname=lab-1")
        })?;
        pairs.push((key.trim(), value.trim()));
    }
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
    edit(&format!("set {}", keys.join(", ")), &keys, |text| {
        let mut text = text.to_string();
        for (key, value) in &pairs {
            text = system::set(&text, key, value)?;
        }
        Ok(text)
    })
}

/// Whether the desktop follows `key` at once, as it does the layout,
/// screen, look and shortcut keys (M4.5, M4.6, M5.5c, M5.13a); `edel
/// settings apply` applies the rest.
fn desktop_follows(key: &str) -> bool {
    ["layout.", "displays.", "appearance.", "shortcuts."]
        .iter()
        .any(|section| key.starts_with(section))
}

/// `edel settings reset KEY...`: removes each key, so the release decides
/// it again.
pub fn reset(keys: &[String]) -> Result<()> {
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    edit(&format!("reset {}", keys.join(", ")), &keys, |text| {
        let mut text = text.to_string();
        for key in &keys {
            text = system::unset(&text, key)?;
        }
        Ok(text)
    })
}

/// A page's name as people type it: its title in lowercase, `_` for a
/// space, such as `default_apps` (ADR-008's same names decision).
fn page_word(page: &system::Page) -> String {
    page.title.to_lowercase().replace(' ', "_").replace('-', "")
}

/// `edel settings` alone: the pages, in the Settings app's order.
pub fn pages() {
    println!("Settings, page by page, as the Settings app shows them:\n");
    let width = system::PAGES
        .iter()
        .map(|p| page_word(p).len())
        .max()
        .unwrap_or(0);
    for page in system::PAGES {
        println!(
            "  {:width$}  {}: {}",
            page_word(page),
            page.title,
            page.about
        );
    }
    println!(
        "\nedel settings get PAGE shows a page's settings and where each comes from;\n\
         edel settings set KEY=VALUE changes one, and reset KEY gives it back to the release."
    );
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
        .and_then(|p| system::read_on_machine(&p).ok());
    if let Some(toml::Value::Table(table)) = file.and_then(|r| toml::Value::try_from(r.file).ok()) {
        flatten(&table, "", &mut out);
    }
    out
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
        Some(w) => match system::PAGES
            .iter()
            .find(|p| page_word(p) == w || p.section == w)
        {
            Some(page) => (Some(page.section), None),
            None if system::KEYS.iter().any(|k| key_fits(k.path, w)) => (None, Some(w)),
            None if machine
                .iter()
                .chain(&person)
                .any(|(k, _)| k.starts_with(&format!("{w}."))) =>
            {
                (None, Some(w))
            }
            None => match system::nearest_key(w) {
                Some(near) => bail!("{w}: no such page or key; did you mean {near}?"),
                None => bail!("{w}: no such page or key; edel settings lists the pages"),
            },
        },
    };
    let wanted = |k: &str| match (section, key) {
        (Some(s), _) => k.starts_with(&format!("{s}.")),
        (_, Some(key)) => k == key || k.starts_with(&format!("{key}.")),
        _ => true,
    };
    // The person's value wins over the machine's for the same key.
    let mut rows: Vec<(String, toml::Value, &str)> = Vec::new();
    for (k, v) in &person {
        if wanted(k) {
            rows.push((k.clone(), v.clone(), "your own file"));
        }
    }
    for (k, v) in &machine {
        if wanted(k) && !rows.iter().any(|(r, _, _)| r == k) {
            rows.push((k.clone(), v.clone(), "this machine"));
        }
    }
    if as_toml {
        let mut table = toml::Table::new();
        for (k, v, _) in &rows {
            insert_path(&mut table, k, v.clone());
        }
        print!("{}", toml::to_string(&table)?);
        return Ok(());
    }
    // The keys a page has that nobody set, so the release decides them.
    for entry in system::KEYS.iter().filter(|k| !k.path.contains('*')) {
        if wanted(entry.path) && !rows.iter().any(|(r, _, _)| r == entry.path) {
            rows.push((
                entry.path.to_string(),
                toml::Value::String(String::new()),
                "",
            ));
        }
    }
    let width = rows.iter().map(|(k, _, _)| k.len()).max().unwrap_or(0);
    for page in system::PAGES {
        let mut mine: Vec<&(String, toml::Value, &str)> = rows
            .iter()
            .filter(|(k, _, _)| k.split('.').next() == Some(page.section))
            .collect();
        if mine.is_empty() {
            continue;
        }
        mine.sort_by(|a, b| (a.2.is_empty(), &a.0).cmp(&(b.2.is_empty(), &b.0)));
        println!("{} ({}): {}", page.title, page_word(page), page.about);
        for (k, v, from) in mine {
            let later = system::KEYS
                .iter()
                .any(|e| !e.supported && key_fits(e.path, k));
            let shown = if from.is_empty() {
                "not set: the release decides".to_string()
            } else {
                format!("{v}  ({from})")
            };
            let later = if later { "  (not supported yet)" } else { "" };
            println!("  {k:width$}  {shown}{later}");
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
        if let Err(err) = system::read(&text) {
            println!("edel settings: passed over the settings file on {from}: {err:#}");
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
/// a former one, mounted read-only for as long as it takes to read it. `fs` names the file system when it is known: early
/// in boot the FAT driver is not loaded yet, so mount cannot guess it.
fn read_from(device: &Path, inner: &Path, fs: Option<&str>) -> Option<String> {
    let dir = Path::new("/run/edel/seed");
    fs::create_dir_all(dir).ok()?;
    // FAT needs a charset the virt kernel has; other file systems refuse
    // the option, so they get a second try without it.
    let mount = |kind: &str, options: &str| {
        Command::new("mount")
            .args(["-t", kind, "-o", options])
            .arg(device)
            .arg(dir)
            .status()
            .is_ok_and(|s| s.success())
    };
    let mounted = match fs {
        Some(kind) => mount(kind, "ro,iocharset=iso8859-1"),
        None => mount("vfat", "ro,iocharset=iso8859-1") || mount("auto", "ro"),
    };
    if !mounted {
        eprintln!(
            "warning: cannot mount {} to look for a settings file",
            device.display()
        );
        return None;
    }
    let text = fs::read_to_string(places::found(&places::settings_in(&dir.join(inner)))).ok();
    let _ = Command::new("umount").arg(dir).status();
    text
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
    let mut file = match system::read_on_machine(&machine) {
        Ok(read) => read.file,
        Err(_) if !machine.exists() => SystemFile::default(),
        Err(err) => {
            eprintln!("warning: {err:#}; exporting only what the machine has");
            SystemFile::default()
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
    );
    print!("{EXPORT_HEADER}\n{}", toml::to_string(&file)?);
    let changed = changed_files(Path::new(ETC_UPPER), Path::new("/etc"));
    if !changed.is_empty() {
        println!("\n# Files this machine changed in /etc, kept on /data and not described above:");
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
    file: &mut SystemFile,
    passwd: &str,
    group: &str,
    hostname: Option<&str>,
    authorized_keys: impl Fn(&Account) -> Option<String>,
    developer: bool,
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
            login_shell: (account.shell != DEFAULT_SHELL).then(|| account.shell.clone()),
        };
        file.users.insert(account.name.clone(), user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            passwd: PASSWD.into(),
            group: "admin:x:101:ci\n".into(),
            shadow: Some("ci:*:1::::::\nali:!:1::::::\n".into()),
            keys: BTreeMap::from([("ci".into(), "ssh-ed25519 AAAA ci@edel\n".into())]),
            shells: BTreeSet::from(["/bin/ash".into()]),
            developer: false,
        }
    }

    #[test]
    fn a_machine_that_matches_needs_no_change() {
        let file = system::read(
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
        let file = system::read(
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
    fn people_and_the_greeter_join_the_seat_where_there_is_one() {
        let file = system::read("format = 1\n[users.ci]\n[users.new]\n[users.sshd]\n")
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
    }

    #[test]
    fn keeps_the_shell_when_the_file_names_one_the_machine_lacks() {
        let file = system::read(
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
        let text = system::unset(
            "format = 1\n[network]\nhostname = \"lab-1\"\n[users.ci]\nadmin = true\n",
            "users.ci.admin",
        )
        .unwrap();
        let (changes, _) = plan(&system::read(&text).unwrap().file, &machine());
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
            fs::read_to_string(system::versioned(
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
        let mut file =
            system::read("format = 1\n[appearance]\nmode = \"dark\"\n[users.gone]\nadmin = true\n")
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
        assert!(system::read(&text).unwrap().problems.is_empty(), "{text}");
    }
}
