//! `edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second
//! long-running process beside the compositor, which starts it and starts
//! it again if it ends. It draws its own surfaces (ADR-002's shell-ui
//! toolkit decision of 2026-10-03): layer-shell surfaces through
//! smithay-client-toolkit, drawn with tiny-skia into shared memory, text
//! shaped by cosmic-text, all from the design tokens. Today it draws the
//! preset's panels (M5.1b, M5.1c): the settings file's `layout.preset`, else
//! Classic, whose one panel runs along the bottom of the first screen
//! with the menu button, the window list (M5.2h), the workspace switcher
//! (M5.2c), the layout toggle (M5.3a) and a clock. Each
//! panel holds widgets from the table in `widgets/` and is drawn again
//! only when what a widget shows, or the panel's size or scale, changes;
//! a click or a scroll on a widget does what the widget says. Super,
//! tapped alone, or the menu button opens the launcher (M5.3b,
//! `launcher.rs`), and the status area's click opens quick settings
//! (M5.9a, `quick.rs`). It exits when the compositor goes away.

mod a11y;
mod banner;
mod calendar;
mod centre;
mod launcher;
mod link;
mod messages;
mod mpris;
mod notice;
mod notify;
mod notify_card;
mod osd;
mod osd_card;
mod paint;
mod popup;
mod portal;
mod quick;
mod quick_card;
mod status;
mod styles;
mod switcher;
mod telling;
mod tooltip;
mod toplevels;
mod tray;
mod tray_card;
mod trayview;
mod watch;
mod widgets;
mod workspaces;

use anyhow::{Context, Result};
use smithay_client_toolkit::compositor::{
    CompositorHandler, CompositorState, FrameCallbackData, Region,
};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use std::path::Path;
use std::time::Duration;

use tiny_skia::Pixmap;

use edel::places;
use edel::presets::{self, Edge, Hide, Style};
use edel::settings;
use edel::tokens::{self, Scheme, Tokens};
use edel::{app_icons as icons, apps};

use crate::notify_card::{BannerCard, CentreCard};
use crate::osd_card::OsdCard;
use crate::paint::{Look, Row, Text};
use crate::popup::Popup;
use crate::quick_card::QuickCard;
use crate::tray_card::TrayCard;
use crate::widgets::{Action, Input, Live};

/// The panels' layer surfaces' namespace, as the compositor's state file
/// lists it, and the launcher's.
const NAMESPACE: &str = "edel-panel";
/// A dock's (M5.4d), which sits `DOCK_MARGIN` from its edge.
const DOCK: &str = "edel-dock";
const DOCK_MARGIN: i32 = 8;
const LAUNCHER: &str = "edel-launcher";
const SWITCHER: &str = "edel-switcher";
/// The layout button's menu of tiling styles (M5.16b).
const STYLES: &str = "edel-styles";
/// Quick settings, opened from the status area (M5.9a).
const QUICK: &str = "edel-quick";
/// A new notification's banner and the notification centre, which the
/// clock opens (M5.9b).
const BANNER: &str = "edel-notification";
const CENTRE: &str = "edel-centre";
/// The volume and brightness pop-up a media key shows (M5.9c).
const OSD: &str = "edel-osd";
/// The tray's grid of the apps behind its arrow (M5.9g).
const TRAY_GRID: &str = "edel-tray";
/// The tray's tooltip, a label over the arrow (M5.9h).
const TOOLTIP: &str = "edel-tooltip";
/// What opens when the Settings button is pressed, and the way to a page.
const SETTINGS: &str = "edel-settings";
/// The feature that brings it.
const SETTINGS_FEATURE: &str = "settings";
/// The launcher's distance from the panel and the screen's side.
const MARGIN: i32 = 8;

/// Where the compositor says how much the desktop animates (M5.11a).
const STATE: &str = places::STATE_FILE;

struct Shell {
    registry: RegistryState,
    outputs: OutputState,
    seat: SeatState,
    pointer: Option<wl_pointer::WlPointer>,
    /// What widgets show beyond the clock: the workspaces, the windows and
    /// the shown workspace's policy.
    live: Live,
    workspaces: workspaces::Workspaces,
    toplevels: toplevels::Toplevels,
    link: link::Link,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    pool: SlotPool,
    /// Panel buffers drawn before the one shown, kept until the compositor
    /// lets go of them so their pages can be given back (free_spent).
    spent: Vec<Buffer>,
    panels: Vec<Panel>,
    /// The panels the settings files ask for as last applied (M5.31b):
    /// `panels_changed` compares with them.
    panel_specs: Vec<presets::Panel>,
    /// The preset's pinned apps (M5.4c), kept so panels made later can
    /// read the apps they show (`read_apps`).
    pins: Vec<String>,
    launcher: launcher::Launcher,
    /// The launcher's surface while it is open.
    menu: Option<Menu>,
    /// The tiling styles' menu while it is open (M5.16b).
    styles: Option<StylesMenu>,
    /// Quick settings while open (M5.9a), the preset's tiles for it, and
    /// whether the machine has the Settings app.
    quick: Option<QuickCard>,
    quick_tiles: Vec<String>,
    quick_settings: bool,
    /// The notifications (M5.9b), at most fifty and text only, the banner
    /// of the newest while it shows, and the notification centre while
    /// open.
    notifications: notify::List,
    banner: Option<BannerCard>,
    centre: Option<CentreCard>,
    /// The volume and brightness pop-up while it shows (M5.9c).
    osd: Option<OsdCard>,
    /// The tray's grid of the apps behind its arrow while open (M5.9g),
    /// the number of apps behind the arrow as last logged, and a press on
    /// a kept icon that may become a drag: its panel, its item's id and
    /// where it went down.
    tray_grid: Option<TrayCard>,
    tray_behind: usize,
    tray_press: Option<(usize, String, f32, f32)>,
    /// The tray's tooltip while it shows (M5.9h), and the timer that shows
    /// it once the pointer has rested on the arrow.
    tooltip: Option<tooltip::Tip>,
    tooltip_timer: Option<RegistrationToken>,
    /// Whether Shift is held, for Shift+F10 on the tray's grid.
    shift: bool,
    /// The status area's reading (M5.9a): whether a panel holds it, how
    /// many threads read or run something, whether another reading is
    /// wanted when they end (and whether with Bluetooth), the way back to
    /// the loop and the system bus's connection.
    status_wanted: bool,
    status_busy: u32,
    status_again: Option<bool>,
    status_tx: channel::Sender<status::Msg>,
    /// Scrolling over a panel not yet a whole step.
    scrolled: widgets::Scrolled,
    /// The window switcher's surface while Alt+Tab is held (M5.3c), and
    /// what it shows.
    flip: Option<Popup<switcher::View>>,
    flipped: switcher::View,
    qh: QueueHandle<Shell>,
    handle: LoopHandle<'static, Shell>,
    tokens: Tokens,
    /// The settings portal's backend on the session's bus (M5.5a), and
    /// the tray's watcher (M5.2e), served while this lives.
    _portal: Option<zbus::blocking::Connection>,
    /// Where the players' news goes while quick settings is open (M5.9d).
    player_tx: Option<channel::Sender<mpris::Event>>,
    text: Text,
    icons: icons::Icons,
    fillets: bool,
    exit: bool,
}

