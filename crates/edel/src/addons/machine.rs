//! On a machine (M7.2a): fetching, mounting and linking the add-ons.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::addon_state::{failed_in, names_in};
use edel::i18n::{tr, trf};
use edel::places;

use crate::release::{AddonEntry, Manifest, os_release_value, to_hex};
use crate::update::run;

use super::build::*;

/// The add-ons the machine's settings file names (`addons.installed`),
/// its profiles' and the slot's default included (M7.6, M8.17c), read
/// leniently: the default alone while there is no file yet.
pub fn wanted_on_machine() -> Vec<String> {
    edel::addon_state::installed_on_machine()
}

/// Where the add-ons of `version` are kept on the data partition.
pub fn version_dir(version: &str) -> PathBuf {
    Path::new(places::ADDONS_DIR).join(version)
}

/// Which of the `wanted` add-ons the release `version` has for this image
/// (`entries`), and a note for each it has not: built into the new slot
/// (`built_in` asks it), or not in the release at all.
pub fn fetch_plan<'a>(
    wanted: &[String],
    entries: &[&'a AddonEntry],
    version: &str,
    built_in: &dyn Fn(&str) -> bool,
) -> (Vec<&'a AddonEntry>, Vec<String>) {
    let mut fetch = Vec::new();
    let mut notes = Vec::new();
    for name in wanted {
        if let Some(entry) = entries.iter().find(|e| &e.name == name) {
            fetch.push(*entry);
        } else if built_in(name) {
            notes.push(trf(
                "add-on {name} is built into {version}, so it needs no download",
                &[("name", name), ("version", version)],
            ));
        } else {
            notes.push(trf(
                "add-on {name} is not in {version}; the update goes on without it, and edel addon list shows the add-ons there are",
                &[("name", name), ("version", version)],
            ));
        }
    }
    (fetch, notes)
}

/// Fetches the add-ons the machine names for `manifest`'s release into
/// `/data/addons/VERSION/`, each checked by size and sha256 as the slot
/// is, after the slot on `slot` is written and checked and before it is
/// switched on, so a failure leaves it off. Then only the folders of the
/// running and the new version are kept.
pub fn fetch(location: &str, manifest: &Manifest, allow_http: bool, slot: &Path) -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let entries = crate::release::addons_for(manifest, &image);
    let built_in = |name: &str| built_into(slot, name);
    // While the file names no add-ons of its own, the new slot's default
    // decides, so a desktop whose files leave the slot for an add-on keeps
    // its desktop through the update (M8.17c).
    let wanted = {
        let path = places::found(&places::machine_settings());
        let default = edel::addon_state::for_this_machine(edel::addon_state::default_in(
            &slot_file(slot, "/usr/lib/os-release"),
        ));
        match edel::settings::read_on_machine(&path) {
            Ok(read) => edel::addon_state::installed(&read.file, &default),
            Err(_) => default,
        }
    };
    let (to_fetch, notes) = fetch_plan(&wanted, &entries, &manifest.version, &built_in);
    for note in notes {
        println!("edel update: {note}");
    }
    let dir = version_dir(&manifest.version);
    if !to_fetch.is_empty() {
        fs::create_dir_all(&dir).with_context(|| {
            trf(
                "could not make {path}",
                &[("path", &dir.display().to_string())],
            )
        })?;
    }
    for entry in to_fetch {
        fetch_one(location, entry, allow_http, &dir, false)?;
    }
    prune(&[&running, &manifest.version]);
    Ok(())
}

/// Whether the slot on `device` carries feature `name`, read with
/// debugfs, so the slot is never mounted.
fn built_into(device: &Path, name: &str) -> bool {
    !slot_file(device, &format!("{}/{name}.toml", edel::features::DIR)).is_empty()
}

/// The text of `file` in the slot on `device`, read with debugfs, so the
/// slot is never mounted; empty when it has none.
fn slot_file(device: &Path, file: &str) -> String {
    Command::new("debugfs")
        .args(["-R", &format!("cat {file}")])
        .arg(device)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Fetches one add-on to `dir/NAME.ext4` through a `.part` file renamed
/// only once it is checked; one already there with the right sha256 is
/// kept. `apply` says whether `edel settings apply` fetches it (M7.2b),
/// else `edel update`, which its messages name.
fn fetch_one(
    location: &str,
    entry: &AddonEntry,
    allow_http: bool,
    dir: &Path,
    apply: bool,
) -> Result<()> {
    let who = if apply {
        "edel settings"
    } else {
        "edel update"
    };
    let dest = dir.join(format!("{}.ext4", entry.name));
    let here = fs::File::open(&dest)
        .ok()
        .and_then(|mut file| crate::update::copy_hashing(&mut file, &mut std::io::sink()).ok());
    if here.is_some_and(|(size, sum)| size == entry.size && to_hex(&sum) == entry.sha256) {
        println!(
            "{who}: {}",
            trf("add-on {name} is already here", &[("name", &entry.name)])
        );
        return Ok(());
    }
    let part = dir.join(format!("{}.ext4.part", entry.name));
    if let Err(err) = fetch_part(location, entry, allow_http, &part, &dest, apply) {
        // A failed download, check or rename leaves no part file behind.
        let _ = fs::remove_file(&part);
        return Err(err);
    }
    println!(
        "{who}: {}",
        trf(
            "add-on {name} fetched and checked",
            &[("name", &entry.name)]
        )
    );
    Ok(())
}

/// Downloads an add-on to `part`, checks it by size and sha256 and renames
/// it to `dest`. The download stops one byte past the size, so a stream that
/// is too long fails the size check instead of filling the data partition,
/// where the running system's `/var` and `/home` live.
fn fetch_part(
    location: &str,
    entry: &AddonEntry,
    allow_http: bool,
    part: &Path,
    dest: &Path,
    apply: bool,
) -> Result<()> {
    let failed = || {
        if apply {
            trf(
                "could not fetch add-on {name}; nothing changes at the next restart; check the network and run edel settings apply again",
                &[("name", &entry.name)],
            )
        } else {
            trf(
                "could not fetch add-on {name}; the new slot stays off and this system is unchanged; check the network and run edel update again",
                &[("name", &entry.name)],
            )
        }
    };
    let source = crate::release::open_addon(location, entry, allow_http).with_context(failed)?;
    let mut source = source.take(entry.size.saturating_add(1));
    let mut file = fs::File::create(part).with_context(failed)?;
    let (size, sum) = crate::update::copy_hashing(&mut source, &mut file).with_context(failed)?;
    file.sync_all().with_context(failed)?;
    drop(file);
    if size != entry.size || to_hex(&sum) != entry.sha256 {
        if apply {
            bail!(
                "{}",
                trf(
                    "refused: add-on {name} does not match the sha256 in release.toml, so it is damaged or was changed on the way; nothing of it was kept; run edel settings apply again",
                    &[("name", &entry.name)],
                )
            );
        }
        bail!(
            "{}",
            trf(
                "refused: add-on {name} does not match the sha256 in release.toml, so it is damaged or was changed on the way; the new slot stays off and this system is unchanged; run edel update again",
                &[("name", &entry.name)],
            )
        );
    }
    fs::rename(part, dest).with_context(failed)?;
    Ok(())
}

/// Takes out the add-on folders of versions in neither slot.
fn prune(keep: &[&str]) {
    let Ok(entries) = fs::read_dir(places::ADDONS_DIR) else {
        return;
    };
    let present: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    for old in stale_versions(&present, keep) {
        let path = Path::new(places::ADDONS_DIR).join(&old);
        if let Err(err) = fs::remove_dir_all(&path) {
            eprintln!(
                "warning: {}",
                trf(
                    "could not take out {path}: {why}",
                    &[
                        ("path", &path.display().to_string()),
                        ("why", &err.to_string())
                    ]
                )
            );
        }
    }
}

// ---- Pending add-ons (M7.2b): fetched by apply, tried at one boot. ----

/// What a boot does with the pending add-ons ([`boot_plan`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BootPlan {
    /// Mounted with the others, once.
    pub tried: Vec<String>,
    /// The boot before did not finish with them: they go, and are listed.
    pub failed: Vec<String>,
    /// No longer chosen: they go, and nothing is said.
    pub discard: Vec<String>,
}

