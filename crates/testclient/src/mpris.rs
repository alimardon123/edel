//! `--mpris` (roadmap M5.9d): quick settings' test player. It claims
//! `org.mpris.MediaPlayer2.edeltest` on the session's bus and serves MPRIS's
//! two interfaces at `/org/mpris/MediaPlayer2`: it plays "Night Drive" by
//! "Lumen" (identity "Edel test player"). Each command quick settings sends
//! prints its name into the output (`play pause` for `PlayPause`, which also
//! switches between playing and paused and tells the bus so), and then it
//! waits until it is killed.

use std::collections::HashMap;
use std::io::Write;

use anyhow::{Context, Result};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const NAME: &str = "org.mpris.MediaPlayer2.edeltest";

/// Prints `line` and flushes, as the output is a file that is read back.
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

/// The root interface: what the app is called and what it can do.
struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Edel test player".into()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn raise(&self) {}

    fn quit(&self) {}
}

/// The player interface: whether it plays, and the track.
struct Player {
    playing: bool,
}

/// A value for the metadata: a string, a list of strings or an object path.
fn owned(value: Value<'static>) -> OwnedValue {
    OwnedValue::try_from(value).expect("a plain value has no file descriptor")
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing { "Playing" } else { "Paused" }.into()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let track = ObjectPath::try_from("/org/edel/track/1")
            .expect("a constant path is valid")
            .into_owned();
        HashMap::from([
            ("mpris:trackid".to_string(), owned(Value::from(track))),
            (
                "xesam:title".to_string(),
                owned(Value::from("Night Drive".to_string())),
            ),
            (
                "xesam:artist".to_string(),
                owned(Value::from(vec!["Lumen".to_string()])),
            ),
        ])
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        0
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }

    async fn play_pause(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.playing = !self.playing;
        say("play pause");
        self.playback_status_changed(&emitter).await?;
        Ok(())
    }

    async fn play(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.playing = true;
        say("play");
        self.playback_status_changed(&emitter).await?;
        Ok(())
    }

    async fn pause(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.playing = false;
        say("pause");
        self.playback_status_changed(&emitter).await?;
        Ok(())
    }

    async fn stop(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.playing = false;
        say("stop");
        self.playback_status_changed(&emitter).await?;
        Ok(())
    }

    fn next(&self) {
        say("next");
    }

    fn previous(&self) {
        say("previous");
    }
}

/// Serves the two interfaces until the process is killed.
pub fn run() -> Result<()> {
    let _connection = zbus::blocking::connection::Builder::session()
        .context("connecting to the session's bus")?
        .name(NAME)
        .context("taking the player's bus name")?
        .serve_at(PATH, Root)
        .context("serving the root interface")?
        .serve_at(PATH, Player { playing: true })
        .context("serving the player interface")?
        .build()
        .context("starting the test player")?;
    loop {
        std::thread::park();
    }
}