/// The open launcher's surface and the keyboard, let go when it closes.
struct Menu {
    popup: Popup<launcher::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
}

/// The open tiling styles' menu, what it shows and the keyboard.
struct StylesMenu {
    popup: Popup<styles::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    view: styles::View,
}

/// One of the preset's panels.
struct Panel {
    edge: Edge,
    style: Style,
    surface: LayerSurface,
    row: Row,
    /// Its logical width, once the compositor has said it.
    width: u32,
    /// The width a dock last asked for, as wide as what it holds.
    asked: u32,
    scale: u32,
    /// What was drawn last, so nothing is drawn twice.
    drawn: Option<Look>,
    /// Drawn, and the compositor has not yet shown it: the next drawing
    /// waits for its frame, so at most one waits and its buffers stay
    /// two however fast things change.
    waiting: bool,
    /// Where each widget lies, start to end: its left edge and width in
    /// logical pixels, for clicks.
    places: Vec<(f32, f32)>,
    /// The buffer attached last, kept so the one before it can be freed
    /// once the compositor releases it.
    shown: Option<Buffer>,
    /// What screen readers read of it (M5.1d).
    reader: a11y::Reader,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("edel-shell-ui: {}", messages::stopped(format!("{e:#}")));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Huge pages stay off for this small process, before any thread starts:
    // with them on, a thread stack that happens to cover a 2 MiB boundary is
    // backed by one 2 MiB page at its first touch, which CI read as 2 MiB
    // more of shell-ui's own memory on some runs.
    if let Err(e) = rustix::thread::disable_transparent_huge_pages(true) {
        eprintln!("edel-shell-ui: {}", messages::huge_pages_on(e));
    }
    if let Some((language, words)) = edel::i18n::init("shell-ui") {
        eprintln!("edel-shell-ui: words in {language}, {words} translated");
    }
    let (preset, scheme) = from_system_files();
    let tokens = load_tokens(scheme);
    let connection = Connection::connect_to_env().context(messages::NO_COMPOSITOR)?;
    let (globals, queue) = registry_queue_init(&connection).context(messages::NO_GLOBALS)?;
    let qh = queue.handle();
    let compositor =
        CompositorState::bind(&globals, &qh).with_context(|| messages::missing("wl_compositor"))?;
    let shm = Shm::bind(&globals, &qh).with_context(|| messages::missing("wl_shm"))?;
    let layers = LayerShell::bind(&globals, &qh)
        .with_context(|| messages::missing("zwlr_layer_shell_v1"))?;
    let strip = paint::fillet_height(&tokens);
    let features = &edel::places::found_shared(edel::features::DIR);
    let mut panels = Vec::new();
    for spec in &preset.panels {
        panels.push(make_panel(
            &compositor,
            &layers,
            &qh,
            &tokens,
            features,
            spec,
        ));
    }
    // The apps a panel's apps widget shows, and whose icons the window
    // list shows, read once (M5.4c); none read when no panel holds either.
    let mut live = Live::default();
    if panels
        .iter()
        .any(|p| p.row.all().any(|w| w.name == "apps" || w.name == "windows"))
    {
        let installed = apps::read_all(&apps::dirs());
        live.pinned = preset
            .apps
            .pinned
            .iter()
            .filter_map(|pin| apps::pinned(&installed, pin))
            .map(widgets::Pin::from)
            .collect();
        live.installed = installed.iter().map(widgets::Pin::from).collect();
    }
    let pool = SlotPool::new(1280 * (tokens.panel_height + strip) as usize * 4, &shm)
        .context("creating the shared memory pool")?;
    let mut event_loop: EventLoop<Shell> =
        EventLoop::try_new().context("starting the event loop")?;
    let (status_tx, status_rx) = channel::channel::<status::Msg>();
    let status_wanted = panels
        .iter()
        .any(|p| p.row.all().any(|w| w.name == "status"));
    let mut shell = Shell {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        pointer: None,
        live,
        workspaces: workspaces::Workspaces::bind(&globals, &qh),
        toplevels: toplevels::Toplevels::bind(&globals, &qh),
        link: link::Link::bind(&globals, &qh),
        compositor,
        layers,
        shm,
        pool,
        spent: Vec::new(),
        panels,
        panel_specs: preset.panels.clone(),
        pins: preset.apps.pinned.clone(),
        launcher: launcher::Launcher::default(),
        menu: None,
        styles: None,
        quick: None,
        quick_tiles: preset.quick.tiles.clone(),
        quick_settings: features
            .join(format!("{}.toml", SETTINGS_FEATURE))
            .is_file(),
        notifications: notify::List::default(),
        banner: None,
        centre: None,
        osd: None,
        tray_grid: None,
        tray_behind: 0,
        tray_press: None,
        tooltip: None,
        tooltip_timer: None,
        shift: false,
        status_wanted,
        status_busy: 0,
        status_again: None,
        status_tx,
        flip: None,
        flipped: switcher::View::default(),
        scrolled: widgets::Scrolled::default(),
        qh: qh.clone(),
        handle: event_loop.handle(),
        text: Text::load(&tokens.font),
        icons: icons::Icons::new(apps::data_dirs()),
        _portal: portal::serve(&tokens),
        player_tx: None,
        tokens,
        fillets: fillets(),
        exit: false,
    };
    // The portal and the tray share the session's bus and zbus's thread
    // for it; the tray's news reaches the loop over a channel (M5.2e), but
    // only when a panel holds the widget. Served once the panel's fonts
    // and icons are loaded, as the portal always was: served before them,
    // shell-ui kept 3.7 MiB of its own instead of 1.6 (#138's first run).
    let tray_wanted = shell
        .panels
        .iter()
        .any(|p| p.row.all().any(|w| w.name == "tray"));
    if let (Some(connection), true) = (&shell._portal, tray_wanted) {
        let (events, news) = channel::channel();
        if tray::serve(connection, events) {
            event_loop
                .handle()
                .insert_source(news, |event, _, shell: &mut Shell| {
                    if let channel::Event::Msg(event) = event {
                        shell.tray_changed(event);
                    }
                })
                .map_err(|e| anyhow::anyhow!("watching the tray: {e}"))?;
        }
    }
    // Notifications (M5.9b): served on the same connection, the calls
    // reaching the loop over a channel as the tray's news does; nothing
    // is kept until an app calls.
    if let Some(connection) = &shell._portal {
        let (calls, notices) = channel::channel();
        if notify::serve(connection, calls) {
            event_loop
                .handle()
                .insert_source(notices, |event, _, shell: &mut Shell| {
                    if let channel::Event::Msg(msg) = event {
                        shell.notify_msg(msg);
                    }
                })
                .map_err(|e| anyhow::anyhow!("watching notifications: {e}"))?;
        }
    }
    // shell-ui's own notices (M5.9e): a fallback record shown once for each
    // person, and the update check on a timer, from a person's session only.
    shell.tell_fallback();
    check_updates(&event_loop.handle());
    // What plays (M5.9d): read over MPRIS only while quick settings is open,
    // its news reaching the loop over a channel as the tray's does.
    if shell._portal.is_some() {
        let (events, news) = channel::channel();
        event_loop
            .handle()
            .insert_source(news, |event, _, shell: &mut Shell| {
                if let channel::Event::Msg(event) = event {
                    shell.player_changed(event);
                }
            })
            .map_err(|e| anyhow::anyhow!("watching what plays: {e}"))?;
        shell.player_tx = Some(events);
    }
    // The status area (M5.9a): what the machine says is read once now, and
    // again when the system bus says NetworkManager or UPower changed
    // something; only when a panel holds the widget.
    if status_wanted {
        event_loop
            .handle()
            .insert_source(status_rx, |event, _, shell: &mut Shell| {
                if let channel::Event::Msg(msg) = event {
                    shell.status_msg(msg);
                }
            })
            .map_err(|e| anyhow::anyhow!("watching the status: {e}"))?;
        watch::serve(features, &shell.status_tx, &event_loop.handle());
        shell.request_status(false);
    }
    WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("watching the compositor: {e}"))?;
    tick(&event_loop.handle());
    while !shell.exit {
        // When the compositor goes away, so does the panel: the end.
        if event_loop.dispatch(None, &mut shell).is_err() {
            eprintln!("edel-shell-ui: {}", messages::COMPOSITOR_GONE);
            break;
        }
        shell.free_spent();
    }
    Ok(())
}

