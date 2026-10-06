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
use crate::report;
use crate::update::Disk;

/// The account that logs in by itself.
const LIVE_USER: &str = "live";
/// The login feature's greetd config, the slot's.
const GREETD: &str = "/etc/greetd/config.toml";
/// greetd's config with the live session, which `conf.d/greetd` prefers.
const GREETD_LIVE: &str = "/run/edel/greetd.toml";
/// The person's session: the one command greetd starts after a login.
const SESSION: &str = "/usr/libexec/edel-session";

/// Whether the live person logs in: on a removable disk (a USB stick), or
/// when no person has an account yet, as in a virtual machine started
/// from the image, where the greeter could log nobody in.
fn wants_live(removable: bool, passwd: &str) -> bool {
    removable || !machine::has_person(passwd)
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

/// `edel boot live`, run at boot by the login feature's `edel-live`.
pub fn live() -> Result<()> {
    let disk = Disk::find()?;
    let removable = report::is_removable(Path::new("/sys"), &disk.name);
    if !wants_live(removable, &fs::read_to_string("/etc/passwd")?) {
        println!(
            "edel boot live: {} is not a removable disk and people have accounts, so the greeter asks who logs in",
            disk.name
        );
        return Ok(());
    }
    let config = fs::read_to_string(GREETD).with_context(|| format!("reading {GREETD}"))?;
    let Some(live) = with_live_session(&config) else {
        println!("edel boot live: {GREETD} is not TOML, so nobody logs in by itself");
        return Ok(());
    };
    machine::add_live_account(LIVE_USER)?;
    if let Some(dir) = Path::new(GREETD_LIVE).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(GREETD_LIVE, live).with_context(|| format!("writing {GREETD_LIVE}"))?;
    let why = if removable {
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
}