/// What a boot does with the `pending` add-ons: with the one-shot mark
/// (`marked`), the chosen ones are tried; without it, a boot that tried
/// them did not finish, so the chosen ones failed. Pending ones no longer
/// chosen go either way.
pub fn boot_plan(pending: &[String], wanted: &[String], marked: bool) -> BootPlan {
    let (chosen, discard): (Vec<String>, Vec<String>) =
        pending.iter().cloned().partition(|n| wanted.contains(n));
    if marked {
        BootPlan {
            tried: chosen,
            discard,
            ..BootPlan::default()
        }
    } else {
        BootPlan {
            failed: chosen,
            discard,
            ..BootPlan::default()
        }
    }
}

/// Of the add-ons this boot tried, those to keep (still chosen) and those
/// to take away, once the boot is confirmed.
pub fn promote_plan(tried: &[String], wanted: &[String]) -> (Vec<String>, Vec<String>) {
    tried.iter().cloned().partition(|n| wanted.contains(n))
}

/// The failed list after a boot: the names still chosen, then `new`, each
/// once. A name taken out of `addons.installed` leaves the list, so
/// choosing it again tries it again.
pub fn with_failed(failed: &[String], wanted: &[String], new: &[String]) -> Vec<String> {
    let mut list: Vec<String> = failed
        .iter()
        .filter(|n| wanted.contains(n))
        .cloned()
        .collect();
    for name in new {
        if !list.contains(name) {
            list.push(name.clone());
        }
    }
    list
}

/// The chosen add-ons apply fetches: neither here, pending, built into
/// the slot nor failed at their first boot.
pub fn missing(
    wanted: &[String],
    here: &[String],
    pending: &[String],
    built_in: &dyn Fn(&str) -> bool,
    failed: &[String],
) -> Vec<String> {
    wanted
        .iter()
        .filter(|n| {
            !here.contains(n) && !pending.contains(n) && !failed.contains(n) && !built_in(n)
        })
        .cloned()
        .collect()
}

/// One name a line.
fn lines(names: &[String]) -> String {
    names.iter().map(|n| format!("{n}\n")).collect()
}

/// Where the release list of `version` came from, as `edel update` kept
/// it in `places::RELEASE_FROM`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReleaseFrom {
    pub format: u32,
    pub version: String,
    pub location: String,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

/// Keeps where the release just installed came from, so apply fetches
/// that version's add-ons from the same list; a local path is kept whole.
/// A failure is said and changes nothing else.
pub fn remember_source(location: &str, version: &str, accept: &crate::release::Accept) {
    let location = if location.contains("://") {
        location.to_string()
    } else {
        fs::canonicalize(location)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| location.to_string())
    };
    let from = ReleaseFrom {
        format: 1,
        version: version.to_string(),
        location,
        allow_http: accept.allow_http,
        channel: accept.channel.clone(),
    };
    let written = toml::to_string(&from)
        .map_err(anyhow::Error::from)
        .and_then(|text| Ok(fs::write(places::RELEASE_FROM, text)?));
    if let Err(err) = written {
        eprintln!(
            "warning: {}",
            trf(
                "could not keep where the release came from ({why}); edel settings apply fetches add-ons from the channel's list instead",
                &[("why", &format!("{err:#}"))]
            )
        );
    }
}

/// The list to fetch `running`'s add-ons from: the one `edel update` took
/// it from, else the channel's.
fn source(running: &str) -> Result<(String, crate::release::Accept)> {
    let from: Option<ReleaseFrom> = fs::read_to_string(places::RELEASE_FROM)
        .ok()
        .and_then(|text| toml::from_str(&text).ok());
    match from {
        Some(from) if from.version == running => Ok((
            from.location,
            crate::release::Accept {
                allow_downgrade: true,
                channel: from.channel,
                allow_http: from.allow_http,
            },
        )),
        _ => {
            let accept = crate::release::Accept {
                allow_downgrade: true,
                ..crate::release::Accept::default()
            };
            Ok((crate::release::default_location(&accept)?, accept))
        }
    }
}