/// The widgets a panel holds, from its lines, as this machine can show
/// them; anything skipped is noted on the standard error.
fn row_of(spec: &presets::Panel, features: &Path) -> Row {
    let pick = |names: &[String]| {
        let (found, notes) = widgets::usable(names, features);
        for note in notes {
            eprintln!("edel-shell-ui: {note}");
        }
        found
    };
    Row {
        start: pick(&spec.start),
        centre: pick(&spec.centre),
        end: pick(&spec.end),
    }
}

/// Makes one of the preset's panels as a layer surface of its own, with
/// the widgets it holds (M5.1b, M5.4d, M5.4f, M5.31b: `panels_changed`
/// makes them again too).
fn make_panel(
    compositor: &CompositorState,
    layers: &LayerShell,
    qh: &QueueHandle<Shell>,
    tokens: &Tokens,
    features: &Path,
    spec: &presets::Panel,
) -> Panel {
    let strip = paint::fillet_height(tokens);
    let row = row_of(spec, features);
    // Along the edge of the first screen; the strip on its inner side
    // for the fillets is drawn but takes no space and no clicks. A
    // dock is centred along its edge, a little away from it, and
    // keeps that much free of windows too; its width follows what it
    // holds once drawn, square until then.
    let dock = spec.style == Style::Dock;
    let namespace = if dock { DOCK } else { NAMESPACE };
    let surface = compositor.create_surface(qh);
    let surface = layers.create_layer_surface(qh, surface, Layer::Top, Some(namespace), None);
    let edge = match spec.edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    };
    if dock {
        surface.set_anchor(edge);
        surface.set_size(paint::DOCK_HEIGHT, paint::DOCK_HEIGHT);
        let (top, bottom) = match spec.edge {
            Edge::Top => (DOCK_MARGIN, 0),
            Edge::Bottom => (0, DOCK_MARGIN),
        };
        surface.set_margin(top, 0, bottom, 0);
    } else {
        surface.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
        surface.set_size(0, tokens.panel_height + strip);
    }
    // A dock that hides while a window covers it (M5.4f) keeps
    // nothing free; the compositor hides it.
    let zone = if dock && spec.hide == Hide::Covered {
        0
    } else {
        paint::height(spec.style, tokens) as i32
    };
    surface.set_exclusive_zone(zone);
    surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    surface.commit();
    eprintln!(
        "edel-shell-ui: panel {namespace} along the {}",
        spec.edge.name()
    );
    Panel {
        edge: spec.edge,
        style: spec.style,
        surface,
        row,
        width: 0,
        asked: 0,
        scale: 1,
        drawn: None,
        waiting: false,
        places: Vec::new(),
        shown: None,
        reader: a11y::Reader::panel(),
    }
}

/// The image's tokens in `scheme` if it has them, else the built-in ones;
/// anything skipped is reported, never fatal (ADR-008).
fn load_tokens(scheme: Scheme) -> Tokens {
    let (tokens, notes) = tokens::load(scheme);
    for note in notes {
        eprintln!("edel-shell-ui: {}: {note}", tokens::PATH);
    }
    tokens
}

/// The preset the settings files name, else Classic, and the colour scheme
/// they pick (M5.5c), the person's over the machine's; a broken file or an
/// unknown name is reported, never fatal (ADR-008).
fn from_system_files() -> (presets::Preset, Scheme) {
    let (mut name, mut panels, mut scheme) = (None, None, None);
    let files = [Some(places::machine_settings()), places::person_settings()];
    for path in files.into_iter().flatten().map(|p| places::found(&p)) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match settings::read(&text) {
            Ok(read) => {
                let layout = read.file.layout;
                name = layout.preset.or(name);
                panels = layout.panels.or(panels);
                scheme = read.file.appearance.mode.or(scheme);
            }
            Err(e) => eprintln!(
                "edel-shell-ui: {}: {e:#}; its keys are left out",
                path.display()
            ),
        }
    }
    let (mut preset, note) = presets::named(name.as_deref());
    if let Some(note) = note {
        eprintln!("edel-shell-ui: {note}");
    }
    // layout.panels, when set, in place of the preset's (M5.4e).
    if let Some(panels) = panels {
        preset.panels = panels;
    }
    let scheme = scheme
        .as_deref()
        .and_then(Scheme::parse)
        .unwrap_or_default();
    (preset, scheme)
}

/// Whether the fillets are drawn: not on the Lite tier, which keeps
/// surfaces flat (ADR-002), nor before the compositor has said.
fn fillets() -> bool {
    let tier = std::fs::read_to_string(STATE)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|t| t.get("tier")?.as_str().map(String::from));
    tier.is_some_and(|t| t != "lite")
}

/// The update check (M5.9e): `edel update --check` runs ten minutes after
/// start (`EDEL_UPDATE_CHECK_SECS` seconds, for tests), then every six
/// hours, each run in a short thread whose answer comes back over a
/// channel, so the loop never waits for the network. Only where the
/// `edel` command is installed and a person has a state folder to keep
/// their stamps in.
fn check_updates(handle: &LoopHandle<'static, Shell>) {
    if places::person_state_dir().is_none() || !edel_installed() {
        return;
    }
    let first = std::env::var("EDEL_UPDATE_CHECK_SECS")
        .ok()
        .and_then(|secs| secs.parse().ok())
        .unwrap_or(telling::FIRST_CHECK_SECS);
    let (answers, news) = channel::channel::<Result<String, String>>();
    let watching = handle.insert_source(news, |event, _, shell: &mut Shell| {
        if let channel::Event::Msg(answer) = event {
            shell.update_checked(answer);
        }
    });
    if let Err(e) = watching {
        eprintln!("edel-shell-ui: could not watch the update check: {e}");
        return;
    }
    let timer = Timer::from_duration(Duration::from_secs(first));
    let checked = handle.insert_source(timer, move |_, _, _shell: &mut Shell| {
        let answers = answers.clone();
        std::thread::spawn(move || {
            let _ = answers.send(run_update_check());
        });
        TimeoutAction::ToDuration(Duration::from_secs(telling::CHECK_EVERY_SECS))
    });
    if let Err(e) = checked {
        eprintln!("edel-shell-ui: could not time the update check: {e}");
    }
}

