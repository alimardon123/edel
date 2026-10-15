//! The in-use sign (M6.14b): which apps use the camera, the microphone or
//! the screen right now. PipeWire's graph comes from `pw-dump --monitor` on a
//! thread of its own, read as a stream of batches: the first is the whole
//! graph, each later one the objects that changed, and an object with no
//! `info` is gone. What shell-ui does with the list is here too: it logs it,
//! writes each use that ends to the person's record of use
//! (`edel::app_use`) and shows it on the status area (`widgets/status.rs`)
//! and in the card (`in_use_card.rs`).
//!
//! Nothing polls: pw-dump says what changed, and the only wait is a back-off
//! after pw-dump fails. The typed structs below name the fields used and
//! skip the rest, so shell-ui never holds a whole JSON value (its memory
//! budget). Pure functions are tested without PipeWire.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufReader;
use std::process::{ChildStdout, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::de::Deserializer;
use smithay_client_toolkit::reexports::calloop::channel;

use edel::app_use::{self, Kind, Use};
use edel::i18n::{tr, trf};
use edel::places;

use crate::Shell;

/// One app using one or more of the camera, the microphone and the screen.
/// `client` is the PipeWire client's id (or the stream's, when the stream
/// has no client), which `pw-cli destroy` takes; `app` the record's id, the
/// Flatpak app id or the program's name; `name` what people read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InUse {
    pub client: u32,
    pub app: String,
    pub name: String,
    pub flatpak: bool,
    /// In `Kind::ALL` order, without repeats.
    pub kinds: Vec<Kind>,
}

/// A scalar of pw-dump's JSON: a property may be a string, a number or a
/// bool, and each is read as its text.
#[derive(Deserialize)]
#[serde(untagged)]
enum Scalar {
    S(String),
    N(i64),
    F(f64),
    B(bool),
}

fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Scalar>::deserialize(d)? {
        Some(Scalar::S(s)) => Some(s),
        Some(Scalar::N(n)) => Some(n.to_string()),
        Some(Scalar::F(f)) => Some(f.to_string()),
        Some(Scalar::B(b)) => Some(b.to_string()),
        None => None,
    })
}

/// The properties shell-ui reads; every other one is skipped unread.
#[derive(Deserialize)]
struct Props {
    #[serde(rename = "media.class", default, deserialize_with = "lenient")]
    class: Option<String>,
    #[serde(rename = "client.id", default, deserialize_with = "lenient")]
    client: Option<String>,
    #[serde(rename = "node.name", default, deserialize_with = "lenient")]
    node_name: Option<String>,
    #[serde(rename = "stream.monitor", default, deserialize_with = "lenient")]
    monitor: Option<String>,
    #[serde(rename = "application.name", default, deserialize_with = "lenient")]
    app_name: Option<String>,
    #[serde(
        rename = "application.process.binary",
        default,
        deserialize_with = "lenient"
    )]
    binary: Option<String>,
    #[serde(
        rename = "pipewire.access.portal.app_id",
        default,
        deserialize_with = "lenient"
    )]
    app_id: Option<String>,
}

/// An object's `info`: its state, its properties and, for a link, its two
/// ends. A field an update leaves out is `None`.
#[derive(Deserialize)]
struct Info {
    #[serde(default, deserialize_with = "lenient")]
    state: Option<String>,
    #[serde(default)]
    props: Option<Props>,
    #[serde(rename = "output-node-id", default)]
    output: Option<u32>,
    #[serde(rename = "input-node-id", default)]
    input: Option<u32>,
}

/// One object of a batch. `info` is `None` when the object was removed.
#[derive(Deserialize)]
pub struct Raw {
    id: u32,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    info: Option<Info>,
}

#[derive(Debug, Default)]
struct Node {
    class: String,
    running: bool,
    client: Option<u32>,
    name: String,
    monitor: bool,
    app_name: String,
    binary: String,
    app_id: String,
}

