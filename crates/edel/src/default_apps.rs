//! The default apps after the system (roadmap M6.2): `edel update`, once
//! it has done with the slots, brings the data partition's Flatpak
//! installation (`places::DEFAULT_APPS`) to the list the running slot
//! records (`places::DEFAULT_APPS_LIST`): it installs what is missing,
//! updates what is there through Flatpak's own downloads of what changed,
//! and takes out an app the release dropped, after giving it to each
//! person who used it (their `~/.var/app/ID` exists) in their own
//! installation, so swapping a default app never takes one away (ADR-008).
//! Apps never roll back with the system: Flatpak keeps their own history.
//! It also takes out the runtimes no app uses any more, never one that a
//! person's own app takes from this installation (M6.2a).
//! It keeps the translations of the languages the machine and its people
//! chose (`region.language`) besides English, the one the image carries:
//! `languages` fetches them once that set changes, at each `edel update`
//! and from the hourly `edel update --automatic` whatever it is set to,
//! never while a game plays or over a metered connection.
//! A failure here never fails the system's update; it is said and tried
//! again at the next `edel update`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::apps::{APPS_REMOTE, DefaultApps, FLATHUB_REMOTE, INSTALLATION};
use edel::i18n::{tr, trf};
use edel::places::{DEFAULT_APPS, DEFAULT_APPS_LIST};

use crate::machine::{self, Account};
use crate::update::run;

/// One thing to do to the default apps, in the order `plan` gives them.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Install the listed apps the installation lacks.
    Install(Vec<String>),
    /// Update every app and runtime in the installation.
    Update,
    /// Give a dropped app to a person who used it, in their own
    /// installation.
    Keep { person: String, id: String },
    /// Take a dropped app out of the installation.
    Remove(String),
    /// Take out runtimes no app uses any more, keeping `keep`, the runtimes
    /// people's own apps take from this installation (M6.2a).
    Tidy { keep: Vec<String> },
}

/// What brings the installation to `list` (pure): `installed` is what it
/// holds, `used` each dropped app's people, those with its data, and
/// `relied` the runtimes people's own apps take from it (M6.2a).
pub fn plan(
    list: &DefaultApps,
    installed: &BTreeSet<String>,
    used: &[(String, Vec<String>)],
    relied: &BTreeSet<String>,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let missing: Vec<String> = list
        .flatpak
        .iter()
        .filter(|id| !installed.contains(*id))
        .cloned()
        .collect();
    let installs = !missing.is_empty();
    if installs {
        steps.push(Step::Install(missing));
    }
    if !installed.is_empty() {
        steps.push(Step::Update);
    }
    for id in list
        .flatpak_dropped
        .iter()
        .filter(|id| installed.contains(*id) && !list.flatpak.contains(id))
    {
        for (person, ids) in used {
            if ids.contains(id) {
                steps.push(Step::Keep {
                    person: person.clone(),
                    id: id.clone(),
                });
            }
        }
        steps.push(Step::Remove(id.clone()));
    }
    // Last, once nothing else is left to use a runtime (M6.2a).
    if !installed.is_empty() || installs {
        steps.push(Step::Tidy {
            keep: relied.iter().cloned().collect(),
        });
    }
    steps
}

/// The runtimes a person's own apps use from the installation (M6.2a,
/// pure): `apps_runtimes` are the runtimes of their apps, `own_runtimes`
/// the runtimes they have themselves and `ours` those the installation
/// holds. Refs are compared exactly, after trimming.
pub fn relied_on(
    apps_runtimes: &[String],
    own_runtimes: &[String],
    ours: &BTreeSet<String>,
) -> BTreeSet<String> {
    let own: BTreeSet<&str> = own_runtimes
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let ours: BTreeSet<&str> = ours
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    apps_runtimes
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty() && !own.contains(r) && ours.contains(r))
        .map(String::from)
        .collect()
}

/// The remote a person's own copy of a dropped app comes from: Flathub
/// under its usual name when the default apps come from Flathub, so the
/// person has one remote for it (M6.1), else the default apps' own.
pub fn person_remote(url: &str) -> (&'static str, String) {
    if url.starts_with("https://dl.flathub.org/") {
        ("flathub", FLATHUB_REMOTE.to_string())
    } else {
        (APPS_REMOTE, url.to_string())
    }
}

/// `flatpak` on the default apps' installation.
fn flatpak() -> Command {
    let mut command = Command::new("flatpak");
    command.arg(format!("--installation={INSTALLATION}"));
    command
}

