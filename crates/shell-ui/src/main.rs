//! `edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second
//! long-running process beside the compositor, which starts it and starts
//! it again if it ends. It draws its own surfaces (ADR-002's shell-ui
//! toolkit decision of 2026-10-03): layer-shell surfaces through
//! smithay-client-toolkit, drawn with tiny-skia into shared memory, text
//! shaped by cosmic-text, all from the design tokens. Today it draws the
//! preset's panels (M5.1b, M5.1c): the system file's `shell.preset`, else
//! Classic, whose one panel runs along the bottom of the first screen
//! with the menu button, the window list (M5.2h), the workspace switcher
//! (M5.2c), the layout toggle (M5.3a) and a clock. Each
//! panel holds widgets from the table in `widgets/` and is drawn again
//! only when what a widget shows, or the panel's size or scale, changes;
//! a click or a scroll on a widget does what the widget says. Super,
//! tapped alone, or the menu button opens the launcher (M5.3b,
//! `launcher.rs`). It exits when the compositor goes away.

mod a11y;
mod apps;
mod launcher;
mod link;
mod paint;
mod popup;
mod switcher;
mod toplevels;
mod widgets;
mod workspaces;

use anyhow::{Context, Result};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
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
    BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use tiny_skia::Pixmap;

use edel::presets::{self, Edge};
use edel::system;
use edel::tokens::{self, Tokens};

use crate::paint::{Look, Row, Text};
use crate::popup::Popup;
use crate::widgets::{Action, Input, Live};

/// The panels' layer surfaces' namespace, as the compositor's state file
/// lists it, and the launcher's.
const NAMESPACE: &str = "edel-panel";
const LAUNCHER: &str = "edel-launcher";
const SWITCHER: &str = "edel-switcher";
/// The launcher's distance from the panel and the screen's side.
const MARGIN: i32 = 8;

/// Where the compositor says how much the desktop animates (M5.11a).
const STATE: &str = "/run/edel/session/state.toml";

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
    panels: Vec<Panel>,
    launcher: launcher::Launcher,
    /// The launcher's surface while it is open.
    menu: Option<Menu>,
    /// Scrolling over a panel not yet a whole step.
    scrolled: widgets::Scrolled,
    /// The window switcher's surface while Alt+Tab is held (M5.3c), and
    /// what it shows.
    flip: Option<Popup<switcher::View>>,
    flipped: switcher::View,
    qh: QueueHandle<Shell>,
    handle: LoopHandle<'static, Shell>,
    tokens: Tokens,
    text: Text,
    fillets: bool,
    exit: bool,
}

/// The open launcher's surface and the keyboard, let go when it closes.
struct Menu {
    popup: Popup<launcher::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
}

