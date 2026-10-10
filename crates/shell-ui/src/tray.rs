//! The tray (M5.2e): apps show a small icon in the panel by registering a
//! StatusNotifierItem with `org.kde.StatusNotifierWatcher`, which shell-ui
//! serves on the session's D-Bus, and is the one host of. It runs on the
//! thread zbus already has for the settings portal (`portal.rs`), shares
//! its connection and starts none of its own: each registered item gets a
//! task there that reads its icon and title, reads them again when the
//! item says they changed, and says it is gone when its app leaves the
//! bus. What it learns goes to the event loop as [`Event`]s, where the
//! tray widget (`widgets/tray.rs`) draws it. A click asks the item to
//! `Activate`, a right click for its `ContextMenu`; showing a menu over
//! `com.canonical.dbusmenu` is not part of this step.

use std::sync::{Arc, Mutex};

use futures_lite::StreamExt;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use tiny_skia::{FilterQuality, IntSize, PixmapPaint, Transform};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use crate::messages;

/// The watcher's name, as items look for it.
pub const NAME: &str = "org.kde.StatusNotifierWatcher";
const PATH: &str = "/StatusNotifierWatcher";
/// The interface items serve, and the path they serve it at when they
/// register with only their bus name.
const ITEM: &str = "org.kde.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";

/// The icon pixmaps are kept at most this many pixels across (two times
/// the widget's icon, for screens at scale 2): bigger ones are shrunk when
/// read, so an app that offers only a large one costs little to carry.
pub const MOST: i32 = 44;

/// An icon an item offers: width, height and ARGB32 bytes, as the
/// specification has it.
type Pixmap = (i32, i32, Vec<u8>);

/// One item as the panel shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    /// `service/path`, which the item registered with (see [`id_of`]).
    pub id: String,
    /// Its title, else its tooltip's: what a screen reader calls it.
    pub label: String,
    /// Its icon's name or path, in the icon themes; may be empty.
    pub name: String,
    /// Its icon pixmap as [`encode`] writes it; empty when it has none.
    pub pixels: String,
    /// The `Id` it gives, which `layout.tray_in_panel` names it by; its
    /// title when it gives none (M5.9g).
    pub app: String,
    /// Whether its `Status` is `NeedsAttention`: it has news (M5.9g).
    pub attention: bool,
}

/// What the tray's tasks tell the event loop.
#[derive(Debug)]
pub enum Event {
    /// An item registered, or changed its icon or title.
    Item(Item),
    /// The item with this id left the bus.
    Gone(String),
}

/// The ids registered now, in order, shared with the watcher's property.
type Ids = Arc<Mutex<Vec<String>>>;

/// The id an item is known by, from what it passed to
/// `RegisterStatusNotifierItem` and the bus name it sent from: a bus name
/// alone is at the usual path, an object path is on the sender, and
/// `name/path` is as it is. None for nothing usable.
pub fn id_of(service: &str, sender: Option<&str>) -> Option<String> {
    let id = if service.starts_with('/') {
        format!("{}{service}", sender.filter(|s| !s.is_empty())?)
    } else if service.contains('/') {
        service.to_string()
    } else if service.is_empty() {
        return None;
    } else {
        format!("{service}{ITEM_PATH}")
    };
    parse_id(&id)?;
    Some(id)
}

/// An id's bus name and object path: `:1.42/StatusNotifierItem` is
/// (`:1.42`, `/StatusNotifierItem`).
pub fn parse_id(id: &str) -> Option<(&str, &str)> {
    let at = id.find('/')?;
    let (name, path) = id.split_at(at);
    (!name.is_empty() && path.len() > 1).then_some((name, path))
}