/// `edel update --check`'s standard output, or why it did not run.
fn run_update_check() -> Result<String, String> {
    let out = std::process::Command::new("edel")
        .args(["update", "--check"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("edel update --check ended with {}", out.status))
    }
}

/// Whether an `edel` program is in one of the `PATH` folders.
fn edel_installed() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("edel").is_file()))
}

/// Draws the panels again when the minute changes, for the clock, and
/// collects the apps the launcher started that ended, then sleeps until
/// the next one.
fn tick(handle: &LoopHandle<'static, Shell>) {
    let next = || widgets::clock::until_next_minute(&jiff::Zoned::now());
    let result = handle.insert_source(
        Timer::from_duration(next()),
        move |_, _, shell: &mut Shell| {
            for i in 0..shell.panels.len() {
                shell.draw(i);
            }
            shell.launcher.reap();
            // The status area follows the machine on the minute too, for
            // a change no daemon signalled.
            shell.request_status(false);
            TimeoutAction::ToDuration(next())
        },
    );
    if let Err(e) = result {
        eprintln!("edel-shell-ui: {}", messages::clock_not_started(e));
    }
}

impl Shell {
    /// Draws panel `i` if anything it shows changed, once the compositor
    /// has shown its last drawing.
    fn draw(&mut self, i: usize) {
        let panel = &self.panels[i];
        if panel.width == 0 || panel.waiting {
            return;
        }
        let strip = paint::strip(panel.style, &self.tokens);
        let shown = panel.row.shows(&self.live);
        if panel.style == Style::Dock {
            // A dock is as wide as what it holds: when that changes it
            // asks for the new width and draws once the compositor
            // agrees.
            let natural = paint::natural_width(
                &self.tokens,
                Some(&mut self.text),
                Some(&mut self.icons),
                &panel.row,
                &shown,
                panel.scale,
            );
            if natural != panel.width {
                let panel = &mut self.panels[i];
                if natural != panel.asked {
                    panel.asked = natural;
                    panel.surface.set_size(natural, paint::DOCK_HEIGHT);
                    panel.surface.commit();
                }
                return;
            }
        }
        let panel = &self.panels[i];
        let look = Look {
            width: panel.width * panel.scale,
            height: (paint::height(panel.style, &self.tokens) + strip) * panel.scale,
            scale: panel.scale,
            edge: panel.edge,
            style: panel.style,
            fillets: self.fillets,
            shown,
        };
        if panel.drawn.as_ref() == Some(&look) {
            return;
        }
        if let Err(e) = self.show(i, &look) {
            eprintln!(
                "edel-shell-ui: {}",
                messages::panel_not_drawn(format!("{e:#}"))
            );
            return;
        }
        // Screen readers get what was drawn, in logical pixels.
        let panel = &mut self.panels[i];
        let top = f64::from(paint::panel_top(panel.edge, panel.style, &self.tokens));
        let bottom = top + f64::from(paint::height(panel.style, &self.tokens));
        let items = panel
            .row
            .all()
            .zip(&look.shown)
            .zip(&panel.places)
            .filter(|(_, (_, width))| *width > 0.0)
            .filter(|((widget, shown), _)| !(widget.label)(shown).is_empty())
            .map(|((widget, shown), &(x, width))| a11y::Item {
                role: widget.role,
                label: (widget.label)(shown),
                bounds: accesskit::Rect::new(x.into(), top, (x + width).into(), bottom),
                children: (widget.parts)(shown)
                    .into_iter()
                    .map(|part| a11y::Item {
                        role: accesskit::Role::Button,
                        label: part.label,
                        bounds: accesskit::Rect::new(
                            f64::from(x + part.x),
                            top,
                            f64::from(x + part.x + part.width),
                            bottom,
                        ),
                        children: Vec::new(),
                        toggled: None,
                        value: None,
                    })
                    .collect(),
                toggled: None,
                value: None,
            })
            .collect();
        let size = (
            f64::from(panel.width),
            f64::from(paint::height(panel.style, &self.tokens) + strip),
        );
        panel.reader.update(size, items);
        panel.drawn = Some(look);
    }

    fn show(&mut self, i: usize, look: &Look) -> Result<()> {
        let panel = &self.panels[i];
        let mut pixmap = Pixmap::new(look.width, look.height).context("a panel of no size")?;
        let places = paint::paint(
            &mut pixmap,
            look,
            &self.tokens,
            Some(&mut self.text),
            Some(&mut self.icons),
            &panel.row,
        );
        if places != panel.places {
            // Where each widget lies, for the tests that click them.
            let list: Vec<String> = (0..places.len())
                .filter_map(|j| Some((panel.row.widget(j)?.name, places[j])))
                .map(|(name, (x, w))| format!("{name} {x:.0}+{w:.0}"))
                .collect();
            eprintln!("edel-shell-ui: panel places {}", list.join(", "));
        }
        let (w, h) = (look.width as i32, look.height as i32);
        let (buffer, canvas) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
            .context("creating a buffer")?;
        paint::to_argb(&pixmap, canvas);
        let surface = panel.surface.wl_surface();
        // Only the panel itself is opaque and takes clicks; the fillets'
        // strip beside it lets both through. A dock's rounded corners
        // are not opaque.
        let top = paint::panel_top(panel.edge, panel.style, &self.tokens) as i32;
        let panel_h = paint::height(panel.style, &self.tokens) as i32;
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, top, panel.width as i32, panel_h);
            if panel.style == Style::Bar {
                surface.set_opaque_region(Some(region.wl_region()));
            }
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.set_buffer_scale(panel.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(surface).context("attaching the buffer")?;
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        panel.surface.commit();
        self.panels[i].places = places;
        self.panels[i].waiting = true;
        if let Some(old) = self.panels[i].shown.replace(buffer) {
            self.spent.push(old);
        }
        Ok(())
    }

    /// Gives back the pages of panel buffers the compositor has let go
    /// of: a freed slot stays mapped in the pool and resident until its
    /// pages are removed, which would keep a second panel's worth of
    /// shared memory for good.
    fn free_spent(&mut self) {
        let pool = &mut self.pool;
        self.spent.retain(|buffer| {
            let Some(canvas) = buffer.canvas(pool) else {
                return true; // still held by the compositor
            };
            release_pages(canvas);
            false
        });
    }

    /// The compositor said the workspaces anew: the switcher's view
    /// follows the shown workspace once that changes.
    fn workspaces_changed(&mut self) {
        let now = self.workspaces.names();
        let shown = |list: &[(String, bool)]| list.iter().position(|(_, on)| *on);
        if shown(&now) != shown(&self.live.workspaces) {
            self.live.view = None;
        }
        self.live.workspaces = now;
        self.draw_all();
    }

    /// The compositor said what changed about the windows.
    fn windows_changed(&mut self) {
        let now = self.toplevels.tasks();
        if now != self.live.windows {
            self.live.windows = now;
            self.draw_all();
        }
    }

    fn draw_all(&mut self) {
        for i in 0..self.panels.len() {
            self.draw(i);
        }
    }

