//! `edel system apply` and `edel system export` (roadmap M2.2): makes this
//! machine match its system file, and describes the machine as one. Apply
//! runs at every boot from the `edel-system` service, seeding the file on
//! the first, and on demand. It applies the sections that need no network:
//! the hostname, users, their ssh keys and developer mode. It adds and
//! changes, and never deletes a user (ADR-006).

use std::fs::{self, Permissions};
use std::os::unix::fs::{MetadataExt, PermissionsExt, chown};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use edel::system::{self, SystemFile, User};

use crate::boot::{ESP_LABEL, GRUB_PREFIX};
use crate::update::{Lock, run};

/// The machine's system file, on the data partition.
pub const SYSTEM_FILE: &str = "/data/edel/system.toml";
/// Developer mode is on while this file exists (ADR-007; M7.1 acts on it).
const DEVELOPER_FLAG: &str = "/data/edel/developer";
/// What this machine changed in `/etc`: a file here differs from the slot's.
const ETC_UPPER: &str = "/data/etc/upper";
/// A volume with this label holding `system.toml` seeds a first boot.
const SEED_LABEL: &str = "EDEL-SEED";
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

/// `edel system apply [FILE]`: applies the system file, by default the
/// machine's own, which is seeded first when it is missing. A given FILE
/// becomes the machine's file once applied, so the next boot keeps it.
/// With `boot`, the `hostname` service, which runs next, sets the hostname.
pub fn apply(file: Option<&Path>, boot: bool) -> Result<()> {
    let _lock = Lock::take("system", "edel system apply")?;
    let machine = Path::new(SYSTEM_FILE);
    let path = file.unwrap_or(machine);
    if file.is_none() && !path.exists() {
        match seed(machine)? {
            Some(from) => println!("edel system: seeded {SYSTEM_FILE} from {from}"),
            None => {
                println!("edel system: no system file, so nothing to apply");
                return Ok(());
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
    let changes = apply_file(&read.file, boot)?;
    for change in &changes {
        println!("edel system: {change}");
    }
    if file.is_some_and(|f| f != machine) {
        fs::create_dir_all(Path::new(SYSTEM_FILE).parent().unwrap_or(Path::new("/")))?;
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

fn apply_file(file: &SystemFile, boot: bool) -> Result<Vec<String>> {
    let mut changes = Vec::new();
    if let Some(hostname) = &file.network.hostname {
        let current = fs::read_to_string("/etc/hostname").unwrap_or_default();
        if current.trim() != hostname {
            fs::write("/etc/hostname", format!("{hostname}\n"))?;
            changes.push(format!("hostname set to {hostname}"));
        }
        if !boot {
            run(Command::new("hostname").arg(hostname))?;
        }
    }
    for (name, user) in &file.users {
        apply_user(name, user, &mut changes).with_context(|| format!("applying [users.{name}]"))?;
    }
    let developer = file.system.developer == Some(true);
    let flag = Path::new(DEVELOPER_FLAG);
    if developer && !flag.exists() {
        fs::create_dir_all(flag.parent().unwrap_or(Path::new("/")))?;
        fs::write(flag, "")?;
        changes.push("developer mode on".into());
    } else if !developer && flag.exists() {
        fs::remove_file(flag)?;
        changes.push("developer mode off".into());
    }
    Ok(changes)
}

fn apply_user(name: &str, user: &User, changes: &mut Vec<String>) -> Result<()> {
    let shell = user.shell.as_deref().unwrap_or(DEFAULT_SHELL);
    let find = || -> Result<Option<Account>> {
        Ok(accounts(&fs::read_to_string("/etc/passwd")?)
            .into_iter()
            .find(|a| a.name == name))
    };
    let account = match find()? {
        Some(account) if !is_person(&account) => {
            changes.push(format!("left {name} alone: it is a system account"));
            return Ok(());
        }
        Some(account) => account,
        None => {
            run(Command::new("adduser").args(["-D", "-s", shell, name]))?;
            changes.push(format!("user {name} added"));
            find()?.with_context(|| format!("adduser did not add {name}"))?
        }
    };
    if let Some(text) = passwd_with_shell(&fs::read_to_string("/etc/passwd")?, name, shell) {
        replace(Path::new("/etc/passwd"), &text)?;
        changes.push(format!("{name}'s shell set to {shell}"));
    }
    if let Some(text) = shadow_unlocked(&fs::read_to_string("/etc/shadow")?, name) {
        replace(Path::new("/etc/shadow"), &text)?;
    }

    let admin = user.admin == Some(true);
    let group = fs::read_to_string("/etc/group")?;
    if admin && members(&group, ADMIN_GROUP).is_none() {
        run(Command::new("addgroup").args(["-S", ADMIN_GROUP]))?;
    }
    let is_admin = members(&fs::read_to_string("/etc/group")?, ADMIN_GROUP)
        .is_some_and(|m| m.iter().any(|m| m == name));
    if admin && !is_admin {
        run(Command::new("addgroup").args([name, ADMIN_GROUP]))?;
        changes.push(format!("{name} is an admin"));
    } else if !admin && is_admin {
        run(Command::new("delgroup").args([name, ADMIN_GROUP]))?;
        changes.push(format!("{name} is no longer an admin"));
    }

    // Absent ssh_keys leaves the file alone: keys a person added by hand
    // are theirs.
    if let Some(keys) = &user.ssh_keys {
        let text: String = keys
            .iter()
            .filter(|k| !k.contains('\n'))
            .map(|k| format!("{k}\n"))
            .collect();
        let dir = Path::new(&account.home).join(".ssh");
        let path = dir.join("authorized_keys");
        if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            fs::create_dir_all(&dir)?;
            fs::write(&path, &text)?;
            for (p, mode) in [(&dir, 0o700), (&path, 0o600)] {
                fs::set_permissions(p, Permissions::from_mode(mode))?;
                chown(p, Some(account.uid), Some(account.gid))?;
            }
            changes.push(format!("{name}'s ssh keys written"));
        }
    }
    Ok(())
}

/// Looks for a first system file: on a volume labelled EDEL-SEED, then on
/// the EFI system partition, then in the slot. Copies the first one found
/// to `target` and says where it came from.
fn seed(target: &Path) -> Result<Option<String>> {
    let esp_file = format!("{}/system.toml", GRUB_PREFIX.trim_start_matches('/'));
    let mut found = None;
    for (label, inner) in [(SEED_LABEL, "system.toml"), (ESP_LABEL, esp_file.as_str())] {
        if let Some(text) = from_volume(label, inner) {
            found = Some((text, format!("the {label} volume")));
            break;
        }
    }
    if found.is_none() {
        if let Ok(text) = fs::read_to_string(SLOT_SEED) {
            found = Some((text, SLOT_SEED.to_string()));
        }
    }
    let Some((text, from)) = found else {
        return Ok(None);
    };
    fs::create_dir_all(target.parent().unwrap_or(Path::new("/")))?;
    fs::write(target, text).with_context(|| format!("writing {}", target.display()))?;
    Ok(Some(from))
}

/// The text of `inner` on the volume labelled `label`, mounted read-only
/// for as long as it takes to read it.
fn from_volume(label: &str, inner: &str) -> Option<String> {
    let out = Command::new("findfs")
        .arg(format!("LABEL={label}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let device = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let dir = Path::new("/run/edel/seed");
    fs::create_dir_all(dir).ok()?;
    // FAT needs a charset the virt kernel has; other file systems refuse
    // the option, so they get a second try without it.
    let mount = |options: &str| {
        Command::new("mount")
            .args(["-o", options, &device])
            .arg(dir)
            .status()
            .is_ok_and(|s| s.success())
    };
    if !mount("ro,iocharset=iso8859-1") && !mount("ro") {
        eprintln!("warning: cannot mount {device} ({label}) to look for a system file");
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
    Ok(())
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