/// The network part of `edel settings apply` (M7.2b): fetches the chosen
/// add-ons this version lacks into its pending folder, checked as an
/// update's are, and leaves the one-shot mark, so the next boot tries
/// them. Nothing is fetched when nothing is missing, so apply needs no
/// network then. A list for another version (the channel moved on) only
/// says the add-on comes with that update.
pub fn fetch_chosen() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&running);
    let pending = dir.join(places::ADDONS_PENDING);
    let wanted = wanted_on_machine();
    let failed = failed_in(&dir);
    let kept = with_failed(&failed, &wanted, &[]);
    if kept != failed {
        fs::write(dir.join(places::ADDONS_FAILED), lines(&kept))?;
    }
    for name in &kept {
        println!(
            "edel settings: {}",
            trf(
                "add-on {name} did not start at its first boot, so it is not fetched again; take it out of addons.installed and choose it again to try once more",
                &[("name", name)]
            )
        );
    }
    let built_in = |name: &str| {
        Path::new(edel::features::DIR)
            .join(format!("{name}.toml"))
            .exists()
    };
    // Pending ones no longer chosen go now; the boot would take them out.
    for name in names_in(&pending) {
        if !wanted.contains(&name) {
            let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
        }
    }
    let missing = missing(
        &wanted,
        &names_in(&dir),
        &names_in(&pending),
        &built_in,
        &kept,
    );
    if missing.is_empty() {
        return Ok(());
    }
    // Fetching is network and disk work, which waits while a game plays (M7.7c).
    crate::shell::wait_for_game("edel settings");
    let (location, accept) = source(&running)?;
    let manifest = crate::release::verified(&location, &accept)?;
    if manifest.version != running {
        for name in &missing {
            println!(
                "edel settings: {}",
                trf(
                    "add-on {name} comes with the update to {version}, as the release list has moved on from {running}",
                    &[
                        ("name", name),
                        ("version", &manifest.version),
                        ("running", &running)
                    ]
                )
            );
        }
        return Ok(());
    }
    let entries = crate::release::addons_for(&manifest, &image);
    let (to_fetch, notes) = fetch_plan(&missing, &entries, &running, &built_in);
    for note in notes {
        println!("edel settings: {note}");
    }
    if to_fetch.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(&pending).with_context(|| {
        trf(
            "could not make {path}",
            &[("path", &pending.display().to_string())],
        )
    })?;
    // The mark first: a fetch cut short leaves only checked files, which
    // the next boot tries, or none.
    fs::write(pending.join(places::ADDONS_TRY), "")?;
    for entry in &to_fetch {
        fetch_one(&location, entry, accept.allow_http, &pending, true)?;
    }
    let names: Vec<&str> = to_fetch.iter().map(|e| e.name.as_str()).collect();
    println!(
        "edel settings: {}",
        trf(
            "add-on {names} starts at the next restart",
            &[("names", &names.join(", "))]
        )
    );
    Ok(())
}

/// `edel install` (M7.2b): puts the add-ons the new machine's settings
/// file (`settings`, its text) chooses into the new data partition, mounted
/// at `data`, as pending ones with the one-shot mark, so its first boot
/// tries them as apply's are tried. Each comes from this machine's own
/// copy of the same version when it has one, else from the release list
/// this version came from, when it can be reached. Nothing here fails the
/// install: an add-on not staged is said and comes with apply later.
pub fn stage_for_install(settings: &str, data: &Path) {
    let default = edel::addon_state::running_default();
    let wanted = edel::settings::read(settings)
        .map(|read| edel::addon_state::installed(&read.file, &default))
        .unwrap_or_default();
    if wanted.is_empty() {
        return;
    }
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let here = version_dir(&running);
    let under = |place: &Path| places::on_data(data, place);
    let pending = under(&here).join(places::ADDONS_PENDING);
    let built_in = |name: &str| {
        Path::new(edel::features::DIR)
            .join(format!("{name}.toml"))
            .exists()
            && !edel::addon_state::mounted().iter().any(|m| m == name)
    };
    // An add-on running here now has started on this machine's hardware,
    // so it goes straight beside the new slot, not to be tried: a first
    // boot that misses its health once must not take the desktop away
    // (M8.17c).
    let running_now = edel::addon_state::mounted();
    let mut copied = Vec::new();
    let mut staged = Vec::new();
    let mut missing = Vec::new();
    let said = |name: &str, why: String| {
        eprintln!(
            "warning: {}",
            trf(
                "add-on {name} was not copied ({why}); it comes once the new system runs edel settings apply online",
                &[("name", name), ("why", &why)]
            )
        )
    };
    if let Err(err) = fs::create_dir_all(&pending) {
        said(&wanted.join(", "), err.to_string());
        return;
    }
    for name in wanted.iter().filter(|n| !built_in(n)) {
        let file = format!("{name}.ext4");
        if here.join(&file).is_file() {
            let running = running_now.contains(name);
            let to = if running {
                under(&here)
            } else {
                pending.clone()
            };
            match fs::copy(here.join(&file), to.join(&file)) {
                Ok(_) if running => copied.push(name.clone()),
                Ok(_) => staged.push(name.clone()),
                Err(err) => said(name, err.to_string()),
            }
        } else {
            missing.push(name.clone());
        }
    }
    if !missing.is_empty() {
        let fetched = source(&running).and_then(|(location, accept)| {
            let manifest = crate::release::verified(&location, &accept)?;
            if manifest.version != running {
                bail!(
                    "{}",
                    trf(
                        "the release list has moved on to {version}",
                        &[("version", &manifest.version)]
                    )
                );
            }
            let entries = crate::release::addons_for(&manifest, &image);
            let (to_fetch, notes) = fetch_plan(&missing, &entries, &running, &built_in);
            for note in notes {
                println!("edel install: {note}");
            }
            let mut got = Vec::new();
            for entry in to_fetch {
                fetch_one(&location, entry, accept.allow_http, &pending, true)?;
                got.push(entry.name.clone());
            }
            Ok(got)
        });
        match fetched {
            Ok(got) => staged.extend(got),
            Err(err) => said(&missing.join(", "), format!("{err:#}")),
        }
    }
    if !copied.is_empty() {
        println!(
            "edel install: {}",
            trf(
                "add-on {names} copied; it starts with the new system",
                &[("names", &copied.join(", "))]
            )
        );
    }
    if staged.is_empty() {
        let _ = fs::remove_dir_all(&pending);
        return;
    }
    if let Err(err) = fs::write(pending.join(places::ADDONS_TRY), "") {
        said(&staged.join(", "), err.to_string());
        return;
    }
    println!(
        "edel install: {}",
        trf(
            "add-on {names} copied; it starts at the first boot",
            &[("names", &staged.join(", "))]
        )
    );
}