    /// The panels `layout.panels` asks for now (M5.31b). When they keep
    /// each panel's edge, style and hiding, each panel takes its new
    /// widgets and is drawn again, its surface kept; otherwise the open
    /// popups close, the panels' surfaces go and every wanted panel is
    /// made anew. Logs `panels now EDGE (N widgets), ...` either way.
    pub fn panels_changed(&mut self, wanted: Vec<presets::Panel>) {
        let features = edel::places::found_shared(edel::features::DIR);
        let same_shape = wanted.len() == self.panel_specs.len()
            && self.panel_specs.iter().zip(&wanted).all(|(old, new)| {
                old.edge == new.edge && old.style == new.style && old.hide == new.hide
            });
        if same_shape {
            for (panel, spec) in self.panels.iter_mut().zip(&wanted) {
                panel.row = row_of(spec, &features);
                panel.drawn = None;
            }
            self.draw_all();
        } else {
            self.close_popups();
            self.tray_press = None;
            for panel in std::mem::take(&mut self.panels) {
                // The layer surface's role goes first, then its surface.
                let surface = panel.surface.wl_surface().clone();
                drop(panel);
                surface.destroy();
            }
            self.panels = wanted
                .iter()
                .map(|spec| {
                    make_panel(
                        &self.compositor,
                        &self.layers,
                        &self.qh,
                        &self.tokens,
                        &features,
                        spec,
                    )
                })
                .collect();
        }
        let list: Vec<String> = self
            .panels
            .iter()
            .map(|p| format!("{} ({} widgets)", p.edge.name(), p.row.all().count()))
            .collect();
        eprintln!("edel-shell-ui: panels now {}", list.join(", "));
        self.panel_specs = wanted;
        self.read_apps();
    }

    /// The apps the apps widget and the window list show, read once, when
    /// a panel first holds either (at start, or when the panels change).
    fn read_apps(&mut self) {
        let wanted = self
            .panels
            .iter()
            .any(|p| p.row.all().any(|w| w.name == "apps" || w.name == "windows"));
        if !wanted || !self.live.installed.is_empty() {
            return;
        }
        let installed = apps::read_all(&apps::dirs());
        self.live.pinned = self
            .pins
            .iter()
            .filter_map(|pin| apps::pinned(&installed, pin))
            .map(widgets::Pin::from)
            .collect();
        self.live.installed = installed.iter().map(widgets::Pin::from).collect();
        self.draw_all();
    }

    /// Closes every popup that hangs on a panel or shows over the desktop,
    /// each its own way, so none is left pointing at a panel that goes.
    fn close_popups(&mut self) {
        self.close_launcher();
        self.close_styles();
        self.close_quick();
        self.hide_banner();
        self.hide_osd();
        self.close_centre();
        self.close_tray_grid();
        self.hide_tooltip();
        self.hide_switcher();
    }

    /// `input` at `x` logical pixels along panel `i`: the widget there,
    /// told how far along it and how wide it is, says what it does.
    fn input(&mut self, i: usize, x: f32, input: impl Fn(f32, f32) -> Input) {
        if let Some((action, left, width)) = self.action_at(i, x, input) {
            self.run_action(i, x, action, left, width);
        }
    }

