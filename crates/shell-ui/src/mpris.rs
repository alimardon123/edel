//! Now playing (M5.9d): what any app plays, read over MPRIS (the
//! `org.mpris.MediaPlayer2` interfaces) on the session's bus. It runs on the
//! connection and zbus thread the portal has (`portal.rs`), with no thread
//! or connection of its own, and is read only while quick settings is open:
//! when it opens, [`follow`] finds every player and follows their changes
//! until the card closes, when the [`Follow`] it holds is dropped and its
//! task cancelled. What it learns goes to the event loop as an [`Event`],
//! the players in the order the card pages through them ([`order`], M5.9i);
//! the buttons ask a player through [`call`].

use std::collections::HashMap;

use futures_lite::StreamExt;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::zvariant::OwnedValue;

use crate::messages;

/// The bus names of media players all start with this.
pub const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// One player as quick settings shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// Its bus name, `org.mpris.MediaPlayer2.` and the app's own part.
    pub bus: String,
    /// The app's name as it gives it, else its bus name's own part.
    pub identity: String,
    /// Its desktop file's id as it gives it (`DesktopEntry`), for its icon
    /// (M5.9i); empty when it gives none.
    pub desktop: String,
    /// The track's title; empty when the app says none.
    pub title: String,
    /// The track's artists, joined with ", "; empty when none.
    pub artist: String,
    /// Whether it plays now (false: paused).
    pub playing: bool,
    /// The cover as a local file, when the app gives a `file://` address.
    pub cover: Option<String>,
    /// Whether the previous and next buttons work for it.
    pub can_previous: bool,
    pub can_next: bool,
}

/// What the tasks tell the event loop: every player that plays or pauses
/// now, in [`order`]; empty when none does any more.
#[derive(Debug)]
pub enum Event {
    Players(Vec<Player>),
}

/// The task that follows the players. Dropping it cancels the task, so the
/// card holds it while it is open and lets go of it when it closes.
pub struct Follow {
    /// Held only for its drop: the task runs while it lives.
    _task: zbus::Task<()>,
}

/// The players in the card's order (M5.9i): those that play first, then the
/// paused ones, each by its bus name, so the first page is what plays and
/// the pages do not move about from one read to the next (the bus lists
/// names in no promised order).
pub fn order(mut players: Vec<Player>) -> Vec<Player> {
    players.sort_by(|a, b| (!a.playing, &a.bus).cmp(&(!b.playing, &b.bus)));
    players
}

/// The artists in `xesam:artist`, joined with ", ". Apps that give one name
/// as a string get that.
fn artist_of(value: &OwnedValue) -> String {
    if let Ok(list) = Vec::<String>::try_from(value.clone()) {
        return list.join(", ");
    }
    String::try_from(value.clone()).unwrap_or_default()
}