/// One of the preset's panels.
struct Panel {
    edge: Edge,
    surface: LayerSurface,
    row: Row,
    /// Its logical width, once the compositor has said it.
    width: u32,
    scale: u32,
    /// What was drawn last, so nothing is drawn twice.
    drawn: Option<Look>,
    /// Where each widget lies, start to end: its left edge and width in
    /// logical pixels, for clicks.
    places: Vec<(f32, f32)>,
    /// What screen readers read of it (M5.1d).
    reader: a11y::Reader,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("edel-shell-ui: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let tokens = load_tokens();
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, queue) = registry_queue_init(&connection).context("reading the globals")?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let layers = LayerShell::bind(&globals, &qh).context("no zwlr_layer_shell_v1")?;
    let strip = paint::fillet_height(&tokens);
    let features = std::path::Path::new(edel::features::DIR);
    let mut panels = Vec::new();
    for spec in &preset().panels {
        let pick = |names: &[String]| {
            let (found, notes) = widgets::usable(names, features);
            for note in notes {
                eprintln!("edel-shell-ui: {note}");
            }
            found
        };
        let row = Row {
            start: pick(&spec.start),
            centre: pick(&spec.centre),
            end: pick(&spec.end),
        };
        // Along the edge of the first screen; the strip on its inner side
        // for the fillets is drawn but takes no space and no clicks.
        let surface = compositor.create_surface(&qh);
        let surface = layers.create_layer_surface(&qh, surface, Layer::Top, Some(NAMESPACE), None);
        let edge = match spec.edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        surface.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
        surface.set_size(0, tokens.panel_height + strip);
        surface.set_exclusive_zone(tokens.panel_height as i32);
        surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        surface.commit();
        eprintln!(
            "edel-shell-ui: panel {NAMESPACE} along the {}",
            spec.edge.name()
        );
        panels.push(Panel {
            edge: spec.edge,
            surface,
            row,
            width: 0,
            scale: 1,
            drawn: None,
            places: Vec::new(),
            reader: a11y::Reader::new(),
        });
    }
    let pool = SlotPool::new(1280 * (tokens.panel_height + strip) as usize * 4, &shm)
        .context("creating the shared memory pool")?;
    let mut event_loop: EventLoop<Shell> =
        EventLoop::try_new().context("starting the event loop")?;
    let mut shell = Shell {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        pointer: None,
        live: Live::default(),
        workspaces: workspaces::Workspaces::bind(&globals, &qh),
        toplevels: toplevels::Toplevels::bind(&globals, &qh),
        link: link::Link::bind(&globals, &qh),
        compositor,
        layers,
        shm,
        pool,
        panels,
        launcher: launcher::Launcher::default(),
        menu: None,
        flip: None,
        flipped: switcher::View::default(),
        scrolled: widgets::Scrolled::default(),
        qh: qh.clone(),
        handle: event_loop.handle(),
        text: Text::load(&tokens.font),
        tokens,
        fillets: fillets(),
        exit: false,
    };
    WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("watching the compositor: {e}"))?;
    tick(&event_loop.handle());
    while !shell.exit {
        // When the compositor goes away, so does the panel: the end.
        if event_loop.dispatch(None, &mut shell).is_err() {
            eprintln!("edel-shell-ui: the compositor went away");
            break;
        }
    }
    Ok(())
}

/// The image's tokens if it has them, else the built-in ones; anything
/// skipped is reported, never fatal (ADR-008).
fn load_tokens() -> Tokens {
    let Ok(text) = std::fs::read_to_string(tokens::PATH) else {
        return Tokens::built_in();
    };
    let (tokens, notes) = Tokens::read(&text);
    for note in notes {
        eprintln!("edel-shell-ui: {}: {note}", tokens::PATH);
    }
    tokens
}

/// The preset the system files name, the person's over the machine's,
/// else Classic; a broken file or an unknown name is reported, never fatal
/// (ADR-008).
fn preset() -> presets::Preset {
    let mut name = None;
    let files = [Some(system::MACHINE_FILE.into()), system::person_file()];
    for path in files.into_iter().flatten() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match system::read(&text) {
            Ok(read) if read.file.shell.preset.is_some() => name = read.file.shell.preset,
            Ok(_) => {}
            Err(e) => eprintln!(
                "edel-shell-ui: {}: {e:#}; its keys are left out",
                path.display()
            ),
        }
    }
    let (preset, note) = presets::named(name.as_deref());
    if let Some(note) = note {
        eprintln!("edel-shell-ui: {note}");
    }
    preset
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
            TimeoutAction::ToDuration(next())
        },
    );
    if let Err(e) = result {
        eprintln!("edel-shell-ui: the clock's timer did not start: {e}");
    }
}