    /// The action of the widget at `x` logical pixels along panel `i`, not
    /// yet run, with the widget's left edge and width: the widget, told how
    /// far along it and how wide it is, says what it does.
    fn action_at(
        &mut self,
        i: usize,
        x: f32,
        input: impl Fn(f32, f32) -> Input,
    ) -> Option<(Action, f32, f32)> {
        let panel = &self.panels[i];
        let j = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))?;
        let (Some(widget), Some(look)) = (panel.row.widget(j), &panel.drawn) else {
            return None;
        };
        let shown = look.shown.get(j).map_or("", String::as_str);
        let (left, width) = panel.places[j];
        // A strip as wide as the panel to measure on, as drawing does:
        // where the window list's buttons lie depends on its titles'
        // widths.
        let mut strip = Pixmap::new(panel.width * panel.scale, 1)?;
        let mut canvas = widgets::Canvas {
            pixmap: &mut strip,
            tokens: &self.tokens,
            text: Some(&mut self.text),
            icons: None,
            scale: panel.scale as f32,
            top: 0.0,
            height: 1.0,
            dock: panel.style == Style::Dock,
            along_top: panel.edge == Edge::Top,
        };
        let action = (widget.input)(&mut canvas, shown, input(x - left, width))?;
        Some((action, left, width))
    }

    /// Does `action`, the widget at `x` along panel `i` (`left` and `width`
    /// as `action_at` gave them).
    fn run_action(&mut self, i: usize, x: f32, action: Action, left: f32, width: f32) {
        match action {
            Action::Show(name) => self.workspaces.show(&name),
            Action::View(first) => {
                self.live.view = Some(first);
                self.draw_all();
            }
            Action::Activate(window) => {
                let seat = self.seat.seats().next();
                self.toplevels.activate(window, seat.as_ref());
            }
            Action::Minimize(window) => self.toplevels.minimize(window),
            Action::TogglePolicy => self.link.toggle_policy(),
            Action::NextKeyboardLayout => self.link.next_keyboard_layout(),
            Action::Launcher => self.toggle_launcher(),
            Action::Styles => self.toggle_styles(i, left + width / 2.0),
            Action::Quick => self.toggle_quick(),
            Action::Centre => self.toggle_centre(),
            Action::TrayOpen => self.toggle_tray_grid(i, left + widgets::tray::arrow_middle()),
            Action::App(id) => self.open_app(&id),
            Action::Tray(id, menu) => self.tray_call(&id, menu, x),
        }
    }

    /// A click on a tray icon (M5.2e): the item's app is asked to
    /// activate, or for its menu, `x` logical pixels along the panel. It
    /// is not told the screen's place, which shell-ui does not know, so a
    /// window it opens there is placed by the compositor.
    fn tray_call(&mut self, id: &str, menu: bool, x: f32) {
        let Some(connection) = &self._portal else {
            return;
        };
        let method = if menu { "ContextMenu" } else { "Activate" };
        tray::call(connection, id, method, (x as i32, 0));
    }

    /// The tray's tasks said an item came, changed or left (M5.2e).
    fn tray_changed(&mut self, event: tray::Event) {
        let before = self.live.tray.len();
        match event {
            tray::Event::Item(item) => match self.live.tray.iter_mut().find(|i| i.id == item.id) {
                Some(known) => *known = item,
                None => self.live.tray.push(item),
            },
            tray::Event::Gone(id) => self.live.tray.retain(|i| i.id != id),
        }
        if self.live.tray.len() != before {
            eprintln!("edel-shell-ui: tray: {} items", self.live.tray.len());
        }
        self.live.tray_in_panel = tray_in_panel();
        self.tray_split_changed();
        self.draw_all();
    }

    /// A click on an app in the apps widget (M5.4c): its window comes
    /// forward, or goes down if it is the focused one, or the app starts.
    fn open_app(&mut self, id: &str) {
        let tasks = self.toplevels.tasks();
        let mine: Vec<usize> = (0..tasks.len())
            .filter(|&i| widgets::apps::belongs(&tasks[i].app_id, id))
            .collect();
        if let Some(&i) = mine
            .iter()
            .find(|&&i| tasks[i].focused && !tasks[i].minimized)
        {
            self.toplevels.minimize(i);
        } else if let Some(&i) = mine.first() {
            let seat = self.seat.seats().next();
            self.toplevels.activate(i, seat.as_ref());
        } else if let Some(app) = apps::read_all(&apps::dirs())
            .into_iter()
            .find(|a| a.id == id)
        {
            self.launcher.run(&app);
        }
    }

    /// Super or the menu button: the launcher opens, or closes if open.
    fn toggle_launcher(&mut self) {
        if self.menu.is_some() {
            self.close_launcher();
        } else {
            self.open_launcher();
        }
    }

    /// Opens the launcher beside the first panel's start, on the screen
    /// the compositor picks, with the keyboard.
    fn open_launcher(&mut self) {
        self.close_styles();
        self.close_quick();
        self.close_centre();
        self.close_tray_grid();
        let (edge, scale) = self
            .panels
            .first()
            .map_or((Edge::Bottom, 1), |p| (p.edge, p.scale));
        let size = launcher::size(&self.tokens);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(popup) = Popup::new(self, LAUNCHER, size, size, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let surface = &popup.surface;
        surface.set_anchor(side | Anchor::LEFT);
        // The card keeps its place; its shadow reaches past it.
        let m = MARGIN - room as i32;
        surface.set_margin(m, m, m, m);
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.launcher_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the launcher: {e}"))
                .ok()
        });
        self.launcher.open();
        self.menu = Some(Menu { popup, keyboard });
        // The menu button's tile lights while the launcher is open.
        self.live.launcher = true;
        self.draw_all();
    }

    /// Closes the launcher and lets go of its keyboard, buffers and apps.
    fn close_launcher(&mut self) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            keyboard.release();
        }
        drop(menu);
        self.launcher.close();
        self.live.launcher = false;
        self.draw_all();
        eprintln!("edel-shell-ui: launcher hidden");
    }

    /// Draws the launcher if what it shows changed.
    fn draw_launcher(&mut self) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let view = self.launcher.view();
        let Some(mut pixmap) = menu.popup.canvas(&view) else {
            return;
        };
        let scale = menu.popup.scale();
        launcher::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        if menu
            .popup
            .show(view, &pixmap, &self.tokens, "launcher", &self.qh)
        {
            eprintln!(
                "edel-shell-ui: launcher shown, {} apps",
                self.launcher.count()
            );
        }
    }

    /// A key while the launcher is open: Escape closes, Return starts the
    /// chosen app, Up and Down choose, Backspace erases, and text searches.
    fn launcher_key(&mut self, event: KeyEvent) {
        if self.menu.is_none() {
            return;
        }
        match event.keysym {
            Keysym::Escape => return self.close_launcher(),
            Keysym::Return | Keysym::KP_Enter => {
                if self.launcher.start(None).is_some() {
                    return self.close_launcher();
                }
            }
            Keysym::Up => self.launcher.step(-1),
            Keysym::Down | Keysym::Tab => self.launcher.step(1),
            Keysym::BackSpace => self.launcher.erase(),
            _ => {
                // Keys that type nothing (arrows, F-keys) give "".
                if let Some(text) = event
                    .utf8
                    .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                {
                    self.launcher.typed(&text);
                }
            }
        }
        self.draw_launcher();
    }

    /// A right click on the layout button: the tiling styles' menu opens
    /// beside it, centred on `centre` logical pixels along panel `i`, or
    /// closes if open (M5.16b).
    fn toggle_styles(&mut self, i: usize, centre: f32) {
        if self.styles.is_some() {
            return self.close_styles();
        }
        self.close_launcher();
        self.close_quick();
        self.close_centre();
        self.close_tray_grid();
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, panel_width) = (panel.edge, panel.scale, panel.width);
        let size = styles::size(&self.tokens);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(popup) = Popup::new(self, STYLES, size, size, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        // Centred on the button, kept inside the screen.
        let most = (panel_width as i32 - size.0 as i32 - MARGIN).max(MARGIN);
        let left = (centre as i32 - size.0 as i32 / 2).clamp(MARGIN, most);
        let surface = &popup.surface;
        surface.set_anchor(side | Anchor::LEFT);
        let m = MARGIN - room as i32;
        surface.set_margin(m, m, m, left - room as i32);
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.styles_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the styles menu: {e}"))
                .ok()
        });
        let (machine, person) = settings_texts();
        let chosen = styles::in_use(machine.as_deref(), person.as_deref());
        self.styles = Some(StylesMenu {
            popup,
            keyboard,
            view: styles::View {
                chosen,
                lit: chosen,
            },
        });
    }

    /// Closes the styles' menu and lets go of its keyboard and buffers.
    fn close_styles(&mut self) {
        let Some(menu) = self.styles.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            keyboard.release();
        }
        eprintln!("edel-shell-ui: styles menu hidden");
    }

    /// Draws the styles' menu if what it shows changed.
    fn draw_styles(&mut self) {
        let Some(menu) = &mut self.styles else {
            return;
        };
        let view = menu.view.clone();
        let Some(mut pixmap) = menu.popup.canvas(&view) else {
            return;
        };
        let scale = menu.popup.scale();
        styles::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        if menu
            .popup
            .show(view, &pixmap, &self.tokens, "styles menu", &self.qh)
        {
            let style = styles::styles()
                .get(menu.view.chosen)
                .copied()
                .unwrap_or("");
            eprintln!("edel-shell-ui: styles menu shown, {style} in use");
        }
    }

    /// The style in row `row` chosen: written to the person's settings
    /// file, or taken out of it when it is what applies without it, as
    /// writers never write a default (ADR-008); the menu closes.
    fn choose_style(&mut self, row: usize) {
        let Some(&style) = styles::styles().get(row) else {
            return;
        };
        let (machine, _) = settings_texts();
        let without = styles::in_use(machine.as_deref(), None);
        let value = (row != without).then_some(style);
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => match settings::write(&path, styles::KEY, value) {
                Ok(()) => eprintln!("edel-shell-ui: tiling style {style} chosen"),
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::style_not_kept(style, format!("{e:#}"))
                ),
            },
            None => eprintln!("edel-shell-ui: {}", messages::style_no_home(style)),
        }
        self.close_styles();
    }

    /// A key while the styles' menu is open: Escape closes, Up and Down
    /// move, Return chooses.
    fn styles_key(&mut self, event: KeyEvent) {
        let Some(menu) = &mut self.styles else {
            return;
        };
        let last = styles::styles().len().saturating_sub(1);
        match event.keysym {
            Keysym::Escape => return self.close_styles(),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let row = menu.view.lit;
                return self.choose_style(row);
            }
            Keysym::Up => menu.view.lit = menu.view.lit.saturating_sub(1),
            Keysym::Down | Keysym::Tab => menu.view.lit = (menu.view.lit + 1).min(last),
            _ => {}
        }
        self.draw_styles();
    }

    fn is_styles(&self, surface: &wl_surface::WlSurface) -> bool {
        self.styles.as_ref().is_some_and(|m| m.popup.is(surface))
    }

    /// The compositor's switcher shows `view`: its surface is made, or
    /// sized again when the number of titles changed, then drawn.
    fn show_switcher(&mut self, view: switcher::View) {
        let rows = view.titles.len().min(switcher::MOST);
        if rows == 0 {
            return self.hide_switcher();
        }
        self.flipped = view;
        let size = switcher::size(rows, &self.tokens);
        match &mut self.flip {
            Some(flip) => flip.resize(size, &self.compositor),
            None => {
                let scale = self.panels.first().map_or(1, |p| p.scale);
                let most = switcher::size(switcher::MOST, &self.tokens);
                let room = paint::shadow_room(&self.tokens, !fillets());
                let Some(flip) = Popup::new(self, SWITCHER, size, most, scale, room) else {
                    return;
                };
                // No anchor: the middle of the screen.
                flip.surface
                    .set_keyboard_interactivity(KeyboardInteractivity::None);
                flip.surface.commit();
                self.flip = Some(flip);
            }
        }
        self.draw_switcher();
    }

    fn hide_switcher(&mut self) {
        if self.flip.take().is_some() {
            self.flipped = switcher::View::default();
            eprintln!("edel-shell-ui: switcher hidden");
        }
    }

    /// Draws the switcher if what it shows changed.
    fn draw_switcher(&mut self) {
        let Some(flip) = &mut self.flip else {
            return;
        };
        let Some(mut pixmap) = flip.canvas(&self.flipped) else {
            return;
        };
        let scale = flip.scale();
        switcher::paint(
            &mut pixmap,
            &self.flipped,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        if flip.show(
            self.flipped.clone(),
            &pixmap,
            &self.tokens,
            "switcher",
            &self.qh,
        ) {
            let rows = self.flipped.titles.len();
            eprintln!("edel-shell-ui: switcher shown, {rows} windows");
        }
    }

    fn is_switcher(&self, surface: &wl_surface::WlSurface) -> bool {
        self.flip.as_ref().is_some_and(|f| f.is(surface))
    }

    /// Whether `surface` is the open launcher's.
    fn is_launcher(&self, surface: &wl_surface::WlSurface) -> bool {
        self.menu.as_ref().is_some_and(|m| m.popup.is(surface))
    }

    /// The panel whose surface is `surface`.
    fn panel_of(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.panels
            .iter()
            .position(|p| p.surface.wl_surface() == surface)
    }
}

