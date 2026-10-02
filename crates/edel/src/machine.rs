//! `edel system apply` and `edel system export` (roadmap M2.2): makes this
//! machine match its system file, and describes the machine as one. Apply
//! runs at every boot from the `edel-system` service, seeding the file on
//! the first, and on demand. It applies the sections that need no network:
//! the hostname, users, their ssh keys and developer mode. It adds and
//! changes, and never deletes a user (ADR-006).

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, Permissions};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, chown};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use edel::system::{self, SystemFile, User};

use crate::boot::GRUB_PREFIX;
use crate::release::os_release_value;
use crate::update::{Disk, Lock, run};

/// The machine's system file, on the data partition.
pub const SYSTEM_FILE: &str = "/data/edel/system.toml";
/// Developer mode is on while this file exists (ADR-007; M7.1 acts on it).
const DEVELOPER_FLAG: &str = "/data/edel/developer";
/// What this machine changed in `/etc`: a file here differs from the slot's.
const ETC_UPPER: &str = "/data/etc/upper";
/// A volume with this label holding `system.toml` seeds a first boot.
const SEED_LABEL: &str = "EDEL-SEED";
/// The EFI system partition's number on the running disk.
const ESP_PARTITION: u32 = 1;
/// The slot's own seed, used when no volume holds one.
const SLOT_SEED: &str = "/usr/share/edel/system.toml";
/// The login shell of a user whose entry names none.
pub const DEFAULT_SHELL: &str = "/bin/sh";
/// Members of this group are admins.
const ADMIN_GROUP: &str = "admin";

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

/// One change apply makes; `edel system diff` lists them without making them.
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
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Hostname(h) => write!(f, "network.hostname: set to {h}"),
            Change::HostnameDefault => write!(f, "network.hostname: back to the release's"),
            Change::AddUser { name, shell } => write!(f, "users.{name}: add, with shell {shell}"),
            Change::Shell { name, shell } => write!(f, "users.{name}.shell: set to {shell}"),
            Change::Unlock(name) => write!(f, "users.{name}: allow key logins"),
            Change::AdminGroup => write!(f, "group {ADMIN_GROUP}: add"),
            Change::Admin { name, on } => {
                write!(f, "users.{name}.admin: {}", if *on { "on" } else { "off" })
            }
            Change::Keys { name, keys } => {
                write!(f, "users.{name}.ssh_keys: write {} keys", keys.len())
            }
            Change::Developer(on) => {
                write!(f, "system.developer: {}", if *on { "on" } else { "off" })
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
    passwd: String,
    group: String,
    /// `None` when this user may not read it
    shadow: Option<String>,
    /// `~/.ssh/authorized_keys` of each named user that has one
    keys: BTreeMap<String, String>,
    developer: bool,
}

