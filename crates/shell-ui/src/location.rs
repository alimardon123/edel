//! Location (M6.14c): shell-ui's side of the place apps ask for.
//!
//! The location portal of xdg-desktop-portal keeps each app's answer in the
//! permission store, table `location`, id `location`, as `[ACCURACY, TIME]`:
//! `EXACT` or `NONE`, and a TIME. An app with no entry is asked, through the
//! Access dialog shell-ui draws, and its answer is written then. Each time the
//! portal lets an app have the place it writes the entry again with a fresh,
//! non-zero TIME; shell-ui's own writes (`app_permissions.rs`) use TIME `0`.
//! This module reads the store's `Changed` signal on the portal's connection
//! (no connection of its own, ADR-010) and so:
//!
//! - shows the sign: an app that gets the place is in use (`Kind::Location`)
//!   until GeoClue, the system's location service, stops using it. GeoClue
//!   runs on the system bus only while an app asks, so a system-bus
//!   connection is opened only while an app holds the place and dropped when
//!   it ends. The waiter polls GeoClue's `InUse` once a second, with no
//!   name owner counted as not in use, so it never starts GeoClue itself.
//! - asks every time: an app set to `ask` has its entry deleted whenever it
//!   appears, so the portal asks again next time, a Deny too.
//!
//! The place is one for the whole machine, so every app's use ends together
//! (GeoClue's `InUse` is one flag). Stop on the sign ends only a PipeWire
//! use, as a location has no client to destroy. The switch itself is the
//! portal's Lockdown (`portal.rs`): `location.enabled` in the person's file,
//! which the Location tile writes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use edel::app_permissions::{STORE_BUS, STORE_INTERFACE, STORE_PATH};
use edel::app_use::Kind;
use edel::places;
use edel::settings::{self, LOCATION_ENABLED};
use smithay_client_toolkit::reexports::calloop::channel;
use zbus::zvariant::OwnedValue;

use crate::Shell;
use crate::in_use::InUse;
use crate::messages;

/// The permission store's table and id for the place, as the portal writes them.
const TABLE: &str = "location";
const ID: &str = "location";
/// GeoClue's manager on the system bus, read for `InUse` only.
const GEOCLUE_BUS: &str = "org.freedesktop.GeoClue2";
const GEOCLUE_PATH: &str = "/org/freedesktop/GeoClue2/Manager";
const GEOCLUE_INTERFACE: &str = "org.freedesktop.GeoClue2.Manager";
/// How long GeoClue may take to say it is in use after an app got the place,
/// how often it is asked, and the back-off of a failed subscription.
const WAIT_FOR_USE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_secs(1);
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(30);

/// Whether the person's Location switch is on, from the machine's and the
/// person's settings files' text: the person's over the machine's, on when
/// neither says.
pub fn enabled(machine: Option<&str>, person: Option<&str>) -> bool {
    settings::flag(LOCATION_ENABLED, machine, person).unwrap_or(true)
}

/// What to write in the person's file to turn Location `on`, or nothing to
/// take the key out: writers never write a default, so when what applies
/// without the person's file is already what was asked, the key goes.
pub fn to_write(machine: Option<&str>, on: bool) -> Option<&'static str> {
    (enabled(machine, None) != on).then_some(if on { "true" } else { "false" })
}

/// Whether the location feature is installed (its `location.toml` in the
/// features folder), as the status area's feature is found.
pub fn feature_there(features: &Path) -> bool {
    features.join("location.toml").is_file()
}

/// Whether a store entry's words grant the place: an accuracy that is not
/// `NONE` and a TIME that is not zero. Pure.
pub fn granted(values: &[String]) -> bool {
    matches!(
        (values.first(), values.get(1)),
        (Some(accuracy), Some(time)) if accuracy != "NONE" && time != "0"
    )
}

/// Keeps the apps set to ask for the place, as `app_permissions::follow`
/// reads them: `asked` is what the list holds from now on.
pub fn set_asked(shared: &Mutex<Vec<String>>, asked: Vec<String>) {
    if let Ok(mut held) = shared.lock() {
        *held = asked;
    }
}