/// The pixmap to show at `want` pixels across: the smallest that is at
/// least that big, else the biggest there is. One whose bytes do not fit
/// its size is no pixmap.
fn best_pixmap(list: &[Pixmap], want: i32) -> Option<&Pixmap> {
    let whole = |p: &&Pixmap| {
        let (w, h) = (i64::from(p.0), i64::from(p.1));
        w > 0 && h > 0 && w * h * 4 == p.2.len() as i64
    };
    let side = |p: &&Pixmap| p.0.max(p.1);
    let mut fine: Vec<&Pixmap> = list.iter().filter(whole).collect();
    fine.sort_by_key(side);
    fine.iter()
        .find(|p| side(p) >= want)
        .or(fine.last())
        .copied()
}

/// ARGB32 in network byte order (alpha first, not premultiplied) as the
/// premultiplied RGBA tiny-skia draws.
fn premultiplied(argb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(argb.len());
    for p in argb.chunks_exact(4) {
        let a = u32::from(p[0]);
        let mul = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        out.extend_from_slice(&[mul(p[1]), mul(p[2]), mul(p[3]), p[0]]);
    }
    out
}

/// `rgba` as a pixmap no bigger than `most` across, shrunk to fit.
fn shrunk(w: u32, h: u32, rgba: Vec<u8>, most: u32) -> Option<tiny_skia::Pixmap> {
    let whole = tiny_skia::Pixmap::from_vec(rgba, IntSize::from_wh(w, h)?)?;
    let big = w.max(h);
    if big <= most {
        return Some(whole);
    }
    let k = most as f32 / big as f32;
    let mut small = tiny_skia::Pixmap::new(
        (w as f32 * k).round().max(1.0) as u32,
        (h as f32 * k).round().max(1.0) as u32,
    )?;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    small.draw_pixmap(
        0,
        0,
        whole.as_ref(),
        &paint,
        Transform::from_scale(k, k),
        None,
    );
    Some(small)
}