/// The local path of a `file://` address, with its `%20`-style escapes
/// undone; None for any other address.
pub fn file_path(url: &str) -> Option<String> {
    let path = url.strip_prefix("file://")?;
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) if bytes[i] == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// A proxy to `interface` of the player at `bus`, reading its properties
/// fresh each time.
async fn proxy(
    connection: &zbus::Connection,
    bus: &str,
    interface: &'static str,
) -> zbus::Result<zbus::Proxy<'static>> {
    zbus::proxy::Builder::new(connection)
        .destination(bus.to_string())?
        .path(PATH)?
        .interface(interface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// The player at `bus` as its properties say now; None when it is stopped
/// or does not answer, so it is not one to show.
async fn read(connection: &zbus::Connection, bus: &str) -> Option<Player> {
    let player = proxy(connection, bus, PLAYER).await.ok()?;
    let root = proxy(connection, bus, ROOT).await.ok()?;
    let status = player.get_property::<String>("PlaybackStatus").await.ok()?;
    let playing = match status.as_str() {
        "Playing" => true,
        "Paused" => false,
        _ => return None,
    };
    let metadata = player
        .get_property::<OwnedValue>("Metadata")
        .await
        .ok()
        .and_then(|value| HashMap::<String, OwnedValue>::try_from(value).ok())
        .unwrap_or_default();
    let title = metadata
        .get("xesam:title")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let artist = metadata
        .get("xesam:artist")
        .map(artist_of)
        .unwrap_or_default();
    let cover = metadata
        .get("mpris:artUrl")
        .and_then(|v| String::try_from(v.clone()).ok())
        .and_then(|url| file_path(&url));
    let identity = root
        .get_property::<String>("Identity")
        .await
        .unwrap_or_else(|_| bus.strip_prefix(PREFIX).unwrap_or(bus).to_string());
    let desktop = root
        .get_property::<String>("DesktopEntry")
        .await
        .unwrap_or_default();
    Some(Player {
        bus: bus.to_string(),
        identity,
        desktop,
        title,
        artist,
        playing,
        cover,
        can_previous: player
            .get_property::<bool>("CanGoPrevious")
            .await
            .unwrap_or(false),
        can_next: player
            .get_property::<bool>("CanGoNext")
            .await
            .unwrap_or(false),
    })
}

/// Every player on the bus now that answers, in the bus's order.
async fn read_all(connection: &zbus::Connection, bus: &zbus::fdo::DBusProxy<'_>) -> Vec<Player> {
    let Ok(names) = bus.list_names().await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for name in names.iter().filter(|n| n.as_str().starts_with(PREFIX)) {
        if let Some(player) = read(connection, name.as_str()).await {
            found.push(player);
        }
    }
    found
}

/// Finds the players and follows them all, sending the list to `events`
/// whenever it changes, until the events' receiver goes or the task is
/// dropped.
async fn watch(connection: zbus::Connection, events: Sender<Event>) {
    let Ok(bus) = zbus::fdo::DBusProxy::new(&connection).await else {
        return;
    };
    // Listening before the first read, so a player that comes, goes or
    // changes in between is not missed: the names' owners, and every
    // player's `PropertiesChanged` at the MPRIS path, whoever sends it.
    let Ok(mut owners) = bus.receive_name_owner_changed().await else {
        return;
    };
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PROPERTIES)
        .and_then(|r| r.member("PropertiesChanged"))
        .and_then(|r| r.path(PATH))
        .map(|r| r.build());
    let Ok(rule) = rule else {
        return;
    };
    let Ok(mut changes) = zbus::MessageStream::for_match_rule(rule, &connection, None).await else {
        return;
    };
    let mut last: Option<Vec<Player>> = None;
    loop {
        let found = order(read_all(&connection, &bus).await);
        if last.as_ref() != Some(&found) {
            last = Some(found.clone());
            if events.send(Event::Players(found)).is_err() {
                return;
            }
        }
        let names_changed = async {
            loop {
                match owners.next().await {
                    Some(change) => {
                        if change
                            .args()
                            .is_ok_and(|a| a.name().as_str().starts_with(PREFIX))
                        {
                            return Some(());
                        }
                    }
                    None => return None,
                }
            }
        };
        let player_changed = async { changes.next().await.map(|_| ()) };
        if futures_lite::future::or(names_changed, player_changed)
            .await
            .is_none()
        {
            return;
        }
    }
}

/// Starts following the players on `connection` (the portal's), sending
/// what to show to `events`. Keep the result for as long as the card is
/// open.
pub fn follow(connection: &zbus::blocking::Connection, events: Sender<Event>) -> Follow {
    let task = connection
        .inner()
        .executor()
        .spawn(watch(connection.inner().clone(), events), "mpris follow");
    Follow { _task: task }
}