#[derive(Debug, Default)]
struct Link {
    from: u32,
    to: u32,
}

#[derive(Debug, Default)]
struct Client {
    app_name: String,
    binary: String,
    app_id: String,
}

/// The part of PipeWire's graph that says who uses what.
#[derive(Default)]
pub struct Graph {
    node: HashMap<u32, Node>,
    link: HashMap<u32, Link>,
    client: HashMap<u32, Client>,
}

/// Overwrites `field` with what an update says, where it says anything.
fn set(field: &mut String, value: Option<String>) {
    if let Some(value) = value {
        *field = value;
    }
}

impl Graph {
    /// Takes one batch in: each object's update, or its removal.
    pub fn apply(&mut self, batch: Vec<Raw>) {
        for raw in batch {
            let Some(info) = raw.info else {
                self.node.remove(&raw.id);
                self.link.remove(&raw.id);
                self.client.remove(&raw.id);
                continue;
            };
            match raw.kind.as_deref() {
                Some("PipeWire:Interface:Node") => {
                    let node = self.node.entry(raw.id).or_default();
                    if let Some(state) = info.state {
                        node.running = state == "running";
                    }
                    if let Some(props) = info.props {
                        set(&mut node.class, props.class);
                        if let Some(client) = props.client.and_then(|c| c.parse().ok()) {
                            node.client = Some(client);
                        }
                        set(&mut node.name, props.node_name);
                        if let Some(monitor) = props.monitor {
                            node.monitor = monitor == "true";
                        }
                        set(&mut node.app_name, props.app_name);
                        set(&mut node.binary, props.binary);
                        set(&mut node.app_id, props.app_id);
                    }
                }
                Some("PipeWire:Interface:Link") => {
                    let link = self.link.entry(raw.id).or_default();
                    if let Some(from) = info.output {
                        link.from = from;
                    }
                    if let Some(to) = info.input {
                        link.to = to;
                    }
                }
                Some("PipeWire:Interface:Client") => {
                    let client = self.client.entry(raw.id).or_default();
                    if let Some(props) = info.props {
                        set(&mut client.app_name, props.app_name);
                        set(&mut client.binary, props.binary);
                        set(&mut client.app_id, props.app_id);
                    }
                }
                _ => {}
            }
        }
    }

    /// The apps using the camera, the microphone or the screen now, sorted
    /// by name and then by client.
    pub fn uses(&self) -> Vec<InUse> {
        let mut ids: Vec<u32> = self.node.keys().copied().collect();
        ids.sort_unstable();
        // Each group is one client (or one stream without one): its kinds,
        // and the first of its streams, which names it.
        let mut groups: BTreeMap<u32, (BTreeSet<Kind>, u32)> = BTreeMap::new();
        for &id in &ids {
            let node = &self.node[&id];
            if !node.running || node.monitor {
                continue;
            }
            let video = node.class == "Stream/Input/Video";
            if !video && node.class != "Stream/Input/Audio" {
                continue;
            }
            for link in self.link.values().filter(|link| link.to == id) {
                let Some(source) = self.node.get(&link.from) else {
                    continue;
                };
                let kind = if video {
                    if source.name.starts_with("xdpw") {
                        Some(Kind::Screen)
                    } else if source.class.starts_with("Video/Source") {
                        Some(Kind::Camera)
                    } else {
                        None
                    }
                } else if source.class.starts_with("Audio/Source") {
                    Some(Kind::Microphone)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    let key = node.client.unwrap_or(id);
                    groups
                        .entry(key)
                        .or_insert_with(|| (BTreeSet::new(), id))
                        .0
                        .insert(kind);
                }
            }
        }
        let mut out: Vec<InUse> = groups
            .into_iter()
            .map(|(client, (kinds, first))| {
                let node = &self.node[&first];
                let found = node.client.and_then(|c| self.client.get(&c));
                let c_app_id = found.map_or("", |c| c.app_id.as_str());
                let c_binary = found.map_or("", |c| c.binary.as_str());
                let c_name = found.map_or("", |c| c.app_name.as_str());
                let portal = first_of(&[c_app_id, node.app_id.as_str()]);
                let binary = first_of(&[c_binary, node.binary.as_str()]).unwrap_or("");
                let raw_name = first_of(&[node.app_name.as_str(), c_name]).unwrap_or("");
                let (app, flatpak) = match portal {
                    Some(id) => (id, true),
                    None => (
                        first_of(&[binary, raw_name, node.name.as_str()]).unwrap_or(""),
                        false,
                    ),
                };
                let name = first_of(&[raw_name, binary, app]).unwrap_or("");
                InUse {
                    client,
                    app: app.to_string(),
                    name: name.to_string(),
                    flatpak,
                    kinds: kinds.into_iter().collect(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.client.cmp(&b.client)));
        out
    }
}

/// The first of `texts` that is not empty.
fn first_of<'a>(texts: &[&'a str]) -> Option<&'a str> {
    texts.iter().copied().find(|t| !t.is_empty())
}