/// Starts following the store, on a thread named `location`, over
/// `connection` (the portal's own, so no connection of its own). `asked`
/// is read at each change, and what the thread finds says who holds the
/// place goes to `tx` as the sign's uses (`Kind::Location`, no client).
pub fn watch(
    connection: zbus::blocking::Connection,
    asked: Arc<Mutex<Vec<String>>>,
    tx: channel::Sender<Vec<InUse>>,
) {
    let holding = Holding {
        apps: Arc::default(),
        waiting: Arc::default(),
        tx,
    };
    let spawned = std::thread::Builder::new()
        .name("location".into())
        .spawn(move || {
            let mut wait = FIRST_WAIT;
            let mut logged = false;
            loop {
                match subscribe(&connection, &asked, &holding) {
                    // The store's signals ended: the bus is gone.
                    Ok(()) => return,
                    Err(e) => {
                        if !logged {
                            eprintln!("edel-shell-ui: {}", messages::location_not_watched(&e));
                            logged = true;
                        }
                        std::thread::sleep(wait);
                        wait = (wait * 2).min(LONGEST_WAIT);
                    }
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("edel-shell-ui: {}", messages::location_not_watched(e));
    }
}

/// The apps holding the place, shared by the store's watcher and the
/// GeoClue waiter. `apps` is one set for the machine, so every use ends
/// together; the waiter clears it when it ends, under the same lock, so a
/// grant that comes meanwhile starts a new waiter.
#[derive(Clone)]
struct Holding {
    apps: Arc<Mutex<BTreeSet<String>>>,
    /// Whether a waiter runs: set when one starts, cleared with `apps`.
    waiting: Arc<AtomicBool>,
    tx: channel::Sender<Vec<InUse>>,
}

/// The sign's uses for the apps holding the place: one per app, with
/// `Kind::Location` and no PipeWire client.
fn rows(apps: &BTreeSet<String>) -> Vec<InUse> {
    apps.iter()
        .map(|app| InUse {
            client: None,
            app: app.clone(),
            name: app.clone(),
            flatpak: edel::apps::is_flatpak_id(app),
            kinds: vec![Kind::Location],
        })
        .collect()
}

impl Holding {
    /// `app` got the place: it is in use now, and a waiter follows GeoClue
    /// when none runs.
    fn grant(&self, app: String) {
        let Ok(mut apps) = self.apps.lock() else {
            return;
        };
        apps.insert(app);
        let _ = self.tx.send(rows(&apps));
        if !self.waiting.swap(true, Ordering::SeqCst) {
            let holding = self.clone();
            let spawned = std::thread::Builder::new()
                .name("location-use".into())
                .spawn(move || wait_for_use(holding));
            if let Err(e) = spawned {
                apps.clear();
                self.waiting.store(false, Ordering::SeqCst);
                let _ = self.tx.send(Vec::new());
                eprintln!("edel-shell-ui: {}", messages::location_not_in_use(e));
            }
        }
    }
}

/// Reads the store's `Changed` signals for the place and acts on each one,
/// until the stream ends (`Ok`) or the subscription fails.
fn subscribe(
    connection: &zbus::blocking::Connection,
    asked: &Mutex<Vec<String>>,
    holding: &Holding,
) -> zbus::Result<()> {
    let proxy = zbus::blocking::Proxy::new(connection, STORE_BUS, STORE_PATH, STORE_INTERFACE)?;
    let signals = proxy.receive_signal_with_args("Changed", &[(0, TABLE)])?;
    // What the store holds now, so only a change is a grant. An app set to
    // ask keeps no entry from before this start.
    let mut seen = lookup(connection);
    let asked_now = asked_list(asked);
    for app in seen.keys().filter(|app| asked_now.contains(*app)) {
        forget(connection, app);
    }
    for message in signals {
        let Ok((_table, _id, deleted, _data, permissions)) = message.body().deserialize::<(
            String,
            String,
            bool,
            OwnedValue,
            HashMap<String, Vec<String>>,
        )>() else {
            continue;
        };
        if deleted {
            continue;
        }
        let (grants, forgets) = changes(&mut seen, &asked_list(asked), &permissions);
        for app in forgets {
            forget(connection, &app);
        }
        for app in grants {
            holding.grant(app);
        }
    }
    Ok(())
}

/// The apps set to ask, copied out of the lock.
fn asked_list(asked: &Mutex<Vec<String>>) -> Vec<String> {
    asked.lock().map(|list| list.clone()).unwrap_or_default()
}

/// The store's entries for the place now: each app with its TIME. A missing
/// table is an error, which means no entries.
fn lookup(connection: &zbus::blocking::Connection) -> BTreeMap<String, String> {
    let Ok(reply) = connection.call_method(
        Some(STORE_BUS),
        STORE_PATH,
        Some(STORE_INTERFACE),
        "Lookup",
        &(TABLE, ID),
    ) else {
        return BTreeMap::new();
    };
    match reply
        .body()
        .deserialize::<(HashMap<String, Vec<String>>, OwnedValue)>()
    {
        Ok((permissions, _)) => permissions
            .into_iter()
            .map(|(app, values)| (app, values.get(1).cloned().unwrap_or_default()))
            .collect(),
        Err(_) => BTreeMap::new(),
    }
}

/// What a store change says, pure: the apps newly granted (a TIME that
/// differs from the one `seen` kept, and an entry that grants) and the apps
/// set to ask whose entry is there, to delete. `seen` takes the change and
/// forgets the apps the store no longer lists.
fn changes(
    seen: &mut BTreeMap<String, String>,
    asked: &[String],
    permissions: &HashMap<String, Vec<String>>,
) -> (Vec<String>, Vec<String>) {
    let mut grants = Vec::new();
    let mut forgets = Vec::new();
    let sorted: BTreeMap<String, Vec<String>> = permissions.clone().into_iter().collect();
    for (app, values) in sorted {
        let time = values.get(1).cloned().unwrap_or_default();
        if seen.get(&app) != Some(&time) && granted(&values) {
            grants.push(app.clone());
        }
        if asked.contains(&app) {
            forgets.push(app.clone());
        }
        seen.insert(app, time);
    }
    seen.retain(|app, _| permissions.contains_key(app));
    (grants, forgets)
}

/// Deletes the place's entry of `app` in the store, so the portal asks the
/// person again next time (`location asked again next time`).
fn forget(connection: &zbus::blocking::Connection, app: &str) {
    let gone = connection.call_method(
        Some(STORE_BUS),
        STORE_PATH,
        Some(STORE_INTERFACE),
        "DeletePermission",
        &(TABLE, ID, app),
    );
    match gone {
        Ok(_) => eprintln!("edel-shell-ui: location asked again next time for {app}"),
        Err(e) => eprintln!(
            "edel-shell-ui: {}",
            messages::app_permission_not_set(app, ID, e)
        ),
    }
}

/// Whether GeoClue is using the place: a name with no owner is not, so
/// nothing here starts GeoClue.
fn geoclue_in_use(system: &zbus::blocking::Connection) -> zbus::Result<bool> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(system)?;
    if !dbus.name_has_owner(zbus::names::BusName::try_from(GEOCLUE_BUS)?)? {
        return Ok(false);
    }
    // Properties are read fresh each time, never from a cache.
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(system)
        .destination(GEOCLUE_BUS)?
        .path(GEOCLUE_PATH)?
        .interface(GEOCLUE_INTERFACE)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()?;
    proxy.get_property::<bool>("InUse")
}

/// Waits while GeoClue uses the place, on a system-bus connection of its own
/// that lives only here. It waits up to `WAIT_FOR_USE` for GeoClue to say it
/// is in use, then until it does not (or its name has no owner). When GeoClue
/// never says so, or there is no system bus, the sign ends the use after that
/// wait and says why once. Then it clears the apps and sends no uses.
fn wait_for_use(holding: Holding) {
    let mut reason = None;
    match zbus::blocking::Connection::system() {
        Err(e) => reason = Some(e.to_string()),
        Ok(system) => {
            let deadline = Instant::now() + WAIT_FOR_USE;
            loop {
                match geoclue_in_use(&system) {
                    Ok(true) => {
                        while geoclue_in_use(&system).unwrap_or(false) {
                            std::thread::sleep(POLL);
                        }
                        break;
                    }
                    Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL),
                    Ok(false) => {
                        reason = Some("GeoClue did not start using the place within 5 s".into());
                        break;
                    }
                    Err(e) => {
                        reason = Some(e.to_string());
                        break;
                    }
                }
            }
        }
    }
    if let Some(reason) = reason {
        eprintln!("edel-shell-ui: {}", messages::location_not_in_use(reason));
    }
    let Ok(mut apps) = holding.apps.lock() else {
        return;
    };
    apps.clear();
    holding.waiting.store(false, Ordering::SeqCst);
    let _ = holding.tx.send(Vec::new());
}

impl Shell {
    /// Turns Location on or off in the person's file, as the Do not disturb
    /// tile does (M6.14c): the file's line is taken out when it is what applies
    /// without it, and what is open shows it at once. The portal follows the
    /// file's change through `settings_changed`.
    pub(crate) fn set_location(&mut self, on: bool) {
        let (machine, _) = crate::settings_texts();
        let value = to_write(machine.as_deref(), on);
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => match settings::write(&path, LOCATION_ENABLED, value) {
                Ok(()) => {
                    eprintln!("edel-shell-ui: location {}", if on { "on" } else { "off" });
                    if let Some(card) = &mut self.quick {
                        card.set_location(on);
                    }
                }
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::location_not_kept(format!("{e:#}"))
                ),
            },
            None => eprintln!("edel-shell-ui: {}", messages::LOCATION_NO_HOME),
        }
        self.draw_quick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn an_entry_grants_the_place_when_it_is_exact_or_city_with_a_time() {
        assert!(granted(&words(&["EXACT", "123"])));
        assert!(granted(&words(&["CITY", "5"])));
        assert!(
            !granted(&words(&["EXACT", "0"])),
            "a TIME of 0 is shell-ui's own"
        );
        assert!(!granted(&words(&["NONE", "123"])), "NONE is no place");
        assert!(!granted(&words(&["CITY"])), "no TIME is no grant");
    }

    #[test]
    fn the_switch_is_on_unless_the_person_or_the_machine_turns_it_off() {
        let off = "format = 1\n[location]\nenabled = false\n";
        let on = "format = 1\n[location]\nenabled = true\n";
        assert!(enabled(None, None));
        assert!(!enabled(Some(off), None));
        assert!(!enabled(None, Some(off)));
        assert!(
            enabled(Some(off), Some(on)),
            "the person's over the machine's"
        );
        assert!(!enabled(Some(on), Some(off)));
    }

    #[test]
    fn the_switch_writes_only_what_differs_from_what_applies() {
        let off = "format = 1\n[location]\nenabled = false\n";
        // On the release's default, off is written and on takes the line out.
        assert_eq!(to_write(None, false), Some("false"));
        assert_eq!(to_write(None, true), None);
        // A machine that turns it off: on is written, off is what applies.
        assert_eq!(to_write(Some(off), true), Some("true"));
        assert_eq!(to_write(Some(off), false), None);
    }

    #[test]
    fn a_store_change_grants_a_fresh_time_and_asks_again_for_the_asked() {
        let entry = |values: &[&str]| words(values);
        let mut seen = BTreeMap::from([("org.a.Maps".to_string(), "5".to_string())]);
        let permissions = HashMap::from([
            ("org.a.Maps".to_string(), entry(&["EXACT", "9"])),
            ("org.a.Weather".to_string(), entry(&["EXACT", "7"])),
            ("org.a.Asked".to_string(), entry(&["NONE", "8"])),
        ]);
        let (grants, forgets) = changes(&mut seen, &["org.a.Asked".to_string()], &permissions);
        assert_eq!(grants, ["org.a.Maps", "org.a.Weather"]);
        assert_eq!(forgets, ["org.a.Asked"], "a Deny is asked again too");
        assert_eq!(seen.get("org.a.Maps").map(String::as_str), Some("9"));
    }

    #[test]
    fn an_unchanged_time_is_no_grant_and_a_gone_app_is_forgotten() {
        let mut seen = BTreeMap::from([
            ("org.a.Maps".to_string(), "9".to_string()),
            ("org.a.Gone".to_string(), "3".to_string()),
        ]);
        let permissions = HashMap::from([("org.a.Maps".to_string(), words(&["EXACT", "9"]))]);
        let (grants, forgets) = changes(&mut seen, &[], &permissions);
        assert!(grants.is_empty(), "the same TIME is what was there");
        assert!(forgets.is_empty());
        assert_eq!(seen.keys().collect::<Vec<_>>(), ["org.a.Maps"]);
    }

    #[test]
    fn an_app_holding_the_place_is_a_location_use_without_a_client() {
        let apps = BTreeSet::from(["org.gnome.Maps".to_string(), "ffmpeg".to_string()]);
        let uses = rows(&apps);
        assert_eq!(uses.len(), 2);
        assert!(uses.iter().all(|u| u.client.is_none()));
        assert!(uses.iter().all(|u| u.kinds == [Kind::Location]));
        let maps = uses.iter().find(|u| u.app == "org.gnome.Maps").unwrap();
        let ffmpeg = uses.iter().find(|u| u.app == "ffmpeg").unwrap();
        assert!(maps.flatpak, "a Flatpak id is a Flatpak app");
        assert!(!ffmpeg.flatpak);
    }
}
