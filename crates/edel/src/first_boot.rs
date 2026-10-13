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
//! deletes, so it is never in the settings file (ADR-006). An image with a
//! greeter (the `login` feature) is left alone: its first start is the
//! desktop's (M6.12).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use edel::i18n::tr;
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
    if Path::new(places::FEATURES_DIR).join("login.toml").exists() || can_log_in_now() {
        return Ok(());
    }
    println!(
        "\n{}\n",
        tr(
            "Welcome to Edel OS. Nobody can log in to this computer yet, so make its first person now: an administrator who logs in with a password or an ssh key."
        )
    );
    let name = installer::ask_until(tr("Login name"), None, edel::install::user_answer)?;
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
    machine::set(&assignments(&name, key.as_deref()))?;
    machine::apply(false)?;
    println!(
        "{}",
        edel::i18n::trf("{name} can log in now.", &[("name", &name)])
    );
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
