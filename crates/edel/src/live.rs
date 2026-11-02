//! `edel boot live` (roadmap M3.6): the desktop image started from a USB
//! stick, or in a virtual machine nobody has set up yet, logs a person
//! called `live` in by itself, so the desktop shows without a login. It
//! makes the account, a system one kept in `/etc`'s overlay on the data
//! partition, so the slot and the settings file never hold it, and writes
//! greetd's config with a session that logs it in once at boot; the login
//! feature's `conf.d/greetd` uses that file when it exists. An installed
//! machine, which has its people, keeps the greeter.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::machine;
use crate::update::Disk;

/// The account that logs in by itself.
const LIVE_USER: &str = edel::places::LIVE_USER;
/// The login feature's greetd config, the slot's.
const GREETD: &str = "/etc/greetd/config.toml";
/// greetd's config with the live session, which `conf.d/greetd` prefers.
const GREETD_LIVE: &str = edel::places::GREETD_LIVE;
/// The person's session: the one command greetd starts after a login.
const SESSION: &str = "/usr/libexec/edel-session";

/// Whether the live person logs in: on a stick (a removable disk
/// `edel install` did not write, M6.17), or when no person has an account
/// yet, as in a virtual machine started from the image, where the greeter
/// could log nobody in.
fn wants_live(stick: bool, passwd: &str) -> bool {
    stick || !machine::has_person(passwd)
}

/// greetd's `config` with an initial session for the live person; `None`
/// when it is not TOML, so greetd keeps the slot's file as it is.
fn with_live_session(config: &str) -> Option<String> {
    let mut doc: DocumentMut = config.parse().ok()?;
    let mut session = Table::new();
    session["command"] = value(SESSION);
    session["user"] = value(LIVE_USER);
    session.decor_mut().set_prefix(
        "\n# Added by edel boot live (M3.6): the live person logs in by itself, once, at boot.\n",
    );
    doc["initial_session"] = Item::Table(session);
    Some(doc.to_string())
}

/// Links the install button's desktop file into `home`'s applications on a
/// stick, so the launcher and the presets' `install` pin find it, and takes
/// the link away otherwise (a machine nobody has set up, which has no
/// installer page). Returns whether the link is there.
fn install_button(home: &Path, stick: bool, uid: u32) -> Result<bool> {
    let apps = home.join(".local/share/applications");
    let link = apps.join(edel::places::INSTALL_APP);
    if !stick {
        match fs::remove_file(&link) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("could not remove {}", link.display()));
            }
        }
        return Ok(false);
    }
    // The folders the live account's launcher reads, made by us so the
    // account owns them (M8.22); a folder already there is left as it is.
    for dir in [home.join(".local"), home.join(".local/share"), apps.clone()] {
        if !dir.is_dir() {
            fs::create_dir(&dir).with_context(|| format!("could not make {}", dir.display()))?;
            std::os::unix::fs::chown(&dir, Some(uid), None)
                .with_context(|| format!("could not give {} to {uid}", dir.display()))?;
        }
    }
    match fs::remove_file(&link) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| format!("could not remove {}", link.display()));
        }
    }
    let target = Path::new(edel::places::LIVE_APPS_DIR).join(edel::places::INSTALL_APP);
    std::os::unix::fs::symlink(&target, &link)
        .with_context(|| format!("could not link {} to {}", link.display(), target.display()))?;
    Ok(true)
}

/// `edel boot live`, run at boot by the login feature's `edel-live`.
pub fn live() -> Result<()> {
    let sys = Path::new("/sys");
    // The one image on a machine with no graphics card starts no desktop
    // (M8.17), so nobody is logged in to one.
    if !edel::services::screen_now() {
        println!("edel boot live: this machine has no screen, so no desktop starts");
        return Ok(());
    }
    let disk = Disk::find()?;
    let stick = edel::install::running_from_stick(sys, &disk.name);
    if !wants_live(stick, &fs::read_to_string("/etc/passwd")?) {
        if edel::install::is_removable(sys, &disk.name) {
            println!(
                "edel boot live: {} is a removable disk Edel OS was installed on, not a stick, and people have accounts, so the greeter asks who logs in",
                disk.name
            );
        } else {
            println!(
                "edel boot live: {} is not a removable disk and people have accounts, so the greeter asks who logs in",
                disk.name
            );
        }
        return Ok(());
    }
    let config = fs::read_to_string(GREETD).with_context(|| format!("could not read {GREETD}"))?;
    let Some(live) = with_live_session(&config) else {
        println!("edel boot live: {GREETD} is not TOML, so nobody logs in by itself");
        return Ok(());
    };
    machine::add_live_account(LIVE_USER)?;
    if install_button(
        &Path::new("/home").join(LIVE_USER),
        stick,
        edel::places::LIVE_UID,
    )? {
        println!(
            "edel boot live: Install Edel OS is first in {LIVE_USER}'s panel and in the launcher"
        );
    }
    if let Some(dir) = Path::new(GREETD_LIVE).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(GREETD_LIVE, live).with_context(|| format!("could not write {GREETD_LIVE}"))?;
    let why = if stick {
        format!("started from a removable disk, {}", disk.name)
    } else {
        "nobody has an account yet".to_string()
    };
    println!("edel boot live: {why}, so {LIVE_USER} logs in by itself");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_ONLY: &str =
        "root:x:0:0:root:/root:/bin/sh\ngreetd:x:101:101::/var/lib/greetd:/sbin/nologin\n";
    const WITH_ALI: &str = "root:x:0:0:root:/root:/bin/sh\nali:x:1000:1000::/home/ali:/bin/sh\n";

    #[test]
    fn a_stick_or_a_machine_without_people_logs_live_in() {
        assert!(wants_live(true, WITH_ALI));
        assert!(wants_live(false, ROOT_ONLY));
        assert!(!wants_live(false, WITH_ALI));
    }

    #[test]
    fn adds_the_session_and_keeps_the_greeter() {
        let config =
            "[terminal]\nvt = 7\n\n[default_session]\ncommand = \"agreety\"\nuser = \"greetd\"\n";
        let live: toml::Table = toml::from_str(&with_live_session(config).unwrap()).unwrap();
        assert_eq!(live["initial_session"]["user"].as_str(), Some("live"));
        assert_eq!(live["initial_session"]["command"].as_str(), Some(SESSION));
        assert_eq!(live["default_session"]["command"].as_str(), Some("agreety"));
        assert_eq!(live["terminal"]["vt"].as_integer(), Some(7));
        // A session the slot's file already starts is replaced, never doubled.
        let twice = with_live_session(&with_live_session(config).unwrap()).unwrap();
        assert_eq!(twice.matches("[initial_session]").count(), 1);
        assert_eq!(with_live_session("not = [toml"), None);
    }

    #[test]
    fn the_install_button_links_on_a_stick_and_is_taken_away_elsewhere() {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::temp_dir().join(format!("edel-install-button-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let uid = fs::metadata(&home).unwrap().uid();
        let link = home
            .join(".local/share/applications")
            .join(edel::places::INSTALL_APP);
        assert!(install_button(&home, true, uid).unwrap());
        assert_eq!(
            fs::read_link(&link).unwrap(),
            Path::new(edel::places::LIVE_APPS_DIR).join(edel::places::INSTALL_APP)
        );
        assert!(!install_button(&home, false, uid).unwrap());
        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        // Taking away a link that is not there is no error.
        assert!(!install_button(&home, false, uid).unwrap());
        fs::remove_dir_all(&home).unwrap();
    }
}
