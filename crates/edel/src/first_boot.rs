//! `edel settings first-boot --console` (roadmap M6.6b): on a machine that
//! nobody can log in to, the console asks for its first person before the
//! login prompt. A VM image started with no seed has no person, no root
//! password and no root ssh key, and no image ships a default password, so
//! without this the machine could not be used at all. busybox init runs
//! `/usr/libexec/edel-console` on each console after `openrc default`, so
//! the guard has confirmed the slot before anyone is asked anything; that
//! script runs this and then getty, whatever this did, so a failure here
//! never leaves a console without its login prompt.
//!
//! The answers become the machine's settings file (`users.NAME.admin`,
//! and `users.NAME.ssh_keys` when a key is given), applied at once; a
//! password goes the way the installer's does, hashed into
//! `places::FIRST_PASSWORDS`, which apply gives with `chpasswd -e` and
//! deletes, so it is never in the settings file (ADR-006). A key also
//! turns ssh on (`services.ssh = true`), as the one image ships it off
//! (M8.17) and a key is for logging in from afar. A machine whose greeter
//! starts (the `login` feature, on a machine with a screen) is left alone:
//! its first start is the desktop's (M6.12); the one image on a server or
//! a virtual machine with no graphics card starts none, so its console asks.
//!
//! The Welcome (M6.12) makes the first person with `edel settings first-boot
//! --user NAME --new-password-stdin [KEY=VALUE...]`, which `first_person`
//! runs: refused once anyone has an account, so it never makes a second
//! person. The account `live` may run it through doas while nobody has an
//! account (privilege.rs, `live_may_set_up`), since the stick's and an unset
//! machine's desktop logs in as `live` by itself.

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result, bail};

use edel::i18n::{tr, trf};
use edel::places;

use crate::installer;
use crate::machine;
use crate::privilege;

/// Whether anyone can log in: a person has an account, or root has a
/// password or an ssh key. `passwd` and `shadow` are those files' texts,
/// `root_keys` root's `authorized_keys`.
pub fn someone_can_log_in(passwd: &str, shadow: &str, root_keys: &str) -> bool {
    let root_password = shadow.lines().any(|line| {
        let mut fields = line.split(':');
        fields.next() == Some("root")
            && fields
                .next()
                .is_some_and(|hash| hash.starts_with('$') && hash.len() > 3)
    });
    let root_key = root_keys
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    machine::has_person(passwd) || root_password || root_key
}

/// An ssh public key on one line, such as `ssh-ed25519 AAAA... me@laptop`.
pub fn is_ssh_key(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let kind = words.next().unwrap_or_default();
    let body = words.next().unwrap_or_default();
    !line.contains(['\n', '\r', '"', '\\'])
        && (kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-"))
        && body.len() >= 16
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
}

/// The `edel settings set` assignments that make `name` an administrator
/// with `key`, when one is given.
pub fn assignments(name: &str, key: Option<&str>) -> Vec<String> {
    let mut out = vec![format!("users.{name}.admin=true")];
    if let Some(key) = key {
        out.push(format!("users.{name}.ssh_keys=[\"{key}\"]"));
    }
    out
}

/// Root's `authorized_keys`, from root's home in `passwd`.
fn root_keys(passwd: &str) -> String {
    let home = passwd
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            (fields.first() == Some(&"root")).then(|| fields.get(5).copied())?
        })
        .unwrap_or("/root");
    fs::read_to_string(Path::new(home).join(".ssh/authorized_keys")).unwrap_or_default()
}

/// Whether anyone can log in to this machine now.
fn can_log_in_now() -> bool {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    let shadow = fs::read_to_string("/etc/shadow").unwrap_or_default();
    someone_can_log_in(&passwd, &shadow, &root_keys(&passwd))
}

/// `edel settings first-boot --console`: asks for the first person when
/// nobody can log in, and does nothing otherwise.
pub fn console() -> Result<()> {
    let greeter =
        Path::new(places::FEATURES_DIR).join("login.toml").exists() && edel::services::screen_now();
    if greeter || can_log_in_now() {
        return Ok(());
    }
    println!(
        "\n{}\n",
        tr(
            "Welcome to Edel OS. Nobody can log in to this computer yet, so make its first person now: an administrator who logs in with a password or an ssh key."
        )
    );
    let name = installer::ask_until(tr("Username"), None, edel::install::user_answer)?;
    let (hashed, key) = loop {
        let first =
            privilege::read_secret(tr("Password, or press Return to give an ssh key instead: "))?;
        if first.letters() == 0 {
            let key = installer::ask_until(tr("Your ssh public key, on one line"), None, |v| {
                if is_ssh_key(v) {
                    Ok(v.to_string())
                } else {
                    Err(tr("a public key such as ssh-ed25519 AAAA... me@laptop, as ~/.ssh/id_ed25519.pub holds it").to_string())
                }
            })?;
            break (None, Some(key));
        }
        let again = privilege::read_secret(tr("The same password again: "))?;
        if first.text() == again.text() {
            break (Some(installer::hash(&first)?), None);
        }
        println!("  {}", tr("the two passwords differ; type them again"));
    };
    // Another console may have been answered meanwhile.
    if can_log_in_now() {
        println!(
            "{}",
            tr("Someone was set up on another console, so this one changes nothing.")
        );
        return Ok(());
    }
    if let Some(hash) = &hashed {
        write_private(places::FIRST_PASSWORDS, &format!("{name}:{hash}\n"))?;
    }
    let mut settings = assignments(&name, key.as_deref());
    if key.is_some() && Path::new(places::FEATURES_DIR).join("ssh.toml").exists() {
        settings.push("services.ssh=true".to_string());
    }
    machine::set(&settings)?;
    machine::apply(false)?;
    println!(
        "{}",
        edel::i18n::trf("{name} can log in now.", &[("name", &name)])
    );
    Ok(())
}