impl Machine {
    fn read(file: &SystemFile) -> Result<Machine> {
        let passwd = fs::read_to_string("/etc/passwd")?;
        let mut keys = BTreeMap::new();
        for account in accounts(&passwd) {
            if file.users.contains_key(&account.name) {
                let path = Path::new(&account.home).join(".ssh/authorized_keys");
                if let Ok(text) = fs::read_to_string(path) {
                    keys.insert(account.name, text);
                }
            }
        }
        Ok(Machine {
            hostname: fs::read_to_string("/etc/hostname").unwrap_or_default(),
            hostname_set: Path::new(ETC_UPPER).join("hostname").exists(),
            passwd,
            group: fs::read_to_string("/etc/group")?,
            shadow: fs::read_to_string("/etc/shadow").ok(),
            keys,
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
    for (name, user) in &file.users {
        let shell = user.shell.as_deref().unwrap_or(DEFAULT_SHELL).to_string();
        let account = all.iter().find(|a| &a.name == name);
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
    let developer = file.system.developer == Some(true);
    if developer != machine.developer {
        changes.push(Change::Developer(developer));
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
        Change::Keys { name, keys } => {
            let account = find_account(name)?;
            let dir = Path::new(&account.home).join(".ssh");
            let path = dir.join("authorized_keys");
            fs::create_dir_all(&dir)?;
            fs::write(&path, keys_text(keys))?;
            for (p, mode) in [(&dir, 0o700), (&path, 0o600)] {
                fs::set_permissions(p, Permissions::from_mode(mode))?;
                chown(p, Some(account.uid), Some(account.gid))?;
            }
        }
        Change::Developer(true) => {
            let flag = Path::new(DEVELOPER_FLAG);
            fs::create_dir_all(flag.parent().unwrap_or(Path::new("/")))?;
            fs::write(flag, "")?;
        }
        Change::Developer(false) => fs::remove_file(DEVELOPER_FLAG)?,
    }
    Ok(())
}

/// The file to apply or diff, read leniently, with its problems and the
/// keys this release skips printed first; `None` when there is none.
fn load(file: Option<&Path>, seed_if_missing: bool) -> Result<Option<system::Read>> {
    let path = file.unwrap_or(Path::new(SYSTEM_FILE));
    if file.is_none() && !path.exists() {
        match seed_if_missing.then(|| seed(path)).transpose()?.flatten() {
            Some(from) => println!("edel system: seeded {SYSTEM_FILE} from {from}"),
            None => {
                println!("edel system: no system file, so nothing to apply");
                return Ok(None);
            }
        }
    }
    let read = system::read_on_machine(path)?;
    for problem in &read.problems {
        println!("edel system: left out {problem}");
    }
    for key in &read.later {
        println!("edel system: skipped {key}: not supported yet");
    }
    Ok(Some(read))
}

/// `edel system apply [FILE]`: applies the system file, by default the
/// machine's own, which is seeded first when it is missing. A given FILE
/// becomes the machine's file once applied, so the next boot keeps it.
pub fn apply(file: Option<&Path>, boot: bool) -> Result<()> {
    let _lock = Lock::take("system", "edel system")?;
    let Some(read) = load(file, true)? else {
        return Ok(());
    };
    let (changes, notes) = plan(&read.file, &Machine::read(&read.file)?);
    for note in &notes {
        println!("edel system: {note}");
    }
    for change in &changes {
        execute(change, boot).with_context(|| format!("applying {change}"))?;
        println!("edel system: {change}");
    }
    let machine = Path::new(SYSTEM_FILE);
    if let Some(path) = file.filter(|f| *f != machine) {
        fs::create_dir_all(machine.parent().unwrap_or(Path::new("/")))?;
        fs::copy(path, machine).with_context(|| format!("saving {SYSTEM_FILE}"))?;
        println!(
            "edel system: {} is now this machine's system file",
            path.display()
        );
    } else if changes.is_empty() {
        println!("edel system: nothing to change");
    }
    Ok(())
}

/// `edel system diff [FILE]`: what apply would change, one `change:` line
/// each, changing nothing. Returns whether there is anything to change.
pub fn diff(file: Option<&Path>) -> Result<bool> {
    let Some(read) = load(file, false)? else {
        return Ok(false);
    };
    let (changes, notes) = plan(&read.file, &Machine::read(&read.file)?);
    for note in &notes {
        println!("edel system: {note}");
    }
    for change in &changes {
        println!("change: {change}");
    }
    Ok(!changes.is_empty())
}

/// Edits the machine's system file through `change`, or for a file in a
/// newer format the `system.toml.v<N>` this release reads (ADR-008,
/// writers). Never applies it.
fn edit(what: &str, change: impl Fn(&str) -> Result<String>) -> Result<()> {
    let _lock = Lock::take("system", "edel system")?;
    let machine = Path::new(SYSTEM_FILE);
    let mut path = machine.to_path_buf();
    let mut newer = None;
    if let Ok(text) = fs::read_to_string(machine) {
        let format = system::format(&text).with_context(|| format!("reading {SYSTEM_FILE}"))?;
        if format > system::FORMAT {
            path = system::versioned(machine, system::FORMAT);
            newer = Some(format);
        }
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) if newer.is_none() => format!("format = {}\n", system::FORMAT),
        Err(_) => bail!(
            "{SYSTEM_FILE} is format {}, newer than this release, and there is no {} beside it to change",
            newer.unwrap_or_default(),
            path.display()
        ),
    };
    let edited = change(&text)?;
    for problem in system::read(&edited)?.problems {
        println!("edel system: kept, not used by this release: {problem}");
    }
    fs::create_dir_all(machine.parent().unwrap_or(Path::new("/")))?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, &edited)?;
    fs::File::open(&new)?.sync_all()?;
    fs::rename(&new, &path).with_context(|| format!("replacing {}", path.display()))?;
    println!(
        "edel system: {what} in {}; edel system apply applies it",
        path.display()
    );
    if let Some(format) = newer {
        println!(
            "edel system: {SYSTEM_FILE} is format {format}, so the change applies to this release only"
        );
    }
    Ok(())
}

/// `edel system set KEY=VALUE`
pub fn set(assignment: &str) -> Result<()> {
    let (key, value) = assignment
        .split_once('=')
        .context("write KEY=VALUE, such as network.hostname=lab-1")?;
    let (key, value) = (key.trim(), value.trim());
    edit(&format!("set {key}"), |text| system::set(text, key, value))
}

/// `edel system unset KEY`
pub fn unset(key: &str) -> Result<()> {
    edit(&format!("removed {key}"), |text| system::unset(text, key))
}

/// Looks for a first system file: on a volume labelled EDEL-SEED, then on
/// this disk's EFI system partition, then in the slot. Copies the first one
/// found to `target` and says where it came from. The partition is found
/// from the running disk, not by label: an installer stick and the disk it
/// installed both have an EDEL-ESP.
fn seed(target: &Path) -> Result<Option<String>> {
    let esp_file = format!("{}/system.toml", GRUB_PREFIX.trim_start_matches('/'));
    let mut found = find_by_label(SEED_LABEL)
        .and_then(|device| read_from(&device, "system.toml"))
        .map(|text| (text, format!("the {SEED_LABEL} volume")));
    if found.is_none() {
        found = Disk::find()
            .and_then(|disk| disk.device(ESP_PARTITION))
            .ok()
            .and_then(|device| read_from(&device, &esp_file))
            .map(|text| (text, "the EFI system partition".to_string()));
    }
    if found.is_none() {
        found = fs::read_to_string(SLOT_SEED)
            .ok()
            .map(|text| (text, SLOT_SEED.to_string()));
    }
    let Some((text, from)) = found else {
        return Ok(None);
    };
    fs::create_dir_all(target.parent().unwrap_or(Path::new("/")))?;
    fs::write(target, text).with_context(|| format!("writing {}", target.display()))?;
    Ok(Some(from))
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

/// The text of `inner` on `device`, mounted read-only for as long as it
/// takes to read it.
fn read_from(device: &Path, inner: &str) -> Option<String> {
    let dir = Path::new("/run/edel/seed");
    fs::create_dir_all(dir).ok()?;
    // FAT needs a charset the virt kernel has; other file systems refuse
    // the option, so they get a second try without it.
    let mount = |options: &str| {
        Command::new("mount")
            .args(["-o", options])
            .arg(device)
            .arg(dir)
            .status()
            .is_ok_and(|s| s.success())
    };
    if !mount("ro,iocharset=iso8859-1") && !mount("ro") {
        eprintln!(
            "warning: cannot mount {} to look for a system file",
            device.display()
        );
        return None;
    }
    let text = fs::read_to_string(dir.join(inner)).ok();
    let _ = Command::new("umount").arg(dir).status();
    text
}

/// `edel system export`: prints this machine as a system file. It starts
/// from the machine's file and replaces what apply owns with what the
/// machine has, writing no defaults (ADR-008).
pub fn export() -> Result<()> {
    let mut file = match system::read_on_machine(Path::new(SYSTEM_FILE)) {
        Ok(read) => read.file,
        Err(_) if !Path::new(SYSTEM_FILE).exists() => SystemFile::default(),
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
        |home| fs::read_to_string(Path::new(home).join(".ssh/authorized_keys")).ok(),
        Path::new(DEVELOPER_FLAG).exists(),
    );
    print!("{}", toml::to_string(&file)?);
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
    authorized_keys: impl Fn(&str) -> Option<String>,
    developer: bool,
) {
    file.network.hostname = hostname.map(|h| h.trim().to_string());
    file.system.developer = developer.then_some(true);
    let admins = members(group, ADMIN_GROUP).unwrap_or_default();
    file.users.clear();
    for account in accounts(passwd).iter().filter(|a| is_person(a)) {
        let keys: Option<Vec<String>> = authorized_keys(&account.home).map(|text| {
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
            shell: (account.shell != DEFAULT_SHELL).then(|| account.shell.clone()),
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
            "format = 1\n[system]\ndeveloper = true\n[users.ali]\nadmin = true\n[users.new]\nshell = \"/bin/ash\"\n",
        )
        .unwrap()
        .file;
        let (changes, _) = plan(&file, &machine());
        let shown: Vec<String> = changes.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            shown,
            [
                "network.hostname: back to the release's",
                "users.ali.shell: set to /bin/sh",
                "users.ali: allow key logins",
                "users.ali.admin: on",
                "users.new: add, with shell /bin/ash",
                "system.developer: on",
            ]
        );
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
        let mut file = system::read(
            "format = 1\n[appearance]\ncolor_scheme = \"dark\"\n[users.gone]\nadmin = true\n",
        )
        .unwrap()
        .file;
        describe(
            &mut file,
            PASSWD,
            "admin:x:101:ci\n",
            Some("lab-1\n"),
            |home| (home == "/home/ci").then(|| "# mine\nssh-ed25519 AAAA ci@edel\n".into()),
            false,
        );
        assert_eq!(file.network.hostname.as_deref(), Some("lab-1"));
        assert_eq!(file.appearance.color_scheme.as_deref(), Some("dark"));
        assert_eq!(file.system.developer, None);
        assert_eq!(file.users.keys().collect::<Vec<_>>(), ["ali", "ci"]);
        assert_eq!(file.users["ci"].admin, Some(true));
        assert_eq!(
            file.users["ci"].ssh_keys,
            Some(vec!["ssh-ed25519 AAAA ci@edel".into()])
        );
        assert_eq!(file.users["ci"].shell, None);
        assert_eq!(file.users["ali"].admin, None);
        assert_eq!(file.users["ali"].shell.as_deref(), Some("/bin/ash"));
        let text = toml::to_string(&file).unwrap();
        assert!(system::read(&text).unwrap().problems.is_empty(), "{text}");
    }
}