/// A pixmap as a line of text for the widget's `shows`: `WxH:` and its
/// premultiplied RGBA bytes in hex.
pub fn encode(pixmap: &tiny_skia::Pixmap) -> String {
    let mut out = format!("{}x{}:", pixmap.width(), pixmap.height());
    for byte in pixmap.data() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// [`encode`]'s text read back; None for anything that is not one.
pub fn decode(text: &str) -> Option<tiny_skia::Pixmap> {
    let (size, hex) = text.split_once(':')?;
    let (w, h) = size.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    if hex.len() % 2 != 0 || !hex.is_ascii() || hex.len() / 2 != w as usize * h as usize * 4 {
        return None;
    }
    let data: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect();
    let pixmap = tiny_skia::Pixmap::from_vec(data?, IntSize::from_wh(w, h)?)?;
    // Premultiplied colours never exceed their alpha; anything that does
    // would draw wrongly, so it is not a pixmap.
    pixmap
        .pixels()
        .iter()
        .all(|p| p.red().max(p.green()).max(p.blue()) <= p.alpha())
        .then_some(pixmap)
}

/// The watcher's interface on the session's bus.
struct Watcher {
    ids: Ids,
    events: Sender<Event>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    /// An item says it is there: `service` is its bus name, its object
    /// path or both (see [`id_of`]).
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header.sender().map(|s| s.as_str());
        let id = id_of(service, sender)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(messages::tray_bad_item(service)))?;
        {
            let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
            if ids.contains(&id) {
                return Ok(());
            }
            ids.push(id.clone());
        }
        Watcher::status_notifier_item_registered(&emitter, &id).await?;
        let task = watch(
            connection.clone(),
            Arc::clone(&self.ids),
            self.events.clone(),
            id,
        );
        connection.executor().spawn(task, "tray item").detach();
        Ok(())
    }

    /// Hosts register here; shell-ui is the only one and always there.
    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.ids.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Serves the watcher on `connection` (the portal's), its events going to
/// `events`; false, with a line saying why, when another tray host
/// already has the name or without a session bus.
pub fn serve(connection: &zbus::blocking::Connection, events: Sender<Event>) -> bool {
    let watcher = Watcher {
        ids: Ids::default(),
        events,
    };
    let served = connection
        .object_server()
        .at(PATH, watcher)
        .and_then(|_| connection.request_name(NAME));
    if let Err(e) = served {
        eprintln!("edel-shell-ui: {}", messages::tray_not_served(e));
        return false;
    }
    let said = SignalEmitter::new(connection.inner(), PATH)
        .and_then(|emitter| zbus::block_on(Watcher::status_notifier_host_registered(&emitter)));
    if let Err(e) = said {
        eprintln!("edel-shell-ui: {}", messages::tray_not_said(e));
    }
    eprintln!("edel-shell-ui: serving the tray as {NAME}");
    true
}

/// Asks the item `id` to do `method` (`Activate` or `ContextMenu`) at
/// `at`, without waiting for its answer.
pub fn call(
    connection: &zbus::blocking::Connection,
    id: &str,
    method: &'static str,
    at: (i32, i32),
) {
    let connection = connection.inner().clone();
    let id = id.to_string();
    let executor = connection.executor().clone();
    let task = async move {
        let Some((name, path)) = parse_id(&id) else {
            return;
        };
        let asked = match proxy(&connection, name, path).await {
            Ok(proxy) => proxy.call_method(method, &at).await.map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = asked {
            eprintln!(
                "edel-shell-ui: {}",
                messages::tray_call_failed(&id, method, e)
            );
        }
    };
    executor.spawn(task, "tray call").detach();
}

/// A proxy to the item at `name` and `path`, reading its properties fresh
/// each time.
async fn proxy(
    connection: &zbus::Connection,
    name: &str,
    path: &str,
) -> zbus::Result<zbus::Proxy<'static>> {
    zbus::proxy::Builder::new(connection)
        .destination(name.to_string())?
        .path(path.to_string())?
        .interface(ITEM)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// The item `id` as its properties say now; None when it answers none of
/// them, so it is no item.
async fn read(connection: &zbus::Connection, id: &str) -> Option<Item> {
    let (name, path) = parse_id(id)?;
    let proxy = proxy(connection, name, path).await.ok()?;
    let icon = proxy.get_property::<String>("IconName").await;
    let pixmaps = proxy.get_property::<Vec<Pixmap>>("IconPixmap").await;
    let title = proxy.get_property::<String>("Title").await;
    let app_id = proxy.get_property::<String>("Id").await;
    let status = proxy.get_property::<String>("Status").await;
    let tip = proxy
        .get_property::<(String, Vec<Pixmap>, String, String)>("ToolTip")
        .await;
    if icon.is_err() && pixmaps.is_err() && title.is_err() && tip.is_err() {
        return None;
    }
    let pixels = pixmaps
        .ok()
        .and_then(|list| {
            let (w, h, argb) = best_pixmap(&list, MOST)?;
            shrunk(*w as u32, *h as u32, premultiplied(argb), MOST as u32)
        })
        .map(|pixmap| encode(&pixmap))
        .unwrap_or_default();
    let label = [title.as_ref().ok().cloned(), tip.ok().map(|t| t.2)]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let app = match app_id {
        Ok(app) if !app.is_empty() => app,
        _ => title.as_deref().unwrap_or_default().to_string(),
    };
    Some(Item {
        id: id.to_string(),
        label,
        name: icon.unwrap_or_default(),
        pixels,
        app,
        attention: status.is_ok_and(|s| s == "NeedsAttention"),
    })
}

/// Follows one item until its app leaves the bus: reads it, reads it
/// again whenever it signals a change, then forgets it and says so.
async fn watch(connection: zbus::Connection, ids: Ids, events: Sender<Event>, id: String) {
    if let Some((name, path)) = parse_id(&id) {
        follow(&connection, &events, &id, name, path).await;
    }
    ids.lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|i| *i != id);
    let said = match SignalEmitter::new(&connection, PATH) {
        Ok(emitter) => Watcher::status_notifier_item_unregistered(&emitter, &id).await,
        Err(e) => Err(e),
    };
    if let Err(e) = said {
        eprintln!("edel-shell-ui: {}", messages::tray_not_said(e));
    }
    let _ = events.send(Event::Gone(id));
}

/// The waiting part of [`watch`]. A false wake is a signal from the item,
/// a true one its name losing its owner.
async fn follow(
    connection: &zbus::Connection,
    events: &Sender<Event>,
    id: &str,
    name: &str,
    path: &str,
) {
    let Ok(bus) = zbus::fdo::DBusProxy::new(connection).await else {
        return;
    };
    // Listening before the first read, so a change in between is not lost.
    let Ok(gone) = bus.receive_name_owner_changed_with_args(&[(0, name)]).await else {
        return;
    };
    let Ok(item) = proxy(connection, name, path).await else {
        return;
    };
    let Ok(signals) = item.receive_all_signals().await else {
        return;
    };
    let mut wakes = signals
        .map(|_| false)
        .or(gone.map(|c| c.args().is_ok_and(|a| a.new_owner().is_none())));
    loop {
        match read(connection, id).await {
            Some(item) => {
                if events.send(Event::Item(item)).is_err() {
                    return;
                }
            }
            // Not an item at all, as its first answer showed.
            None => return,
        }
        if wakes.next().await != Some(false) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_known_by_its_service_and_path() {
        // A bus name alone is at the usual path.
        assert_eq!(
            id_of("org.kde.StatusNotifierItem-12-1", Some(":1.7")).unwrap(),
            "org.kde.StatusNotifierItem-12-1/StatusNotifierItem"
        );
        // An object path is on whoever sent it.
        assert_eq!(
            id_of("/org/ayatana/NotificationItem/x", Some(":1.7")).unwrap(),
            ":1.7/org/ayatana/NotificationItem/x"
        );
        assert_eq!(id_of("/x", None), None);
        // Both together stay as they are.
        assert_eq!(id_of(":1.9/Item", Some(":1.7")).unwrap(), ":1.9/Item");
        // Nothing, or a name with no path after it, is not an item.
        assert_eq!(id_of("", Some(":1.7")), None);
        assert_eq!(id_of(":1.9/", Some(":1.7")), None);
        assert_eq!(id_of("/", Some(":1.7")), None);
    }

    #[test]
    fn an_id_splits_into_a_bus_name_and_an_object_path() {
        assert_eq!(
            parse_id(":1.42/StatusNotifierItem"),
            Some((":1.42", "/StatusNotifierItem"))
        );
        assert_eq!(parse_id("a.b/c/d"), Some(("a.b", "/c/d")));
        assert_eq!(parse_id("a.b"), None);
        assert_eq!(parse_id("/path"), None);
        assert_eq!(parse_id("a.b/"), None);
    }

    fn solid(side: i32, argb: [u8; 4]) -> Pixmap {
        let bytes = argb.repeat((side * side) as usize);
        (side, side, bytes)
    }

    #[test]
    fn the_smallest_pixmap_that_is_big_enough_wins() {
        let list = [
            solid(48, [255, 1, 2, 3]),
            solid(16, [255, 4, 5, 6]),
            solid(32, [255, 7, 8, 9]),
        ];
        assert_eq!(best_pixmap(&list, 22).unwrap().0, 32);
        assert_eq!(best_pixmap(&list, 16).unwrap().0, 16);
        // None big enough: the biggest there is.
        assert_eq!(best_pixmap(&list, 100).unwrap().0, 48);
        assert!(best_pixmap(&[], 22).is_none());
        // A pixmap whose bytes do not fit its size is skipped.
        let broken = [
            (8, 8, vec![0; 10]),
            (0, 5, vec![]),
            solid(4, [255, 0, 0, 0]),
        ];
        assert_eq!(best_pixmap(&broken, 22).unwrap().0, 4);
    }

    #[test]
    fn argb_becomes_premultiplied_rgba() {
        // Opaque green, half-transparent white, and nothing.
        let out = premultiplied(&[255, 0x33, 0xaa, 0x66, 128, 255, 255, 255, 0, 9, 9, 9]);
        assert_eq!(out, [0x33, 0xaa, 0x66, 255, 128, 128, 128, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn a_pixmap_survives_the_text_it_travels_in() {
        let (w, h, argb) = solid(22, [255, 0x33, 0xaa, 0x66]);
        let pixmap = shrunk(w as u32, h as u32, premultiplied(&argb), MOST as u32).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (22, 22));
        let text = encode(&pixmap);
        assert!(text.starts_with("22x22:33aa66ff"), "{text}");
        let back = decode(&text).unwrap();
        assert_eq!(back.data(), pixmap.data());
        // Text that is not a pixmap is none.
        for bad in [
            "",
            "22x22",
            "2x2:00",
            "2x2:zz",
            "1x1:ff00ff00",
            "1x1:00000000é",
        ] {
            assert!(decode(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_big_pixmap_is_shrunk_and_a_small_one_left() {
        let (w, h, argb) = solid(256, [255, 10, 20, 30]);
        let big = shrunk(w as u32, h as u32, premultiplied(&argb), 44).unwrap();
        assert_eq!((big.width(), big.height()), (44, 44));
        let wide = shrunk(100, 50, vec![0; 100 * 50 * 4], 44).unwrap();
        assert_eq!((wide.width(), wide.height()), (44, 22));
        let small = shrunk(16, 16, vec![0; 16 * 16 * 4], 44).unwrap();
        assert_eq!(small.width(), 16);
    }

    /// A tray item that serves a title and a one colour pixmap.
    struct Test;

    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl Test {
        #[zbus(property)]
        fn icon_name(&self) -> String {
            "no-such-icon".into()
        }

        #[zbus(property)]
        fn icon_pixmap(&self) -> Vec<Pixmap> {
            vec![solid(22, [255, 0x33, 0xaa, 0x66])]
        }

        #[zbus(property)]
        fn title(&self) -> String {
            "edel test".into()
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

    use smithay_client_toolkit::reexports::calloop;

    /// Needs a session bus (`dbus-run-session -- cargo test`), else it
    /// has nothing to check and passes: an item registers by its object
    /// path, shows up with its title and pixmap, and goes when it leaves
    /// the bus.
    #[test]
    fn an_item_registers_shows_up_and_leaves_with_its_app() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no session bus: the tray is not tried");
            return;
        }
        let (sender, channel) = calloop::channel::channel();
        let host = zbus::blocking::connection::Builder::session()
            .unwrap()
            .build()
            .unwrap();
        assert!(serve(&host, sender));
        let mut events = calloop::EventLoop::<Vec<String>>::try_new().unwrap();
        events
            .handle()
            .insert_source(channel, |event, _, seen| {
                if let calloop::channel::Event::Msg(event) = event {
                    seen.push(match event {
                        Event::Item(item) => format!("{}|{}|{}", item.id, item.label, item.name),
                        Event::Gone(id) => format!("gone {id}"),
                    });
                }
            })
            .unwrap();
        let app = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at("/StatusNotifierItem", Test)
            .unwrap()
            .build()
            .unwrap();
        let unique = app.unique_name().unwrap().to_string();
        app.call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "RegisterStatusNotifierItem",
            &("/StatusNotifierItem",),
        )
        .unwrap();
        let id = format!("{unique}/StatusNotifierItem");
        let mut seen = Vec::new();
        wait(&mut events, &mut seen, |s| !s.is_empty());
        assert_eq!(seen, [format!("{id}|edel test|no-such-icon")]);
        // The watcher lists it, and a host is always there.
        let listed: Vec<String> = host
            .call_method(
                Some(NAME),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(NAME, "RegisteredStatusNotifierItems"),
            )
            .unwrap()
            .body()
            .deserialize::<zbus::zvariant::OwnedValue>()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(listed, std::slice::from_ref(&id));
        // It leaves with its app.
        drop(app);
        wait(&mut events, &mut seen, |s| s.len() > 1);
        assert_eq!(seen[1], format!("gone {id}"));
    }
}