/// What the sign's card and the log say of `uses`: each app and its kinds,
/// or that nothing is in use.
pub fn log_line(uses: &[InUse]) -> String {
    if uses.is_empty() {
        return "in use: nothing".to_string();
    }
    let apps: Vec<String> = uses
        .iter()
        .map(|used| {
            let kinds: Vec<&str> = used.kinds.iter().map(|k| k.name()).collect();
            format!("{} {}", used.app, kinds.join(","))
        })
        .collect();
    format!("in use: {}", apps.join("; "))
}

/// A word after the first in a list, lowered at its first letter, so
/// "Camera and microphone" reads as a sentence.
fn lower_first(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The kinds' titles as a sentence: "Camera", "Camera and microphone",
/// "Camera, microphone and screen". The "and" is translated.
pub fn kinds_text(kinds: &[Kind]) -> String {
    let titles: Vec<String> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            let title = tr(kind.title());
            if i == 0 {
                title.to_string()
            } else {
                lower_first(title)
            }
        })
        .collect();
    match titles.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => trf(
            "{first} and {last}",
            &[("first", &rest.join(", ")), ("last", last)],
        ),
    }
}

/// The kinds any of `uses` holds, in `Kind::ALL` order.
pub fn kinds_of(uses: &[InUse]) -> Vec<Kind> {
    Kind::ALL
        .into_iter()
        .filter(|kind| uses.iter().any(|used| used.kinds.contains(kind)))
        .collect()
}

/// A list of (app, kind) pairs: one use each.
type Pairs = Vec<(String, Kind)>;

/// Which (app, kind) pairs began and which ended, between the pairs
/// `since` holds (each with the time it began) and `now`, sorted.
pub fn changes(since: &HashMap<(String, Kind), u64>, now: &[InUse]) -> (Pairs, Pairs) {
    let current: BTreeSet<(String, Kind)> = now
        .iter()
        .flat_map(|used| used.kinds.iter().map(|kind| (used.app.clone(), *kind)))
        .collect();
    let started: Vec<(String, Kind)> = current
        .iter()
        .filter(|pair| !since.contains_key(*pair))
        .cloned()
        .collect();
    let mut ended: Vec<(String, Kind)> = since
        .keys()
        .filter(|pair| !current.contains(*pair))
        .cloned()
        .collect();
    ended.sort();
    (started, ended)
}

/// How long pw-dump may run before its failure starts the back-off again
/// from its first wait.
const LONG_RUN: Duration = Duration::from_secs(60);
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(30);

/// Starts the watcher: a thread that reads pw-dump and sends the uses to
/// `tx` whenever they change.
pub fn watch(tx: channel::Sender<Vec<InUse>>) {
    let spawned = std::thread::Builder::new()
        .name("in-use".into())
        .spawn(move || run(&tx));
    if let Err(e) = spawned {
        eprintln!("edel-shell-ui: could not watch what apps use: {e}");
    }
}