/// Once the guard confirms this boot (M7.2b): the pending add-ons it
/// tried and that are still chosen join the others, the rest go. Returns
/// what happened, one line each; a failure is a warning, as the boot is
/// fine either way and the next one tries them again.
pub fn promote() -> Vec<String> {
    let tried: Vec<String> = fs::read_to_string(places::ADDONS_TRIED)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if tried.is_empty() {
        return Vec::new();
    }
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&version);
    let pending = dir.join(places::ADDONS_PENDING);
    let (keep, drop) = promote_plan(&tried, &edel::addon_state::installed_on_machine_or(&tried));
    let mut said = Vec::new();
    for name in &keep {
        let file = format!("{name}.ext4");
        match fs::rename(pending.join(&file), dir.join(&file)) {
            Ok(()) => said.push(format!(
                "edel update: {}",
                trf("add-on {name} started, so it stays", &[("name", name)])
            )),
            Err(err) => said.push(format!(
                "warning: {}",
                trf(
                    "add-on {name} started but could not join the others ({why}); the next boot tries it again",
                    &[("name", name), ("why", &err.to_string())]
                )
            )),
        }
    }
    for name in &drop {
        let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
    }
    let _ = fs::remove_file(places::ADDONS_TRIED);
    if names_in(&pending).is_empty() && !pending.join(places::ADDONS_TRY).exists() {
        let _ = fs::remove_dir_all(&pending);
    }
    said
}

/// `edel boot mount-data`, first: mounts each add-on the settings file
/// names that this slot's version has on `/data`, read-only at
/// `/run/edel/addons/NAME`, and lays their `/usr` over the slot's, so the
/// rest of the boot sees them. A chosen add-on that is not here is said,
/// never fatal: `edel settings apply` or the next update fetches it.
///
/// The pending ones `edel settings apply` fetched (M7.2b) are tried once:
/// with the mark there, it is taken away (and written to the disk) before
/// they mount, so a boot that hangs comes back without them; without it,
/// they are the ones a boot did not finish with, so they go, are listed as
/// failed and the failure is recorded for the person.
pub fn mount_all() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&version);
    let pending = dir.join(places::ADDONS_PENDING);
    let mark = pending.join(places::ADDONS_TRY);
    // With the settings file unreadable, the default and the fetched ones
    // still to try; an add-on the person took out never comes back by it.
    let waiting = names_in(&pending);
    let wanted = edel::addon_state::installed_on_machine_or(&waiting);
    let plan = boot_plan(&waiting, &wanted, mark.exists());
    if mark.exists() {
        fs::remove_file(&mark)?;
        // The mark must be gone from the disk before anything new mounts.
        fs::File::open(&pending)?.sync_all()?;
    }
    for name in plan.failed.iter().chain(&plan.discard) {
        let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
    }
    if !plan.failed.is_empty() {
        let failed = with_failed(&failed_in(&dir), &wanted, &plan.failed);
        if let Err(err) = fs::write(dir.join(places::ADDONS_FAILED), lines(&failed)) {
            eprintln!("warning: edel-data: could not list the failed add-ons: {err}");
        }
        let names = plan.failed.join(", ");
        println!(
            "edel-data: {}",
            trf(
                "add-on {names} did not start at the last boot, so Edel OS started without it; it is not fetched again until it is taken out of addons.installed and chosen again",
                &[("names", &names)]
            )
        );
        if let Err(err) = crate::update::record_failed_addons(&plan.failed) {
            eprintln!(
                "warning: edel-data: the add-on that did not start could not be recorded: {err:#}; Settings will not mention it"
            );
        }
    }
    let mut found = Vec::new();
    for name in wanted {
        let file = dir.join(format!("{name}.ext4"));
        let file = if !file.is_file() && plan.tried.contains(&name) {
            pending.join(format!("{name}.ext4"))
        } else {
            file
        };
        if !file.is_file() {
            if !Path::new(edel::features::DIR)
                .join(format!("{name}.toml"))
                .exists()
            {
                println!(
                    "edel-data: {}",
                    trf(
                        "add-on {name} is chosen but not here yet; edel settings apply fetches it, and it starts at the next restart",
                        &[("name", &name)]
                    )
                );
            }
            continue;
        }
        // The add-on it is built over, from its own feature file (M8.17c).
        let text = slot_file(&file, &format!("{}/{name}.toml", edel::features::DIR));
        let on = edel::features::read(&text).ok().and_then(|(f, _)| f.on);
        found.push((name, file, on));
    }
    let named: Vec<(String, Option<String>)> = found
        .iter()
        .map(|(n, _, on)| (n.clone(), on.clone()))
        .collect();
    let (order, skipped) = layers(&named);
    for (name, base) in skipped {
        println!(
            "edel-data: {}",
            trf(
                "add-on {name} needs the {base} add-on, so it is left off; add it with edel settings set addons.installed+={base}",
                &[("name", &name), ("base", &base)]
            )
        );
    }
    let mut mounts = Vec::new();
    let mut tried = String::new();
    for name in order {
        let Some((_, file, _)) = found.iter().find(|(n, _, _)| *n == name) else {
            continue;
        };
        let file = file.clone();
        let at = Path::new(places::ADDONS_RUN).join(&name);
        fs::create_dir_all(&at)?;
        if !crate::data::is_mounted(&at.to_string_lossy())? {
            let _ = Command::new("modprobe").arg("loop").status();
            let mounted = run(Command::new("mount")
                .args(["-t", "ext4", "-o", "ro,loop,nodev"])
                .arg(&file)
                .arg(&at))
            .with_context(|| {
                trf(
                    "could not mount add-on {name}; the other add-ons still mount, and the next update fetches it again",
                    &[("name", &name)],
                )
            });
            // One add-on that does not mount never keeps the others away.
            if let Err(err) = mounted {
                eprintln!("warning: edel-data: {err:#}");
                continue;
            }
        }
        if file.starts_with(&pending) {
            tried.push_str(&name);
            tried.push('\n');
        }
        mounts.push(at);
    }
    if !tried.is_empty() {
        fs::write(places::ADDONS_TRIED, &tried)?;
    }
    if mounts.is_empty() || crate::data::is_mounted("/usr")? {
        return Ok(());
    }
    run(Command::new("mount")
        .args(["-t", "overlay", "overlay", "-o"])
        .arg(format!(
            "ro,lowerdir={}",
            lowerdirs(&mounts, Path::new("/usr"))
        ))
        .arg("/usr"))
    .context(tr("could not lay the add-ons over /usr"))?;
    let names: Vec<String> = mounts
        .iter()
        .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    println!(
        "edel-data: {}",
        trf("add-ons mounted: {names}", &[("names", &names.join(", "))])
    );
    if !tried.is_empty() {
        println!(
            "edel-data: {}",
            trf(
                "trying the add-ons fetched since the last boot: {names}; they stay once this boot is confirmed",
                &[("names", &tried.trim_end().replace('\n', ", "))]
            )
        );
    }
    Ok(())
}