impl LayerShellHandler for Shell {
    /// The compositor took a panel away (its screen went): shell-ui ends,
    /// and the compositor starts it again, every panel afresh. The
    /// launcher just closes.
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &LayerSurface) {
        if self.is_launcher(surface.wl_surface()) {
            self.close_launcher();
        } else if self.is_styles(surface.wl_surface()) {
            self.close_styles();
        } else if self.is_quick(surface.wl_surface()) {
            self.close_quick();
        } else if self.is_banner(surface.wl_surface()) {
            self.hide_banner();
        } else if self.is_osd(surface.wl_surface()) {
            self.hide_osd();
        } else if self.is_centre(surface.wl_surface()) {
            self.close_centre();
        } else if self.is_tray_grid(surface.wl_surface()) {
            self.close_tray_grid();
        } else if self.is_tooltip(surface.wl_surface()) {
            self.hide_tooltip();
        } else if self.is_switcher(surface.wl_surface()) {
            self.hide_switcher();
        } else {
            self.exit = true;
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if self.is_launcher(surface.wl_surface()) {
            if let Some(menu) = &mut self.menu {
                menu.popup.configured();
            }
            return self.draw_launcher();
        }
        if self.is_styles(surface.wl_surface()) {
            if let Some(menu) = &mut self.styles {
                menu.popup.configured();
            }
            return self.draw_styles();
        }
        if self.is_quick(surface.wl_surface()) {
            if let Some(card) = &mut self.quick {
                card.popup_mut().configured();
            }
            return self.draw_quick();
        }
        if self.is_banner(surface.wl_surface()) {
            if let Some(card) = &mut self.banner {
                card.popup_mut().configured();
            }
            return self.draw_banner();
        }
        if self.is_osd(surface.wl_surface()) {
            if let Some(card) = &mut self.osd {
                card.popup.configured();
            }
            return self.draw_osd();
        }
        if self.is_centre(surface.wl_surface()) {
            if let Some(card) = &mut self.centre {
                card.popup_mut().configured();
            }
            return self.draw_centre();
        }
        if self.is_tray_grid(surface.wl_surface()) {
            if let Some(card) = &mut self.tray_grid {
                card.popup.configured();
            }
            return self.draw_tray_grid();
        }
        if self.is_tooltip(surface.wl_surface()) {
            if let Some(tip) = &mut self.tooltip {
                tip.popup.configured();
            }
            return self.draw_tooltip();
        }
        if self.is_switcher(surface.wl_surface()) {
            if let Some(flip) = &mut self.flip {
                flip.configured();
            }
            return self.draw_switcher();
        }
        let Some(i) = self.panel_of(surface.wl_surface()) else {
            return;
        };
        let (w, _) = configure.new_size;
        if w > 0 {
            self.panels[i].width = w;
        }
        self.draw(i);
    }
}