/// Asks the player at `bus` to do `method` on its player interface
/// (`PlayPause`, `Previous` or `Next`), without waiting for its answer.
pub fn call(connection: &zbus::blocking::Connection, bus: &str, method: &'static str) {
    let connection = connection.inner().clone();
    let bus = bus.to_string();
    let executor = connection.executor().clone();
    let task = async move {
        let asked = match proxy(&connection, &bus, PLAYER).await {
            Ok(proxy) => proxy.call_method(method, &()).await.map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = asked {
            eprintln!(
                "edel-shell-ui: {}",
                messages::player_call_failed(&bus, method, e)
            );
        }
    };
    executor.spawn(task, "mpris call").detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::calloop;
    use zbus::zvariant::Value;

    fn player(bus: &str, playing: bool) -> Player {
        Player {
            bus: bus.into(),
            identity: "test".into(),
            desktop: String::new(),
            title: "Song".into(),
            artist: String::new(),
            playing,
            cover: None,
            can_previous: false,
            can_next: false,
        }
    }

    #[test]
    fn the_playing_players_come_first_each_by_its_bus_name() {
        let a = player("org.mpris.MediaPlayer2.a", false);
        let b = player("org.mpris.MediaPlayer2.b", true);
        let c = player("org.mpris.MediaPlayer2.c", false);
        let d = player("org.mpris.MediaPlayer2.d", true);
        let buses = |list: Vec<Player>| list.into_iter().map(|p| p.bus).collect::<Vec<_>>();
        assert_eq!(
            buses(order(vec![c.clone(), d.clone(), a.clone(), b.clone()])),
            [
                b.bus.as_str(),
                d.bus.as_str(),
                a.bus.as_str(),
                c.bus.as_str()
            ]
        );
        assert!(order(Vec::new()).is_empty());
    }

    #[test]
    fn a_local_cover_path_is_read_out_of_its_file_address() {
        assert_eq!(
            file_path("file:///home/ci/My%20Song.png").as_deref(),
            Some("/home/ci/My Song.png")
        );
        assert_eq!(file_path("https://x/y.png"), None);
        // An escape that is not a byte stays as it is.
        assert_eq!(file_path("file:///a%zzb").as_deref(), Some("/a%zzb"));
        assert_eq!(file_path("file://host/x.png"), None);
    }

    #[test]
    fn the_artists_are_joined_and_a_single_name_is_kept() {
        let list = OwnedValue::try_from(Value::from(vec!["Lumen", "Mira"])).unwrap();
        assert_eq!(artist_of(&list), "Lumen, Mira");
        let one = OwnedValue::try_from(Value::from("Lumen")).unwrap();
        assert_eq!(artist_of(&one), "Lumen");
    }

    /// The player the test serves: the root interface, then the player.
    struct Root;

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        #[zbus(property)]
        fn identity(&self) -> String {
            "Edel mpris test".into()
        }
    }

    struct Playing;

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Playing {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            "Playing".into()
        }

        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            let mut m = HashMap::new();
            m.insert(
                "xesam:title".to_string(),
                OwnedValue::try_from(Value::from("Night Drive")).unwrap(),
            );
            m
        }
    }

    /// Waits up to ten seconds for the events `done` accepts.
    fn wait(
        events: &mut calloop::EventLoop<'_, Vec<String>>,
        seen: &mut Vec<String>,
        done: impl Fn(&[String]) -> bool,
    ) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done(seen) && std::time::Instant::now() < until {
            events
                .dispatch(std::time::Duration::from_millis(100), seen)
                .unwrap();
        }
    }

    /// Needs a session bus (`dbus-run-session -- cargo test -p edel-shell-ui
    /// mpris`), else it has nothing to check and passes: a player that
    /// serves MPRIS is shown with its title, and gone when it leaves the bus.
    #[test]
    fn a_player_on_the_bus_is_shown_and_gone_when_it_leaves() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no session bus: the players are not tried");
            return;
        }
        let host = zbus::blocking::connection::Builder::session()
            .unwrap()
            .build()
            .unwrap();
        let (sender, channel) = calloop::channel::channel();
        let mut events = calloop::EventLoop::<Vec<String>>::try_new().unwrap();
        events
            .handle()
            .insert_source(channel, |event, _, seen| {
                if let calloop::channel::Event::Msg(Event::Players(players)) = event {
                    seen.push(match players.first() {
                        Some(p) => format!("{}|{}|{}", p.identity, p.title, p.playing),
                        None => "none".to_string(),
                    });
                }
            })
            .unwrap();
        let app = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.mpris.MediaPlayer2.edelmpristest")
            .unwrap()
            .serve_at(PATH, Root)
            .unwrap()
            .serve_at(PATH, Playing)
            .unwrap()
            .build()
            .unwrap();
        let _following = follow(&host, sender);
        let mut seen = Vec::new();
        wait(&mut events, &mut seen, |s| !s.is_empty());
        assert_eq!(seen, ["Edel mpris test|Night Drive|true"]);
        drop(app);
        wait(&mut events, &mut seen, |s| s.len() > 1);
        assert_eq!(seen[1], "none");
    }
}