/// Has udev take the mounted add-ons' device rules and drivers: udev read
/// its rules and announced the devices present at boot before the add-ons
/// were over `/usr` (edel-data runs after udev-settle), so an add-on's
/// rules (printing's scanners, gaming's controllers, M7.6b; the desktop's
/// sound devices, M8.17c) would reach only devices plugged in later, and
/// maybe not even those, and its drivers (the desktop's sound, the Wi-Fi
/// packs', M8.17e) would load for no device present at boot. Replaying the
/// devices sets the rules; udev loads drivers only when a device arrives,
/// so each device that has no driver is asked for its own by its modalias,
/// as udev would have at its arrival, before the network or any session
/// starts. `data::mount_data` calls it once `/etc` has its layer
/// and the add-ons' accounts, so a rule's group (`seat`, which the desktop
/// add-on brings) is known. Only where udev runs; a failure never stops
/// the boot.
pub fn take_device_rules() {
    let mounts: Vec<PathBuf> = edel::addon_state::mounted()
        .iter()
        .map(|name| Path::new(places::ADDONS_RUN).join(name))
        .collect();
    if brings(&mounts, DRIVERS) {
        // Only the devices an add-on's driver names are asked for, so the
        // start pays one call for each of them, not one for every device
        // without a driver (a bridge, a hub's port).
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let aliases = driver_aliases(
            &Path::new("/").join(DRIVERS).join(kernel.trim()),
            kernel.trim(),
            &mounts,
        );
        let waiting: Vec<String> =
            edel::hardware::without_driver_in(Path::new(edel::hardware::BUS))
                .into_iter()
                .filter(|modalias| edel::hardware::serves_one(&aliases, modalias))
                .collect();
        let loaded = waiting
            .iter()
            .filter(|modalias| run(&mut edel::base::load_driver(modalias)).is_ok())
            .count();
        println!(
            "edel-data: loaded the add-ons' drivers for {loaded} of the {} devices that wait for one",
            waiting.len()
        );
    }
    if brings(&mounts, DEVICE_RULES) && Path::new(edel::base::DEVICE_EVENTS_SOCKET).exists() {
        let taken = run(&mut edel::base::reread_device_rules())
            .and_then(|()| run(&mut edel::base::replay_device_events()));
        match taken {
            Ok(()) => println!("edel-data: udev took the add-ons' device rules"),
            Err(err) => eprintln!(
                "warning: edel-data: udev could not take the add-ons' device rules, so a device plugged in before this start may need plugging in again: {err:#}"
            ),
        }
    }
}

/// The account lines the mounted add-ons add to `/etc/FILE` (`passwd`,
/// `group` or `shadow`), in name order.
pub fn account_lines(file: &str) -> String {
    let dir = Path::new(places::ADDON_ACCOUNTS_DIR);
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    let texts: Vec<String> = names
        .iter()
        .filter_map(|d| fs::read_to_string(d.join(file)).ok())
        .collect();
    first_of_each(&texts)
}

/// The members the mounted add-ons add to groups the slot has (M7.2b),
/// every add-on's `GROUP:USER,...` lines in name order.
pub fn member_lines() -> String {
    let mut dirs: Vec<PathBuf> = fs::read_dir(places::ADDON_ACCOUNTS_DIR)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| fs::read_to_string(d.join(MEMBERS_FILE)).ok())
        .collect()
}

/// The lines of `texts`, each name (the first field) once, in order: two
/// add-ons sharing a package add its user in the same line.
pub(super) fn first_of_each(texts: &[String]) -> String {
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for line in texts.iter().flat_map(|t| t.lines()) {
        if !line.is_empty() && seen.insert(line.split(':').next().unwrap_or_default()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The folders of `/var` the mounted add-ons bring, one per add-on.
pub fn var_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(places::ADDON_VAR_DIR)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// The links in `/etc` that make the mounted add-ons' files show there:
/// what they provide, the links apply made before, and the paths this
/// machine has a file of its own at, which are never replaced.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Links {
    /// (path under `/etc`, target) for each file an add-on provides.
    pub provided: Vec<(String, String)>,
    /// (path under `/etc`, target) for each link into the add-ons' folders.
    pub ours: Vec<(String, String)>,
    /// Paths under `/etc` that hold something of the machine's or the
    /// slot's: a file, a folder or a link that is not ours, which are never
    /// replaced.
    pub theirs: BTreeSet<String>,
}

/// Whether a mounted add-on brings a service: its init script is in
/// `places::ADDON_INIT_DIR`, from where apply links it into `/etc/init.d`.
pub fn has_services() -> bool {
    fs::read_dir(places::ADDON_INIT_DIR)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// Where an add-on's rules for udev are, relative to its mount, as
/// printing's scanners and gaming's controllers ship them (M7.6b), and
/// where its kernel drivers are, as the desktop's sound and the Wi-Fi
/// packs bring them (M8.17e).
const DEVICE_RULES: &str = "usr/lib/udev/rules.d";
const DRIVERS: &str = "usr/lib/modules";

/// The modalias patterns of the kernel drivers the add-ons mounted at
/// `mounts` bring (M8.17e): the lines of the kernel's `modules.alias` in
/// `modules` (`/usr/lib/modules/KERNEL`) whose module `modules.dep` finds
/// in one of them. The kernel's index is the slot's and stays whole.
fn driver_aliases(modules: &Path, kernel: &str, mounts: &[PathBuf]) -> Vec<String> {
    let dep = fs::read_to_string(modules.join("modules.dep")).unwrap_or_default();
    let brought: BTreeSet<String> = dep
        .lines()
        .filter_map(|line| line.split_once(':').map(|(path, _)| path))
        .filter(|path| {
            mounts
                .iter()
                .any(|m| m.join(DRIVERS).join(kernel).join(path).is_file())
        })
        .filter_map(|path| {
            let file = path.rsplit('/').next()?;
            let name = file.split(".ko").next()?;
            Some(name.replace('-', "_"))
        })
        .collect();
    let alias = fs::read_to_string(modules.join("modules.alias")).unwrap_or_default();
    alias
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("alias")).then_some(())?;
            let (pattern, module) = (words.next()?, words.next()?);
            brought
                .contains(&module.replace('-', "_"))
                .then(|| pattern.to_string())
        })
        .collect()
}