/// How one run of pw-dump ended.
enum Flow {
    /// pw-dump stopped or said something unreadable.
    Ended,
    /// The loop is gone: nothing more is sent.
    Gone,
}

fn run(tx: &channel::Sender<Vec<InUse>>) {
    let mut last: Vec<InUse> = Vec::new();
    let mut wait = FIRST_WAIT;
    loop {
        let began = Instant::now();
        let spawned = Command::new("pw-dump")
            .args(["--monitor", "--no-colors"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let flow = match spawned {
            Ok(mut child) => {
                let flow = match child.stdout.take() {
                    Some(out) => read(out, tx, &mut last),
                    None => Flow::Ended,
                };
                let _ = child.kill();
                let _ = child.wait();
                flow
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("edel-shell-ui: pw-dump is not here, so the panel shows no in-use sign");
                return;
            }
            Err(e) => {
                eprintln!("edel-shell-ui: pw-dump did not start: {e}");
                Flow::Ended
            }
        };
        if matches!(flow, Flow::Gone) {
            return;
        }
        if !last.is_empty() {
            last.clear();
            if tx.send(Vec::new()).is_err() {
                return;
            }
        }
        if began.elapsed() > LONG_RUN {
            wait = FIRST_WAIT;
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(LONGEST_WAIT);
    }
}

/// Reads pw-dump's batches from `out` until it ends, sending each change of
/// the uses to `tx` and keeping `last` as what was sent last.
fn read(out: ChildStdout, tx: &channel::Sender<Vec<InUse>>, last: &mut Vec<InUse>) -> Flow {
    let mut graph = Graph::default();
    let batches =
        serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Vec<Raw>>();
    for batch in batches {
        let Ok(batch) = batch else {
            return Flow::Ended;
        };
        graph.apply(batch);
        let now = graph.uses();
        if now != *last {
            *last = now.clone();
            if tx.send(now).is_err() {
                return Flow::Gone;
            }
        }
    }
    Flow::Ended
}

/// Seconds since 1970, UTC.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Shell {
    /// The uses changed: logs them, writes each use that ended to the
    /// person's record, and shows the new state on the status area and in
    /// the card. Logs `in use: ...` (`log_line`), `in use recorded APP KIND
    /// SECONDS` for each use written, or the error if it was not.
    pub(crate) fn in_use_changed(&mut self, uses: Vec<InUse>) {
        eprintln!("edel-shell-ui: {}", log_line(&uses));
        let now = now_secs();
        let (started, ended) = changes(&self.in_use_since, &uses);
        for pair in started {
            self.in_use_since.insert(pair, now);
        }
        for (app, kind) in ended {
            let Some(start) = self.in_use_since.remove(&(app.clone(), kind)) else {
                continue;
            };
            let used = Use {
                start,
                seconds: now.saturating_sub(start),
                kind,
                app,
            };
            record(&used);
        }
        self.live.in_use = kinds_of(&uses);
        self.in_use = uses;
        self.draw_all();
        self.in_use_card_changed();
    }
}

/// Adds `used` to the person's record of use (`places::APP_USE`).
fn record(used: &Use) {
    let Some(folder) = places::person_state_dir() else {
        eprintln!("edel-shell-ui: could not write the record of use: no state folder");
        return;
    };
    let path = folder.join(places::APP_USE);
    match app_use::append(&path, used) {
        Ok(()) => eprintln!(
            "edel-shell-ui: in use recorded {} {} {}",
            used.app,
            used.kind.name(),
            used.seconds
        ),
        Err(e) => eprintln!(
            "edel-shell-ui: could not write the record of use {}: {e}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The batches of a stream of pw-dump's JSON, applied in order.
    fn graph_of(stream: &str) -> Graph {
        let mut graph = Graph::default();
        for batch in serde_json::Deserializer::from_str(stream).into_iter::<Vec<Raw>>() {
            graph.apply(batch.expect("a batch of pw-dump's JSON"));
        }
        graph
    }

    /// A camera on a client with a portal id, used by a running stream.
    const CAMERA: &str = r#"[
        {"id": 50, "type": "PipeWire:Interface:Node", "info": {"state": "idle",
            "props": {"media.class": "Video/Source", "node.name": "v4l2_input.camera"}}},
        {"id": 60, "type": "PipeWire:Interface:Client", "info": {"props": {
            "application.name": "Snapshot", "application.process.binary": "snapshot",
            "pipewire.access.portal.app_id": "org.gnome.Snapshot", "pipewire.sec.pid": 1234}}},
        {"id": 61, "type": "PipeWire:Interface:Node", "info": {"state": "running",
            "props": {"media.class": "Stream/Input/Video", "client.id": 60,
                      "node.name": "gst-launch", "media.role": "Camera",
                      "stream.monitor": false}}},
        {"id": 70, "type": "PipeWire:Interface:Link", "info": {
            "output-node-id": 50, "input-node-id": 61, "state": "active"}}
    ]"#;

    #[test]
    fn a_camera_on_a_portal_app_is_named_by_its_app_id() {
        let uses = graph_of(CAMERA).uses();
        assert_eq!(
            uses,
            vec![InUse {
                client: 60,
                app: "org.gnome.Snapshot".into(),
                name: "Snapshot".into(),
                flatpak: true,
                kinds: vec![Kind::Camera],
            }]
        );
    }

    #[test]
    fn a_screen_share_is_the_screen_and_a_microphone_is_the_microphone() {
        let screen = CAMERA
            .replace("v4l2_input.camera", "xdpw-stream")
            .replace("Video/Source", "Video/Source/Virtual");
        assert_eq!(graph_of(&screen).uses()[0].kinds, vec![Kind::Screen]);
        let mic = r#"[
            {"id": 50, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                "props": {"media.class": "Audio/Source/Virtual", "node.name": "mic"}}},
            {"id": 61, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                "props": {"media.class": "Stream/Input/Audio", "client.id": 60,
                          "application.name": "Calls"}}},
            {"id": 70, "type": "PipeWire:Interface:Link", "info": {
                "output-node-id": 50, "input-node-id": 61}}
        ]"#;
        let uses = graph_of(mic).uses();
        assert_eq!(uses[0].kinds, vec![Kind::Microphone]);
        assert_eq!(uses[0].name, "Calls");
    }

    #[test]
    fn one_client_using_both_is_one_use_with_both_kinds() {
        let both = format!(
            "{}\n{}",
            CAMERA,
            r#"[
                {"id": 80, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                    "props": {"media.class": "Audio/Source", "node.name": "mic"}}},
                {"id": 81, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                    "props": {"media.class": "Stream/Input/Audio", "client.id": 60}}},
                {"id": 82, "type": "PipeWire:Interface:Link", "info": {
                    "output-node-id": 80, "input-node-id": 81}}
            ]"#
        );
        let uses = graph_of(&both).uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].kinds, vec![Kind::Camera, Kind::Microphone]);
    }

    #[test]
    fn idle_monitor_sink_and_unlinked_streams_are_not_counted() {
        let idle = CAMERA.replace(r#""state": "running""#, r#""state": "suspended""#);
        assert!(graph_of(&idle).uses().is_empty(), "suspended");
        let monitor = CAMERA.replace(r#""stream.monitor": false"#, r#""stream.monitor": true"#);
        assert!(graph_of(&monitor).uses().is_empty(), "a monitor stream");
        let sink = CAMERA.replace("Video/Source", "Audio/Sink");
        let sink = sink.replace("Stream/Input/Video", "Stream/Input/Audio");
        assert!(graph_of(&sink).uses().is_empty(), "a sink's monitor");
        let unlinked = CAMERA.replace(r#""output-node-id": 50"#, r#""output-node-id": 99"#);
        assert!(graph_of(&unlinked).uses().is_empty(), "no source");
    }

    #[test]
    fn a_removed_link_takes_the_use_away_and_a_state_alone_keeps_the_props() {
        let mut graph = graph_of(CAMERA);
        assert_eq!(graph.uses().len(), 1);
        let state =
            r#"[{"id": 61, "type": "PipeWire:Interface:Node", "info": {"state": "running"}}]"#;
        for batch in serde_json::Deserializer::from_str(state).into_iter::<Vec<Raw>>() {
            graph.apply(batch.expect("a batch"));
        }
        assert_eq!(graph.uses().len(), 1, "the props were kept");
        let removed = r#"[{"id": 70, "info": null}]"#;
        for batch in serde_json::Deserializer::from_str(removed).into_iter::<Vec<Raw>>() {
            graph.apply(batch.expect("a batch"));
        }
        assert!(graph.uses().is_empty());
    }

    #[test]
    fn a_host_app_is_named_by_its_binary_and_is_not_flatpak() {
        let host = CAMERA
            .replace(
                r#""pipewire.access.portal.app_id": "org.gnome.Snapshot", "#,
                "",
            )
            .replace(r#""application.name": "Snapshot", "#, "");
        let uses = graph_of(&host).uses();
        assert_eq!(uses[0].app, "snapshot");
        assert_eq!(uses[0].name, "snapshot");
        assert!(!uses[0].flatpak);
    }

    #[test]
    fn two_concatenated_arrays_are_two_batches() {
        let stream = format!("{}\n{}\n", "[]", r#"[{"id": 1, "info": null}]"#);
        let batches: Vec<Vec<Raw>> = serde_json::Deserializer::from_str(&stream)
            .into_iter::<Vec<Raw>>()
            .map(|b| b.expect("a batch"))
            .collect();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].len(), 1);
    }

    fn used(app: &str, kinds: &[Kind]) -> InUse {
        InUse {
            client: 1,
            app: app.into(),
            name: app.into(),
            flatpak: false,
            kinds: kinds.to_vec(),
        }
    }

    #[test]
    fn changes_say_what_began_and_what_ended() {
        let mut since = HashMap::new();
        since.insert(("a".to_string(), Kind::Camera), 10);
        since.insert(("b".to_string(), Kind::Microphone), 10);
        let now = [used("a", &[Kind::Camera, Kind::Screen])];
        let (started, ended) = changes(&since, &now);
        assert_eq!(started, vec![("a".to_string(), Kind::Screen)]);
        assert_eq!(ended, vec![("b".to_string(), Kind::Microphone)]);
    }

    #[test]
    fn the_log_line_has_both_shapes() {
        assert_eq!(log_line(&[]), "in use: nothing");
        let uses = [
            used("org.gnome.Snapshot", &[Kind::Camera]),
            used("calls", &[Kind::Microphone, Kind::Screen]),
        ];
        assert_eq!(
            log_line(&uses),
            "in use: org.gnome.Snapshot camera; calls microphone,screen"
        );
    }

    #[test]
    fn kinds_read_as_a_sentence() {
        assert_eq!(kinds_text(&[]), "");
        assert_eq!(kinds_text(&[Kind::Camera]), "Camera");
        assert_eq!(
            kinds_text(&[Kind::Camera, Kind::Microphone]),
            "Camera and microphone"
        );
        assert_eq!(
            kinds_text(&[Kind::Camera, Kind::Microphone, Kind::Screen]),
            "Camera, microphone and screen"
        );
        assert_eq!(
            kinds_of(&[used("a", &[Kind::Screen]), used("b", &[Kind::Camera])]),
            vec![Kind::Camera, Kind::Screen]
        );
    }
}