/// The first assignment that names a person (`users.`), which first-boot
/// refuses: it makes only the one person `--user` names.
pub fn names_a_person(settings: &[String]) -> Option<&str> {
    settings
        .iter()
        .map(String::as_str)
        .find(|assignment| assignment.starts_with("users."))
}

/// `edel settings first-boot --user NAME --new-password-stdin [KEY=VALUE...]`
/// (M6.12): Settings' Welcome makes the computer's first person, an
/// administrator, with the settings its pages chose, and applies them.
/// Refused once anyone has an account, so it never makes a second
/// person; the password goes the installer's way, hashed into
/// `places::FIRST_PASSWORDS`, which apply gives and deletes.
pub fn first_person(name: &str, settings: &[String]) -> Result<()> {
    let passwd = fs::read_to_string("/etc/passwd")
        .with_context(|| trf("could not read {path}", &[("path", "/etc/passwd")]))?;
    if machine::has_person(&passwd) {
        bail!(
            "{}",
            tr(
                "this computer has people with accounts already, so first-boot makes nobody; add a person with edel settings set users.NAME.admin=true"
            )
        );
    }
    if let Err(why) = edel::install::user_answer(name) {
        bail!("{why}");
    }
    if let Some(assignment) = names_a_person(settings) {
        bail!(
            "{}",
            trf(
                "{assignment} names a person; first-boot makes only {name}, so leave it out",
                &[("assignment", assignment), ("name", name)]
            )
        );
    }
    let password = privilege::read_password_line(io::stdin().lock())?;
    if password.letters() == 0 {
        bail!(
            "{}",
            tr("the new password on standard input is empty; nothing was changed")
        );
    }
    let hash = installer::hash(&password)?;
    let mut all = assignments(name, None);
    all.extend(settings.iter().cloned());
    machine::set(&all)?;
    write_private(places::FIRST_PASSWORDS, &format!("{name}:{hash}\n"))?;
    machine::apply(false)?;
    println!("{}", trf("{name} can log in now.", &[("name", name)]));
    Ok(())
}

/// Writes `text` to `path`, readable by root only.
fn write_private(path: &str, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| edel::i18n::trf("could not write {path}", &[("path", path)]))?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str =
        "root:x:0:0:root:/root:/bin/sh\nsshd:x:22:22:sshd:/dev/null:/sbin/nologin\n";

    #[test]
    fn nobody_can_log_in_to_a_fresh_image() {
        assert!(!someone_can_log_in(PASSWD, "root:*:20000:0:::::\n", ""));
        assert!(!someone_can_log_in(
            PASSWD,
            "root:!:20000:0:::::\n",
            "# none yet\n\n"
        ));
    }

    #[test]
    fn a_person_or_roots_password_or_key_is_enough() {
        let person = format!("{PASSWD}ali:x:1000:1000:Ali:/home/ali:/bin/bash\n");
        assert!(someone_can_log_in(&person, "root:*::::::\n", ""));
        assert!(someone_can_log_in(
            PASSWD,
            "root:$6$salt$hash:20000:0:::::\n",
            ""
        ));
        assert!(someone_can_log_in(
            PASSWD,
            "root:*::::::\n",
            "ssh-ed25519 AAAAC3Nza me\n"
        ));
    }

    #[test]
    fn a_public_key_is_one_line_of_a_known_kind() {
        assert!(is_ssh_key(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFake ali@laptop"
        ));
        assert!(is_ssh_key(
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY= x"
        ));
        assert!(!is_ssh_key("ssh-ed25519"));
        assert!(!is_ssh_key("hello world, this is no key"));
        assert!(!is_ssh_key(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA \"quoted\""
        ));
        assert!(!is_ssh_key("ssh-ed25519 AAAA$(reboot)AAAAAAAAAA"));
    }

    #[test]
    fn only_a_users_assignment_names_a_person() {
        let with: Vec<String> = ["region.timezone=Europe/Berlin", "users.bob.admin=true"]
            .map(String::from)
            .to_vec();
        assert_eq!(names_a_person(&with), Some("users.bob.admin=true"));
        let without: Vec<String> = ["region.timezone=Europe/Berlin", "layout.preset=mac-like"]
            .map(String::from)
            .to_vec();
        assert_eq!(names_a_person(&without), None);
    }

    #[test]
    fn the_answers_make_an_administrator_with_the_key() {
        assert_eq!(assignments("ali", None), ["users.ali.admin=true"]);
        assert_eq!(
            assignments("ali", Some("ssh-ed25519 AAAA me")),
            [
                "users.ali.admin=true",
                "users.ali.ssh_keys=[\"ssh-ed25519 AAAA me\"]"
            ]
        );
    }
}