impl Shell {
    /// Draws panel `i` if anything it shows changed.
    fn draw(&mut self, i: usize) {
        let panel = &self.panels[i];
        if panel.width == 0 {
            return;
        }
        let strip = paint::fillet_height(&self.tokens);
        let look = Look {
            width: panel.width * panel.scale,
            height: (self.tokens.panel_height + strip) * panel.scale,
            scale: panel.scale,
            edge: panel.edge,
            fillets: self.fillets,
            shown: panel.row.shows(&self.live),
        };
        if panel.drawn.as_ref() == Some(&look) {
            return;
        }
        if let Err(e) = self.show(i, &look) {
            eprintln!("edel-shell-ui: drawing the panel failed: {e:#}");
            return;
        }
        // Screen readers get what was drawn, in logical pixels.
        let panel = &mut self.panels[i];
        let top = f64::from(paint::panel_top(panel.edge, &self.tokens));
        let bottom = top + f64::from(self.tokens.panel_height);
        let items = panel
            .row
            .all()
            .zip(&look.shown)
            .zip(&panel.places)
            .filter(|(_, (_, width))| *width > 0.0)
            .map(|((widget, shown), &(x, width))| a11y::Item {
                role: widget.role,
                label: (widget.label)(shown),
                bounds: accesskit::Rect::new(x.into(), top, (x + width).into(), bottom),
            })
            .collect();
        let size = (
            f64::from(panel.width),
            f64::from(self.tokens.panel_height + strip),
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
        // strip beside it lets both through.
        let top = paint::panel_top(panel.edge, &self.tokens) as i32;
        let panel_h = self.tokens.panel_height as i32;
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, top, panel.width as i32, panel_h);
            surface.set_opaque_region(Some(region.wl_region()));
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.set_buffer_scale(panel.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(surface).context("attaching the buffer")?;
        panel.surface.commit();
        self.panels[i].places = places;
        Ok(())
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

    /// `input` at `x` logical pixels along panel `i`: the widget there,
    /// told how far along it and how wide it is, says what it does.
    fn input(&mut self, i: usize, x: f32, input: impl Fn(f32, f32) -> Input) {
        let panel = &self.panels[i];
        let Some(j) = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))
        else {
            return;
        };
        let (Some(widget), Some(look)) = (panel.row.widget(j), &panel.drawn) else {
            return;
        };
        let shown = look.shown.get(j).map_or("", String::as_str);
        let (left, width) = panel.places[j];
        let action = (widget.input)(shown, input(x - left, width));
        match action {
            Some(Action::Show(name)) => self.workspaces.show(&name),
            Some(Action::View(first)) => {
                self.live.view = Some(first);
                self.draw_all();
            }
            Some(Action::Activate(window)) => {
                let seat = self.seat.seats().next();
                self.toplevels.activate(window, seat.as_ref());
            }
            Some(Action::Minimize(window)) => self.toplevels.minimize(window),
            Some(Action::TogglePolicy) => self.link.toggle_policy(),
            Some(Action::Launcher) => self.toggle_launcher(),
            None => {}
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
        let (edge, scale) = self
            .panels
            .first()
            .map_or((Edge::Bottom, 1), |p| (p.edge, p.scale));
        let size = launcher::size(&self.tokens);
        let Some(popup) = Popup::new(self, LAUNCHER, size, size, scale) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let surface = &popup.surface;
        surface.set_anchor(side | Anchor::LEFT);
        surface.set_margin(MARGIN, MARGIN, MARGIN, MARGIN);
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
        if menu.popup.show(view, &pixmap, "launcher") {
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
            Some(flip) => flip.resize(size),
            None => {
                let scale = self.panels.first().map_or(1, |p| p.scale);
                let most = switcher::size(switcher::MOST, &self.tokens);
                let Some(flip) = Popup::new(self, SWITCHER, size, most, scale) else {
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
        if flip.show(self.flipped.clone(), &pixmap, "switcher") {
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

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

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
                let (x, y) = (event.position.0 as f32, event.position.1 as f32);
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
            let Some(i) = self.panel_of(&event.surface) else {
                continue;
            };
            let x = event.position.0 as f32;
            match &event.kind {
                PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                    self.input(i, x, Input::Click);
                }
                PointerEventKind::Leave { .. } => self.scrolled.reset(),
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
        self.launcher_key(event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.launcher_key(event);
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
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

delegate_registry!(Shell);
smithay_client_toolkit::delegate_dispatch2!(Shell);
