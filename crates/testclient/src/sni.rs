//! `--sni` (roadmap M5.2e): the tray's test item. It registers one
//! StatusNotifierItem with `org.kde.StatusNotifierWatcher` on the session's
//! bus (shell-ui serves the watcher), titled "edel test", whose icon is a
//! 22 by 22 pixmap of one colour, `#33aa66`, and then waits until it is
//! killed. Like a real app it passes only its object path, so the watcher
//! must take the bus name from the sender, and it tries again until the
//! watcher is there, since shell-ui starts a moment after the session.
//! A click on its icon prints `activate X Y`, a right click
//! `context menu X Y`.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

const SIDE: i32 = 22;
const PATH: &str = "/StatusNotifierItem";

struct Item;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }

    /// Solid `#33aa66`, as ARGB32 in network byte order.
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(
            SIDE,
            SIDE,
            [0xff, 0x33, 0xaa, 0x66].repeat((SIDE * SIDE) as usize),
        )]
    }

    #[zbus(property)]
    fn title(&self) -> String {
        "edel test".into()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        "edel-testclient".into()
    }

    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }

    fn activate(&self, x: i32, y: i32) {
        println!("activate {x} {y}");
    }

    fn context_menu(&self, x: i32, y: i32) {
        println!("context menu {x} {y}");
    }
}

pub fn run() -> Result<()> {
    let connection = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(PATH, Item))
        .and_then(|b| b.build())
        .context("connecting to the session's bus")?;
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let registered = connection.call_method(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            Some("org.kde.StatusNotifierWatcher"),
            "RegisterStatusNotifierItem",
            &(PATH,),
        );
        match registered {
            Ok(_) => break,
            Err(e) if Instant::now() > until => bail!("no tray to register with after 60 s: {e}"),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
    println!("registered");
    loop {
        std::thread::park();
    }
}