/// The apps the installation holds; none when it is not there.
fn installed() -> BTreeSet<String> {
    let listed = flatpak()
        .args(["list", "--app", "--columns=application"])
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

/// The non-empty, trimmed lines of a listing command's output; none when
/// the command fails or does not run (M6.2a).
fn listed(command: &mut Command) -> Vec<String> {
    match command.stderr(Stdio::null()).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// Takes out the runtimes no app uses any more (M6.2a), keeping `keep`: each
/// of them is pinned for the removal, so `--unused` leaves it, and the pins
/// this step added are taken off again, also when the removal failed. A
/// runtime that cannot be pinned is not removed at all.
fn tidy(keep: &[String]) -> Result<()> {
    let pinned: BTreeSet<String> = listed(flatpak().arg("pin")).into_iter().collect();
    let mut added: Vec<&String> = Vec::new();
    let mut result: Result<()> = Ok(());
    // `flatpak pin` lists a pin as `runtime/NAME/ARCH/BRANCH`; a person's
    // own pin is never added again, nor taken off afterwards.
    for runtime in keep
        .iter()
        .filter(|r| !pinned.contains(*r) && !pinned.contains(&format!("runtime/{r}")))
    {
        match run(flatpak().args(["pin", "--runtime", runtime])) {
            Ok(()) => added.push(runtime),
            Err(err) => {
                result = Err(err.context(trf(
                    "could not keep the runtime {runtime} for a person's own apps",
                    &[("runtime", runtime)],
                )));
                break;
            }
        }
    }
    if result.is_ok() {
        result = run(flatpak().args(["uninstall", "--noninteractive", "-y", "--unused"]));
    }
    for runtime in added {
        if let Err(err) = run(flatpak().args(["pin", "--remove", "--runtime", runtime])) {
            if result.is_ok() {
                result = Err(err);
            }
        }
    }
    result
}

/// The languages the machine and its people chose (`region.language`), as
/// [`edel::apps::languages_for`] keeps them.
fn wanted_languages(accounts: &[Account]) -> String {
    let mut chosen: Vec<String> = edel::settings::on_machine("region.language")
        .into_iter()
        .collect();
    for account in accounts
        .iter()
        .filter(|a| machine::is_person(a) && a.name != "root")
    {
        let file = edel::places::person_settings_in(Path::new(&account.home));
        if let Ok(text) = fs::read_to_string(file) {
            chosen.extend(edel::settings::chosen("region.language", None, Some(&text)));
        }
    }
    edel::apps::languages_for(chosen.iter().map(String::as_str))
}

/// The languages to set in the installation, when they differ from what
/// it keeps; none when they do not or there is no installation.
fn languages_to_set(accounts: &[Account]) -> Option<String> {
    if !Path::new(DEFAULT_APPS).is_dir() {
        return None;
    }
    let wanted = wanted_languages(accounts);
    let now = listed(flatpak().args(["config", "--get", "languages"]));
    (now.first() != Some(&wanted)).then_some(wanted)
}

/// Keeps the translations of `accounts`' languages in the installation:
/// when the set differs from the installation's, sets it, and with
/// `fetch` downloads the translations of the runtimes and apps there
/// (their `.Locale` extensions) at once. False when nothing changed or
/// there is no installation.
pub fn languages(accounts: &[Account], fetch: bool) -> Result<bool> {
    let Some(wanted) = languages_to_set(accounts) else {
        return Ok(false);
    };
    run(flatpak().args(["config", "--set", "languages", &wanted]))?;
    if fetch {
        let locales: Vec<String> = listed(flatpak().args(["list", "--runtime", "--columns=ref"]))
            .into_iter()
            .filter(|r| r.split('/').next().is_some_and(|n| n.ends_with(".Locale")))
            .collect();
        if !locales.is_empty() {
            run(flatpak()
                .args(["update", "--noninteractive", "-y"])
                .args(&locales))?;
        }
    }
    Ok(true)
}

/// The languages step of the hourly `edel update --automatic`: the
/// translations a newly chosen language needs, fetched within the hour
/// whatever `updates.automatic` says, as choosing the language asked for
/// them, but never while a game plays (M7.7c) or over a metered
/// connection: then the next hour tries again. Said on one line; a
/// failure is said and tried again next hour.
pub fn follow_languages() {
    let Ok(passwd) = fs::read_to_string("/etc/passwd") else {
        return;
    };
    let accounts = machine::accounts(&passwd);
    if languages_to_set(&accounts).is_none() {
        return;
    }
    let waits = if crate::shell::playing_now() {
        Some(tr("a game is playing"))
    } else if edel::network::metered() {
        Some(tr("the connection is metered"))
    } else {
        None
    };
    if let Some(why) = waits {
        println!(
            "edel update: {}",
            trf(
                "the default apps' translations for the languages chosen wait, as {why}; the next hour tries again",
                &[("why", why)]
            )
        );
        return;
    }
    match languages(&accounts, true) {
        Ok(true) => println!(
            "edel update: {}",
            tr("fetched the default apps' translations for the languages chosen")
        ),
        Ok(false) => {}
        Err(err) => eprintln!(
            "warning: {}: {err:#}",
            tr(
                "could not fetch the default apps' translations for the languages chosen; the next hour tries again"
            )
        ),
    }
}

/// The address of the default apps' remote, from the installation.
fn remote_url() -> Option<String> {
    let out = flatpak()
        .args(["remotes", "--columns=name,url"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some(APPS_REMOTE)).then(|| fields.next().map(String::from))?
        })
}

/// The people (root aside) who have data of each of `ids`.
fn used_by(accounts: &[Account], ids: &[String]) -> Vec<(String, Vec<String>)> {
    accounts
        .iter()
        .filter(|a| machine::is_person(a) && a.name != "root")
        .map(|a| {
            let had: Vec<String> = ids
                .iter()
                .filter(|id| Path::new(&a.home).join(".var/app").join(id).is_dir())
                .cloned()
                .collect();
            (a.name.clone(), had)
        })
        .filter(|(_, had)| !had.is_empty())
        .collect()
}

/// Gives `id` to `account` in their own installation, from `url`'s
/// remote, as that person.
fn keep_for(account: &Account, id: &str, url: &str) -> Result<()> {
    let (name, address) = person_remote(url);
    if name == "flathub" {
        return machine::install_apps(account, &[id.to_string()]);
    }
    let mut add = vec!["--user", "remote-add", "--if-not-exists"];
    add.extend(edel::apps::remote_add_args(&address));
    add.extend([name, address.as_str()]);
    run(&mut machine::as_user(account, "flatpak", &add))?;
    run(&mut machine::as_user(
        account,
        "flatpak",
        &["--user", "install", "--noninteractive", "-y", name, id],
    ))
}

/// Brings the default apps to the running slot's list; called by `edel
/// update` after the slots. Each step that fails is said; the others go
/// on, and the error names what failed.
pub fn follow() -> Result<()> {
    let list = match fs::read_to_string(DEFAULT_APPS_LIST) {
        Ok(text) => DefaultApps::read(&text),
        Err(_) => return Ok(()),
    };
    if list.flatpak.is_empty() && !Path::new(DEFAULT_APPS).is_dir() {
        return Ok(());
    }
    let have = installed();
    let passwd = fs::read_to_string("/etc/passwd").context("could not read /etc/passwd")?;
    let accounts = machine::accounts(&passwd);
    let url = remote_url().unwrap_or_default();
    // The runtimes people's own apps take from this installation, which
    // `Step::Tidy` keeps (M6.2a).
    let ours: BTreeSet<String> = listed(flatpak().args(["list", "--runtime", "--columns=ref"]))
        .into_iter()
        .collect();
    let mut relied = BTreeSet::new();
    for account in accounts
        .iter()
        .filter(|a| machine::is_person(a) && a.name != "root")
    {
        if !Path::new(&account.home)
            .join(".local/share/flatpak/app")
            .is_dir()
        {
            continue;
        }
        let apps = listed(&mut machine::as_user(
            account,
            "flatpak",
            &["--user", "list", "--app", "--columns=runtime"],
        ));
        let own = listed(&mut machine::as_user(
            account,
            "flatpak",
            &["--user", "list", "--runtime", "--columns=ref"],
        ));
        relied.extend(relied_on(&apps, &own, &ours));
    }
    let used = used_by(&accounts, &list.flatpak_dropped);
    let mut failed = Vec::new();
    // The languages first, so installing and updating below fetch their
    // translations.
    if let Err(err) = languages(&accounts, false) {
        let said = tr("keeping the translations of the languages chosen");
        println!("edel update: {said}");
        failed.push(format!("{said}: {err:#}"));
    }
    for step in plan(&list, &have, &used, &relied) {
        let (said, result) = match &step {
            Step::Install(ids) => (
                trf(
                    "installing the default apps {apps}",
                    &[("apps", &ids.join(", "))],
                ),
                run(flatpak()
                    .args(["install", "--noninteractive", "-y", APPS_REMOTE])
                    .args(ids)),
            ),
            Step::Update => (
                tr("updating the default apps").to_string(),
                run(flatpak().args(["update", "--noninteractive", "-y"])),
            ),
            Step::Keep { person, id } => (
                trf(
                    "keeping {app} for {person}, who used it, in their own apps",
                    &[("app", id), ("person", person)],
                ),
                accounts
                    .iter()
                    .find(|a| &a.name == person)
                    .context("the person is gone")
                    .and_then(|a| keep_for(a, id, &url)),
            ),
            Step::Remove(id) => (
                trf("taking {app} out of the default apps", &[("app", id)]),
                run(flatpak().args(["uninstall", "--noninteractive", "-y", id])),
            ),
            Step::Tidy { keep } => (
                tr("taking out the runtimes no app uses any more").to_string(),
                tidy(keep),
            ),
        };
        println!("edel update: {said}");
        // One line at the end says what failed and why, not a line here too.
        if let Err(err) = result {
            failed.push(format!("{said}: {err:#}"));
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    bail!(
        "{}",
        trf(
            "the default apps were not all brought up to date ({what}); the system's update is not affected, and the next edel update tries again",
            &[("what", &failed.join("; "))]
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_first_update_installs_the_list_and_a_later_one_updates_it() {
        let list = DefaultApps {
            flatpak: ids(&["org.edel.TestA"]),
            flatpak_dropped: vec![],
        };
        let none = BTreeSet::new();
        assert_eq!(
            plan(&list, &none, &[], &none),
            [
                Step::Install(ids(&["org.edel.TestA"])),
                Step::Tidy { keep: vec![] }
            ]
        );
        let have: BTreeSet<String> = ids(&["org.edel.TestA"]).into_iter().collect();
        assert_eq!(
            plan(&list, &have, &[], &none),
            [Step::Update, Step::Tidy { keep: vec![] }]
        );
    }

    #[test]
    fn tidy_keeps_the_runtimes_people_use_and_comes_last() {
        let list = DefaultApps {
            flatpak: ids(&["org.edel.TestA"]),
            flatpak_dropped: vec![],
        };
        let have: BTreeSet<String> = ids(&["org.edel.TestA"]).into_iter().collect();
        let relied: BTreeSet<String> = ids(&["org.freedesktop.Platform/x86_64/24.08"])
            .into_iter()
            .collect();
        assert_eq!(
            plan(&list, &have, &[], &relied).last(),
            Some(&Step::Tidy {
                keep: ids(&["org.freedesktop.Platform/x86_64/24.08"])
            })
        );
    }

    #[test]
    fn nothing_installed_and_nothing_to_install_gives_no_steps() {
        let list = DefaultApps {
            flatpak: vec![],
            flatpak_dropped: vec![],
        };
        let none = BTreeSet::new();
        assert!(plan(&list, &none, &[], &none).is_empty());
    }

    #[test]
    fn a_runtime_a_persons_app_uses_is_kept_unless_they_have_it_themselves() {
        let ours: BTreeSet<String> = ids(&[
            "org.freedesktop.Platform/x86_64/24.08",
            "org.gnome.Platform/x86_64/47",
        ])
        .into_iter()
        .collect();
        let apps = ids(&[
            "org.freedesktop.Platform/x86_64/24.08",
            "org.gnome.Platform/x86_64/47",
            "",
            "org.kde.Platform/x86_64/6.7",
        ]);
        let own = ids(&["org.gnome.Platform/x86_64/47 "]);
        assert_eq!(
            relied_on(&apps, &own, &ours),
            ids(&["org.freedesktop.Platform/x86_64/24.08"])
                .into_iter()
                .collect::<BTreeSet<String>>()
        );
    }

    #[test]
    fn a_swapped_app_is_kept_for_who_used_it_and_taken_out_after() {
        let list = DefaultApps {
            flatpak: ids(&["org.edel.TestB"]),
            flatpak_dropped: ids(&["org.edel.TestA"]),
        };
        let have: BTreeSet<String> = ids(&["org.edel.TestA"]).into_iter().collect();
        let used = vec![
            ("ci".to_string(), ids(&["org.edel.TestA"])),
            ("ali".to_string(), ids(&["org.other.App"])),
        ];
        let none = BTreeSet::new();
        assert_eq!(
            plan(&list, &have, &used, &none),
            [
                Step::Install(ids(&["org.edel.TestB"])),
                Step::Update,
                Step::Keep {
                    person: "ci".into(),
                    id: "org.edel.TestA".into()
                },
                Step::Remove("org.edel.TestA".into()),
                Step::Tidy { keep: vec![] },
            ]
        );
        // Once taken out, nothing more happens to it.
        let after: BTreeSet<String> = ids(&["org.edel.TestB"]).into_iter().collect();
        assert_eq!(
            plan(&list, &after, &used, &none),
            [Step::Update, Step::Tidy { keep: vec![] }]
        );
    }

    #[test]
    fn a_dropped_app_from_flathub_is_kept_from_flathub() {
        assert_eq!(
            person_remote("https://dl.flathub.org/repo/"),
            ("flathub", FLATHUB_REMOTE.to_string())
        );
        assert_eq!(
            person_remote("http://10.0.2.2:8002/"),
            (APPS_REMOTE, "http://10.0.2.2:8002/".to_string())
        );
    }
}