/// Whether one of the add-ons mounted at `mounts` brings something in
/// `dir`, rules for udev (`DEVICE_RULES`) or kernel drivers (`DRIVERS`),
/// which udev's first pass over the devices at boot ran without, as it
/// came before the add-ons' layer was over `/usr`.
fn brings(mounts: &[PathBuf], dir: &str) -> bool {
    mounts.iter().any(|mount| {
        fs::read_dir(mount.join(dir)).is_ok_and(|mut entries| entries.next().is_some())
    })
}

impl Links {
    /// Reads what the mounted add-ons provide and what `/etc` has now.
    pub fn read() -> Links {
        let mut provided = Vec::new();
        if let Ok(entries) = fs::read_dir(places::ADDON_INIT_DIR) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            for name in names {
                provided.push((
                    format!("init.d/{name}"),
                    format!("{}/{name}", places::ADDON_INIT_DIR),
                ));
            }
        }
        let mut addons: Vec<PathBuf> = fs::read_dir(places::ADDON_ETC_DIR)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        addons.sort();
        for dir in addons {
            let mut files = Vec::new();
            walk(&dir, "", &mut files, &|_, meta| !meta.is_dir());
            for rest in files {
                provided.push((rest.clone(), dir.join(&rest).to_string_lossy().into_owned()));
            }
        }
        let mut ours = Vec::new();
        let mut links = Vec::new();
        walk(Path::new("/etc"), "", &mut links, &|_, meta| {
            meta.file_type().is_symlink()
        });
        for rest in links {
            let Ok(target) = fs::read_link(Path::new("/etc").join(&rest)) else {
                continue;
            };
            let target = target.to_string_lossy().into_owned();
            if is_ours(&target) {
                ours.push((rest, target));
            }
        }
        let theirs = provided
            .iter()
            .map(|(rest, _)| rest)
            .filter(|rest| {
                Path::new("/etc").join(rest).symlink_metadata().is_ok()
                    && !ours.iter().any(|(mine, _)| mine == *rest)
            })
            .cloned()
            .collect();
        Links {
            provided,
            ours,
            theirs,
        }
    }

    /// The links to make, (path under `/etc`, target), and the paths of
    /// links to take out: the first add-on to provide a path has it, and a
    /// machine's own file is never replaced.
    pub fn plan(&self) -> (Vec<(String, String)>, Vec<String>) {
        let mut wanted: BTreeMap<&str, &str> = BTreeMap::new();
        for (rest, target) in &self.provided {
            if !self.theirs.contains(rest) {
                wanted.entry(rest).or_insert(target);
            }
        }
        let make = wanted
            .iter()
            .filter(|(rest, target)| !self.ours.iter().any(|(r, t)| r == *rest && t == *target))
            .map(|(rest, target)| (rest.to_string(), target.to_string()))
            .collect();
        let remove = self
            .ours
            .iter()
            .filter(|(rest, target)| wanted.get(rest.as_str()) != Some(&target.as_str()))
            .map(|(rest, _)| rest.clone())
            .collect();
        (make, remove)
    }
}

/// One line of `edel addon list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub summary: String,
    /// `edel::addon_state::State::word`: `installed`, `built-in`,
    /// `pending`, `failed`, `chosen`, `leaving` or empty.
    pub status: String,
}

/// The rows of `edel addon list`: the image's add-ons (`offered`, name and
/// summary), then any chosen or mounted one it lacks, each with where it
/// stands by `facts`.
pub fn rows(offered: &[(String, String)], facts: &edel::addon_state::Facts) -> Vec<Row> {
    let status = |name: &str| edel::addon_state::state(name, facts).word().to_string();
    let mut rows: Vec<Row> = offered
        .iter()
        .map(|(name, summary)| Row {
            name: name.clone(),
            summary: summary.clone(),
            status: status(name),
        })
        .collect();
    for name in facts.mounted.iter().chain(&facts.wanted) {
        if !rows.iter().any(|r| &r.name == name) {
            rows.push(Row {
                name: name.clone(),
                summary: String::new(),
                status: status(name),
            });
        }
    }
    rows
}

/// `edel addon list`: the add-ons this image was built with, read without
/// the network (M7.2b), each with where it stands; an image that names
/// none lists those of its channel's release for it, which needs the
/// network, and offline says so and lists what this machine has.
pub fn list() -> Result<()> {
    let mut offered = edel::addon_state::offered();
    if offered.is_empty() {
        let accept = crate::release::Accept::default();
        let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
        let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
        match crate::release::default_location(&accept)
            .and_then(|at| crate::release::verified(&at, &accept))
        {
            Ok(manifest) => {
                offered = crate::release::addons_for(&manifest, &image)
                    .into_iter()
                    .map(|a| (a.name.clone(), a.summary.clone()))
                    .collect()
            }
            Err(err) => eprintln!(
                "warning: {}",
                trf(
                    "could not read the release list ({why}), so only this machine's add-ons are shown",
                    &[("why", &format!("{err:#}"))]
                )
            ),
        }
    }
    let rows = rows(&offered, &edel::addon_state::Facts::read());
    if rows.is_empty() {
        println!("{}", tr("no add-ons for this image"));
        return Ok(());
    }
    for row in &rows {
        let line = format!("{:<16} {:<10} {}", row.name, row.status, row.summary);
        println!("{}", line.trim_end());
    }
    println!(
        "{}",
        tr(
            "to add one: edel settings set addons.installed+=NAME, then edel settings apply; it starts at the next restart"
        )
    );
    Ok(())
}