impl CompositorHandler for Shell {
    /// The screen's scale: the panel draws at it, sharp.
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if self.is_launcher(surface) {
            if let Some(menu) = &mut self.menu {
                menu.popup.set_scale(factor);
            }
            return self.draw_launcher();
        }
        if self.is_styles(surface) {
            if let Some(menu) = &mut self.styles {
                menu.popup.set_scale(factor);
            }
            return self.draw_styles();
        }
        if self.is_quick(surface) {
            if let Some(card) = &mut self.quick {
                card.popup_mut().set_scale(factor);
            }
            return self.draw_quick();
        }
        if self.is_banner(surface) {
            if let Some(card) = &mut self.banner {
                card.popup_mut().set_scale(factor);
            }
            return self.draw_banner();
        }
        if self.is_osd(surface) {
            if let Some(card) = &mut self.osd {
                card.popup.set_scale(factor);
            }
            return self.draw_osd();
        }
        if self.is_centre(surface) {
            if let Some(card) = &mut self.centre {
                card.popup_mut().set_scale(factor);
            }
            return self.draw_centre();
        }
        if self.is_tray_grid(surface) {
            if let Some(card) = &mut self.tray_grid {
                card.popup.set_scale(factor);
            }
            return self.draw_tray_grid();
        }
        if self.is_tooltip(surface) {
            if let Some(tip) = &mut self.tooltip {
                tip.popup.set_scale(factor);
            }
            return self.draw_tooltip();
        }
        if self.is_switcher(surface) {
            if let Some(flip) = &mut self.flip {
                flip.set_scale(factor);
            }
            return self.draw_switcher();
        }
        let Some(i) = self.panel_of(surface) else {
            return;
        };
        self.panels[i].scale = factor.clamp(1, 4) as u32;
        self.draw(i);
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    /// The compositor showed a surface's last drawing: it draws again if
    /// what it shows changed meanwhile.
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.is_launcher(surface) {
            if let Some(menu) = &mut self.menu {
                menu.popup.framed();
            }
            return self.draw_launcher();
        }
        if self.is_styles(surface) {
            if let Some(menu) = &mut self.styles {
                menu.popup.framed();
            }
            return self.draw_styles();
        }
        if self.is_quick(surface) {
            if let Some(card) = &mut self.quick {
                card.popup_mut().framed();
            }
            return self.draw_quick();
        }
        if self.is_banner(surface) {
            if let Some(card) = &mut self.banner {
                card.popup_mut().framed();
            }
            return self.draw_banner();
        }
        if self.is_osd(surface) {
            if let Some(card) = &mut self.osd {
                card.popup.framed();
            }
            return self.draw_osd();
        }
        if self.is_centre(surface) {
            if let Some(card) = &mut self.centre {
                card.popup_mut().framed();
            }
            return self.draw_centre();
        }
        if self.is_tray_grid(surface) {
            if let Some(card) = &mut self.tray_grid {
                card.popup.framed();
            }
            return self.draw_tray_grid();
        }
        if self.is_tooltip(surface) {
            if let Some(tip) = &mut self.tooltip {
                tip.popup.framed();
            }
            return self.draw_tooltip();
        }
        if self.is_switcher(surface) {
            if let Some(flip) = &mut self.flip {
                flip.framed();
            }
            return self.draw_switcher();
        }
        let Some(i) = self.panel_of(surface) else {
            return;
        };
        self.panels[i].waiting = false;
        self.draw(i);
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Shell {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Shell {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Shell {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers!(OutputState, SeatState);
}

impl SeatHandler for Shell {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    /// A pointer, for clicks and scrolls on the widgets; the keyboard is
    /// asked for only while the launcher is open.
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(e) => eprintln!("edel-shell-ui: no pointer: {e}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Shell {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if self.is_launcher(&event.surface) {
                // From the card's corner, inside the shadow's room.
                let room = self.menu.as_ref().map_or(0, |m| m.popup.room()) as f32;
                let (x, y) = (
                    event.position.0 as f32 - room,
                    event.position.1 as f32 - room,
                );
                let row = launcher::row_at(y, &self.tokens).filter(|_| x >= 0.0);
                match &event.kind {
                    PointerEventKind::Motion { .. } => {
                        if let Some(row) = row.filter(|r| *r < self.launcher.view().names.len()) {
                            self.launcher.selected = row;
                            self.draw_launcher();
                        }
                    }
                    PointerEventKind::Press { button, .. }
                        if *button == BTN_LEFT
                            && row.is_some()
                            && self.launcher.start(row).is_some() =>
                    {
                        self.close_launcher();
                    }
                    _ => {}
                }
                continue;
            }
            if self.is_styles(&event.surface) {
                let room = self.styles.as_ref().map_or(0, |m| m.popup.room()) as f32;
                let (x, y) = (
                    event.position.0 as f32 - room,
                    event.position.1 as f32 - room,
                );
                let row = styles::row_at(y, &self.tokens).filter(|_| x >= 0.0);
                match &event.kind {
                    PointerEventKind::Motion { .. } => {
                        if let (Some(row), Some(menu)) = (row, &mut self.styles) {
                            menu.view.lit = row;
                            self.draw_styles();
                        }
                    }
                    PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                        if let Some(row) = row {
                            self.choose_style(row);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if self.is_quick(&event.surface) {
                self.quick_pointer(event);
                continue;
            }
            if self.is_banner(&event.surface) {
                self.banner_pointer(event);
                continue;
            }
            if self.is_osd(&event.surface) {
                self.osd_pointer(event);
                continue;
            }
            if self.is_centre(&event.surface) {
                self.centre_pointer(event);
                continue;
            }
            if self.is_tray_grid(&event.surface) {
                self.tray_pointer(event);
                continue;
            }
            let Some(i) = self.panel_of(&event.surface) else {
                continue;
            };
            let x = event.position.0 as f32;
            match &event.kind {
                // The tray's tooltip waits for a rest on its arrow (M5.9h).
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.tooltip_hover(i, x);
                }
                PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                    self.hide_tooltip();
                    // A kept tray icon's click waits for the release, so
                    // the icon can be dragged (M5.9g).
                    match self.action_at(i, x, Input::Click) {
                        Some((Action::Tray(id, false), ..)) => {
                            let y = event.position.1 as f32;
                            self.tray_press = Some((i, id, x, y));
                        }
                        Some((action, left, width)) => {
                            self.run_action(i, x, action, left, width);
                        }
                        None => {}
                    }
                }
                PointerEventKind::Press { button, .. } if *button == BTN_RIGHT => {
                    self.hide_tooltip();
                    self.input(i, x, Input::Menu);
                }
                PointerEventKind::Release { button, .. } if *button == BTN_LEFT => {
                    let y = event.position.1 as f32;
                    self.tray_panel_release(i, x, y);
                }
                PointerEventKind::Leave { .. } => {
                    self.scrolled.reset();
                    self.tray_press = None;
                    self.hide_tooltip();
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    // A wheel's steps, else a touchpad's pixels, a step
                    // for every 40, adding up small ones; down or right
                    // moves towards the end.
                    let n = self.scrolled.steps(
                        vertical.value120 + horizontal.value120,
                        vertical.discrete + horizontal.discrete,
                        vertical.absolute + horizontal.absolute,
                    );
                    if n != 0 {
                        self.input(i, x, |_, _| Input::Scroll(n));
                    }
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for Shell {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    /// The keyboard went elsewhere, a window clicked: the launcher closes.
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.is_launcher(surface) {
            self.close_launcher();
        } else if self.is_styles(surface) {
            self.close_styles();
        } else if self.is_quick(surface) {
            self.close_quick();
        } else if self.is_centre(surface) {
            self.close_centre();
        } else if self.is_tray_grid(surface) {
            self.close_tray_grid();
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if self.quick.is_some() {
            self.quick_key(event);
        } else if self.centre.is_some() {
            self.centre_key(event);
        } else if self.styles.is_some() {
            self.styles_key(event);
        } else if self.tray_grid.is_some() {
            self.tray_key(event);
        } else {
            self.launcher_key(event);
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if self.quick.is_some() {
            self.quick_key(event);
        } else if self.centre.is_some() {
            self.centre_key(event);
        } else if self.styles.is_some() {
            self.styles_key(event);
        } else if self.tray_grid.is_some() {
            self.tray_key(event);
        } else {
            self.launcher_key(event);
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.shift = modifiers.shift;
    }
}

/// The machine's settings file and the person's, as text, for what the
/// menus mark as in use.
fn settings_texts() -> (Option<String>, Option<String>) {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(places::found(&path)).ok();
    (
        read(places::machine_settings()),
        places::person_settings().and_then(read),
    )
}

/// `layout.tray_in_panel` as the machine's and the person's files say it
/// now: the apps kept in the panel (M5.9g).
fn tray_in_panel() -> Vec<String> {
    let (machine, person) = settings_texts();
    edel::settings::texts(
        edel::settings::TRAY_IN_PANEL,
        machine.as_deref(),
        person.as_deref(),
    )
    .unwrap_or_default()
}

/// Removes the whole pages inside `bytes` from memory; reading them
/// again gives zeros. Only for a buffer no one draws from any more.
fn release_pages(bytes: &mut [u8]) {
    let page = rustix::param::page_size();
    let start = bytes.as_mut_ptr() as usize;
    let first = start.div_ceil(page) * page;
    let end = (start + bytes.len()) / page * page;
    if end <= first {
        return;
    }
    // SAFETY: the range lies inside `bytes`, a mapping of the shm pool
    // that this process owns and nothing reads until it is drawn anew.
    let removed = unsafe {
        rustix::mm::madvise(
            first as *mut _,
            end - first,
            rustix::mm::Advice::LinuxRemove,
        )
    };
    if let Err(e) = removed {
        eprintln!("edel-shell-ui: {}", messages::pages_not_given_back(e));
    }
}

delegate_registry!(Shell);
smithay_client_toolkit::delegate_dispatch2!(Shell);