/// Whether a link's target is in the add-ons' folders, so apply made it.
fn is_ours(target: &str) -> bool {
    [places::ADDON_INIT_DIR, places::ADDON_ETC_DIR]
        .iter()
        .any(|dir| target.strip_prefix(dir).is_some_and(|r| r.starts_with('/')))
}

/// The paths under `dir`, relative to it, that `keep` picks, never
/// following a link to a folder.
fn walk(
    dir: &Path,
    under: &str,
    found: &mut Vec<String>,
    keep: &dyn Fn(&str, &fs::Metadata) -> bool,
) {
    let Ok(entries) = fs::read_dir(dir.join(under)) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rest = if under.is_empty() {
            name
        } else {
            format!("{under}/{name}")
        };
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if keep(&rest, &meta) {
            found.push(rest.clone());
        }
        if meta.is_dir() {
            walk(dir, &rest, found, keep);
        }
    }
}

/// The order the add-ons `named` (each with the add-on it is built on, if
/// any) lie over `/usr`, the topmost first, and those left off with the
/// add-on they need (M8.17c): an add-on built on another lies above it, so
/// its copy of a merged cache shows, and one whose base is not here is
/// left off, as its files need the base's.
pub fn layers(named: &[(String, Option<String>)]) -> (Vec<String>, Vec<(String, String)>) {
    let here = |base: &str| named.iter().any(|(n, on)| n == base && on.is_none());
    let mut skipped = Vec::new();
    let (mut over, mut rest) = (Vec::new(), Vec::new());
    for (name, on) in named {
        match on {
            Some(base) if here(base) => over.push(name.clone()),
            Some(base) => skipped.push((name.clone(), base.clone())),
            None => rest.push(name.clone()),
        }
    }
    over.extend(rest);
    (over, skipped)
}

/// The `lowerdir` of the slot's overlay for `/usr`: each mounted add-on's
/// `usr` folder in the order given, then the slot's own `usr` last, so the
/// lowest layer is the slot's.
pub fn lowerdirs(mounts: &[PathBuf], usr: &Path) -> String {
    let mut parts: Vec<String> = mounts
        .iter()
        .map(|mount| mount.join("usr").to_string_lossy().into_owned())
        .collect();
    parts.push(usr.to_string_lossy().into_owned());
    parts.join(":")
}

/// The names of the installed slot versions that are not kept, sorted.
pub fn stale_versions(present: &[String], keep: &[&str]) -> Vec<String> {
    let mut stale: Vec<String> = present
        .iter()
        .filter(|name| !keep.contains(&name.as_str()))
        .cloned()
        .collect();
    stale.sort();
    stale
}

#[cfg(test)]
mod tests {
    use std::process;

    use super::*;

    #[test]
    fn an_add_on_lies_over_the_one_it_is_built_on_or_stays_off() {
        let named = |list: &[(&str, Option<&str>)]| -> Vec<(String, Option<String>)> {
            list.iter()
                .map(|(n, on)| (n.to_string(), on.map(String::from)))
                .collect()
        };
        let (order, skipped) = layers(&named(&[
            ("desktop", None),
            ("printing", None),
            ("orca", Some("desktop")),
        ]));
        assert_eq!(order, ["orca", "desktop", "printing"]);
        assert!(skipped.is_empty());
        let (order, skipped) = layers(&named(&[("orca", Some("desktop")), ("printing", None)]));
        assert_eq!(order, ["printing"]);
        assert_eq!(skipped, [("orca".to_string(), "desktop".to_string())]);
    }

    #[test]
    fn only_the_aliases_of_the_drivers_an_add_on_brings_are_asked_for() {
        let root = std::env::temp_dir().join(format!("edel-driver-aliases-{}", process::id()));
        let kernel = "6.18.57-0-lts";
        let modules = root.join("slot").join(kernel);
        fs::create_dir_all(&modules).unwrap();
        fs::write(
            modules.join("modules.dep"),
            "kernel/sound/hda/controllers/snd-hda-intel.ko.gz: kernel/sound/core/snd.ko.gz\nkernel/sound/core/snd.ko.gz:\nkernel/drivers/net/virtio_net.ko.gz:\n",
        )
        .unwrap();
        fs::write(
            modules.join("modules.alias"),
            "# Aliases extracted from modules themselves.\nalias pci:v00008086d00002668sv*sd*bc*sc*i* snd_hda_intel\nalias virtio:d00000001v* virtio_net\nalias char-major-116-* snd\n",
        )
        .unwrap();
        let desktop = root.join("desktop");
        let hda = desktop
            .join(DRIVERS)
            .join(kernel)
            .join("kernel/sound/hda/controllers");
        fs::create_dir_all(&hda).unwrap();
        fs::write(hda.join("snd-hda-intel.ko.gz"), "").unwrap();
        assert_eq!(
            driver_aliases(&modules, kernel, &[desktop]),
            ["pci:v00008086d00002668sv*sd*bc*sc*i*"]
        );
        assert!(driver_aliases(&modules, kernel, &[]).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_an_add_on_with_device_rules_or_drivers_has_udev_read_them_again() {
        let root = std::env::temp_dir().join(format!("edel-device-rules-{}", process::id()));
        let (with, without, empty, drivers) = (
            root.join("gaming"),
            root.join("orca"),
            root.join("printing"),
            root.join("sound"),
        );
        fs::create_dir_all(with.join(DEVICE_RULES)).unwrap();
        fs::write(with.join(DEVICE_RULES).join("90-edel-gaming.rules"), "").unwrap();
        fs::create_dir_all(without.join("usr/bin")).unwrap();
        fs::create_dir_all(empty.join(DEVICE_RULES)).unwrap();
        let sound = drivers.join(DRIVERS).join("6.12.1-0-lts/kernel/sound/hda");
        fs::create_dir_all(&sound).unwrap();
        fs::write(sound.join("snd-hda-intel.ko.gz"), "").unwrap();
        assert!(brings(&[without.clone(), drivers.clone()], DRIVERS));
        assert!(!brings(&[without.clone(), drivers], DEVICE_RULES));
        assert!(brings(&[without.clone(), with.clone()], DEVICE_RULES));
        assert!(!brings(&[without.clone(), with], DRIVERS));
        assert!(!brings(&[without, empty], DEVICE_RULES));
        assert!(!brings(&[], DRIVERS));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lowerdirs_stacks_the_add_ons_above_the_slot() {
        let mounts = [PathBuf::from("/run/edel/addons/a"), PathBuf::from("/x")];
        assert_eq!(
            lowerdirs(&mounts, Path::new("/slot/usr")),
            "/run/edel/addons/a/usr:/x/usr:/slot/usr"
        );
        assert_eq!(lowerdirs(&[], Path::new("/slot/usr")), "/slot/usr");
    }

    #[test]
    fn stale_versions_are_the_present_ones_not_kept() {
        let present = [
            "edel-2".to_string(),
            "edel-0".to_string(),
            "edel-1".to_string(),
        ];
        assert_eq!(
            stale_versions(&present, &["edel-2"]),
            vec!["edel-0".to_string(), "edel-1".to_string()]
        );
    }

    fn entry(name: &str) -> AddonEntry {
        AddonEntry {
            name: name.into(),
            image: "edel-vm-x86_64".into(),
            base: "edel-vm-x86_64.ext4.gz".into(),
            summary: String::new(),
            file: format!("edel-vm-x86_64.{name}.ext4.gz"),
            sha256: "0".repeat(64),
            size: 1,
        }
    }

    #[test]
    fn fetches_the_wanted_add_ons_and_notes_the_rest() {
        let hello = entry("ci-hello");
        let entries = [&hello];
        let wanted = ["ci-hello", "ci-gone", "fonts-cjk"].map(String::from);
        let built_in = |name: &str| name == "fonts-cjk";
        let (fetch, notes) = fetch_plan(&wanted, &entries, "2026.10.1", &built_in);
        assert_eq!(fetch.len(), 1);
        assert_eq!(fetch[0].name, "ci-hello");
        assert_eq!(notes.len(), 2);
        assert!(
            notes[0].starts_with("add-on ci-gone is not in 2026.10.1"),
            "{}",
            notes[0]
        );
        assert!(
            notes[1].starts_with("add-on fonts-cjk is built into 2026.10.1"),
            "{}",
            notes[1]
        );
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn links_what_is_provided_and_takes_out_what_went() {
        let links = Links {
            provided: vec![
                pair("init.d/ci-hello", "/usr/lib/edel/init.d/ci-hello"),
                pair("ci-hello.conf", "/usr/lib/edel/etc/ci-hello/ci-hello.conf"),
                pair(
                    "cups/cupsd.conf",
                    "/usr/lib/edel/etc/printing/cups/cupsd.conf",
                ),
                pair("ci-hello.conf", "/usr/lib/edel/etc/other/ci-hello.conf"),
            ],
            ours: vec![
                pair("ci-hello.conf", "/usr/lib/edel/etc/ci-hello/ci-hello.conf"),
                pair("init.d/gone", "/usr/lib/edel/init.d/gone"),
            ],
            theirs: BTreeSet::from(["cups/cupsd.conf".to_string()]),
        };
        let (make, remove) = links.plan();
        assert_eq!(
            make,
            [pair("init.d/ci-hello", "/usr/lib/edel/init.d/ci-hello")]
        );
        assert_eq!(remove, ["init.d/gone"]);
        assert!(is_ours("/usr/lib/edel/init.d/x") && !is_ours("/usr/lib/edel/init.dx"));
        assert!(!is_ours("/usr/share/zoneinfo/UTC"));
    }

    #[test]
    fn lists_the_offered_and_the_chosen_add_ons() {
        let offered = [pair("fonts-cjk", "Fonts"), pair("printing", "Printers")];
        let facts = edel::addon_state::Facts {
            wanted: vec!["printing".to_string(), "ci-gone".to_string()],
            mounted: vec!["printing".to_string()],
            ..Default::default()
        };
        let rows = rows(&offered, &facts);
        let got: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.status.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("fonts-cjk", ""),
                ("printing", "installed"),
                ("ci-gone", "chosen")
            ]
        );
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_marked_boot_tries_the_pending_ones_and_one_without_the_mark_fails_them() {
        let pending = names(&["ci-hang", "gone", "virtualization"]);
        let wanted = names(&["ci-hello", "ci-hang", "virtualization"]);
        let tried = boot_plan(&pending, &wanted, true);
        assert_eq!(tried.tried, names(&["ci-hang", "virtualization"]));
        assert!(tried.failed.is_empty());
        assert_eq!(tried.discard, names(&["gone"]));
        // The boot that tried them did not finish: no mark is left.
        let failed = boot_plan(&pending, &wanted, false);
        assert!(failed.tried.is_empty());
        assert_eq!(failed.failed, names(&["ci-hang", "virtualization"]));
        assert_eq!(failed.discard, names(&["gone"]));
        assert_eq!(boot_plan(&[], &wanted, true), BootPlan::default());
    }

    #[test]
    fn a_confirmed_boot_keeps_the_tried_ones_still_chosen() {
        let (keep, drop) = promote_plan(&names(&["a", "b"]), &names(&["b", "c"]));
        assert_eq!(keep, names(&["b"]));
        assert_eq!(drop, names(&["a"]));
    }

    #[test]
    fn a_failed_add_on_is_fetched_again_only_once_chosen_again() {
        let failed = with_failed(
            &names(&["a", "b"]),
            &names(&["b", "c"]),
            &names(&["c", "b"]),
        );
        assert_eq!(failed, names(&["b", "c"]));
        assert!(with_failed(&names(&["a"]), &[], &[]).is_empty());
        let built_in = |name: &str| name == "fonts";
        let wanted = names(&["here", "pending", "fonts", "failed", "new"]);
        assert_eq!(
            missing(
                &wanted,
                &names(&["here"]),
                &names(&["pending"]),
                &built_in,
                &names(&["failed"])
            ),
            names(&["new"])
        );
    }
}
