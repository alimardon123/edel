//! The greeter (M5.10b): `edel-shell-ui --greeter --cmd COMMAND`, which
//! greetd runs on the greeter's own compositor. It is a small client of
//! its own, with none of the panels, the tray or the session bus: on every
//! screen a layer surface over everything, drawn as the lock screen is
//! (`lock::paint`), first asking for the user name, then for the password
//! with the person's initial and name over it. Return logs in through
//! greetd (`greetd::log_in`) on a thread; once greetd starts the session
//! (`COMMAND` as one word, which greetd runs through the shell itself,
//! so its quoting holds) the greeter ends, and so does its
//! compositor, as greetd expects. A refusal says "Wrong user name or
//! password" and asks for the user name again. Escape goes back a step.
//! Logs `greeter shown, N screens`, `greeter: logging USER in`,
//! `greeter: wrong user name or password` and `greeter: session started
//! for USER`.

use anyhow::{Context, Result};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
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

use edel::tokens::Tokens;

use crate::greetd::{self, Login};
use crate::lock::{self, Field, Password, View};
use crate::messages;
use crate::paint::{self, Text};

/// The greeter: its screens, what is typed and where it stands.
struct Greeter {
    registry: RegistryState,
    outputs: OutputState,
    seat: SeatState,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    qh: QueueHandle<Greeter>,
    screens: Vec<Screen>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    tokens: Tokens,
    text: Text,
    /// The user name typed, the password typed, and whether the password
    /// is asked for (the second step).
    name: String,
    password: Password,
    asking_password: bool,
    field: Field,
    /// A login under way: the password's letters, for its dots.
    busy: Option<usize>,
    /// The session's command, and where the login's answer comes back.
    cmd: String,
    tx: channel::Sender<Login>,
    exit: bool,
}

/// One screen's surface.
struct Screen {
    surface: LayerSurface,
    output: wl_output::WlOutput,
    size: Option<(u32, u32)>,
    scale: u32,
    pool: Option<SlotPool>,
    drawn: Option<View>,
}

/// Runs the greeter until the session starts or the compositor goes.
pub fn run(cmd: String) -> Result<()> {
    let (_, scheme) = crate::from_system_files();
    let tokens = crate::load_tokens(scheme);
    let connection = Connection::connect_to_env().context(messages::NO_COMPOSITOR)?;
    let (globals, queue) = registry_queue_init(&connection).context(messages::NO_GLOBALS)?;
    let qh = queue.handle();
    let compositor =
        CompositorState::bind(&globals, &qh).with_context(|| messages::missing("wl_compositor"))?;
    let shm = Shm::bind(&globals, &qh).with_context(|| messages::missing("wl_shm"))?;
    let layers = LayerShell::bind(&globals, &qh)
        .with_context(|| messages::missing("zwlr_layer_shell_v1"))?;
    let mut event_loop: EventLoop<Greeter> =
        EventLoop::try_new().context("starting the event loop")?;
    let (tx, rx) = channel::channel::<Login>();
    event_loop
        .handle()
        .insert_source(rx, |event, _, greeter: &mut Greeter| {
            if let channel::Event::Msg(login) = event {
                greeter.logged_in(login);
            }
        })
        .map_err(|e| anyhow::anyhow!("listening for greetd's answers: {e}"))?;
    let mut greeter = Greeter {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        compositor,
        layers,
        shm,
        qh: qh.clone(),
        screens: Vec::new(),
        keyboard: None,
        text: Text::load(&tokens.font),
        tokens,
        name: String::new(),
        password: Password::default(),
        asking_password: false,
        field: Field::Ready,
        busy: None,
        cmd,
        tx,
        exit: false,
    };
    WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("watching the compositor: {e}"))?;
    minute(&event_loop.handle());
    while !greeter.exit {
        if event_loop.dispatch(None, &mut greeter).is_err() {
            eprintln!("edel-shell-ui: {}", messages::COMPOSITOR_GONE);
            break;
        }
    }
    Ok(())
}

/// Draws the clock again on the minute.
fn minute(handle: &LoopHandle<'static, Greeter>) {
    let next = || crate::widgets::clock::until_next_minute(&jiff::Zoned::now());
    let _ = handle.insert_source(
        Timer::from_duration(next()),
        move |_, _, g: &mut Greeter| {
            g.draw();
            TimeoutAction::ToDuration(next())
        },
    );
}

impl Greeter {
    /// A surface over everything on `output`, with the keyboard.
    fn add_screen(&mut self, output: wl_output::WlOutput) {
        if self.screens.iter().any(|s| s.output == output) {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let surface = self.layers.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("edel-greeter"),
            Some(&output),
        );
        surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        surface.set_exclusive_zone(-1);
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let scale = self
            .outputs
            .info(&output)
            .map_or(1, |info| info.scale_factor.max(1) as u32);
        self.screens.push(Screen {
            surface,
            output,
            size: None,
            scale,
            pool: None,
            drawn: None,
        });
    }

    fn view(&self) -> View {
        let (time, date) = lock::time_and_date(&jiff::Zoned::now());
        let (field, dots) = match self.busy {
            Some(dots) => (Field::Checking, dots),
            None => (self.field, self.password.letters()),
        };
        View {
            time,
            date,
            name: self.name.clone(),
            dots,
            field,
            asking_name: !self.asking_password,
        }
    }

    /// Draws every configured screen whose view changed.
    fn draw(&mut self) {
        let view = self.view();
        let mut first = false;
        for screen in &mut self.screens {
            let Some((w, h)) = screen.size else {
                continue;
            };
            if screen.drawn.as_ref() == Some(&view) {
                continue;
            }
            let s = screen.scale;
            let (pw, ph) = (w * s, h * s);
            let Some(mut pixmap) = Pixmap::new(pw, ph) else {
                continue;
            };
            lock::paint(&mut pixmap, &view, &self.tokens, &mut self.text, s as f32);
            if screen.pool.is_none() {
                screen.pool = SlotPool::new(2 * (pw * ph * 4) as usize, &self.shm)
                    .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the greeter: {e}"))
                    .ok();
            }
            let Some(pool) = &mut screen.pool else {
                continue;
            };
            let Ok((buffer, canvas)) = pool.create_buffer(
                pw as i32,
                ph as i32,
                pw as i32 * 4,
                wl_shm::Format::Argb8888,
            ) else {
                continue;
            };
            paint::to_argb(&pixmap, canvas);
            let surface = screen.surface.wl_surface();
            surface.set_buffer_scale(s as i32);
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            if buffer.attach_to(surface).is_err() {
                continue;
            }
            screen.surface.commit();
            first |= screen.drawn.is_none();
            screen.drawn = Some(view.clone());
        }
        if first && self.screens.iter().all(|s| s.drawn.is_some()) {
            eprintln!(
                "edel-shell-ui: greeter shown, {} screens",
                self.screens.len()
            );
        }
    }

    /// A key: typed letters go to the field in use; Return moves on or
    /// logs in; Backspace takes a letter back; Escape goes back a step.
    fn key(&mut self, event: KeyEvent) {
        if self.busy.is_some() {
            return;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => {
                if !self.asking_password {
                    if !self.name.trim().is_empty() {
                        self.asking_password = true;
                        self.field = Field::Ready;
                    }
                } else if self.password.letters() > 0 {
                    return self.log_in();
                }
            }
            Keysym::BackSpace if self.asking_password => self.password.pop(),
            Keysym::BackSpace => {
                self.name.pop();
            }
            Keysym::Escape if self.asking_password => {
                self.password.clear();
                self.asking_password = false;
                self.field = Field::Ready;
            }
            Keysym::Escape => self.name.clear(),
            _ => {
                let Some(text) = event
                    .utf8
                    .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                else {
                    return;
                };
                if self.asking_password {
                    self.password.push(&text);
                } else if self.name.len() < 64 {
                    self.name.push_str(&text);
                }
                if matches!(self.field, Field::Refused | Field::Failed) && self.asking_password {
                    self.field = Field::Ready;
                }
            }
        }
        self.draw();
    }

    /// Return on the password: greetd is asked on a thread of its own.
    fn log_in(&mut self) {
        let user = self.name.trim().to_string();
        eprintln!("edel-shell-ui: greeter: logging {user} in");
        self.busy = Some(self.password.letters());
        let password = self.password.take();
        let cmd = self.cmd.clone();
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("greetd".into())
            .spawn(move || {
                let login = match greetd::Greetd::connect() {
                    Ok(mut greetd) => {
                        let words = password.text().unwrap_or_default();
                        greetd::log_in(&mut greetd, &user, words, &[&cmd])
                    }
                    Err(e) => Login::Failed(format!("{e:#}")),
                };
                drop(password);
                let _ = tx.send(login);
            });
        if let Err(e) = spawned {
            self.busy = None;
            eprintln!("edel-shell-ui: {}", messages::login_failed(e));
        }
        self.draw();
    }

    /// greetd's answer.
    fn logged_in(&mut self, login: Login) {
        self.busy = None;
        match login {
            Login::Started => {
                eprintln!(
                    "edel-shell-ui: greeter: session started for {}",
                    self.name.trim()
                );
                self.exit = true;
                return;
            }
            Login::Refused => {
                eprintln!("edel-shell-ui: greeter: wrong user name or password");
                self.field = Field::Refused;
            }
            Login::Failed(why) => {
                eprintln!("edel-shell-ui: {}", messages::login_failed(why));
                self.field = Field::Failed;
            }
        }
        // Asked again from the user name, which stays typed.
        self.password.clear();
        self.asking_password = false;
        self.draw();
    }
}

impl LayerShellHandler for Greeter {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &LayerSurface) {
        self.screens
            .retain(|s| s.surface.wl_surface() != surface.wl_surface());
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(screen) = self
            .screens
            .iter_mut()
            .find(|s| s.surface.wl_surface() == surface.wl_surface())
        {
            let size = configure.new_size;
            if size.0 > 0 && size.1 > 0 && screen.size != Some(size) {
                screen.size = Some(size);
                screen.drawn = None;
                screen.pool = None;
            }
        }
        self.draw();
    }
}

impl CompositorHandler for Greeter {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(screen) = self
            .screens
            .iter_mut()
            .find(|s| s.surface.wl_surface() == surface)
        {
            screen.scale = factor.max(1) as u32;
            screen.drawn = None;
            screen.pool = None;
        }
        self.draw();
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

impl OutputHandler for Greeter {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_screen(output);
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.screens.retain(|s| s.output != output);
    }
}

impl ShmHandler for Greeter {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Greeter {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers!(OutputState, SeatState);
}

impl SeatHandler for Greeter {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    /// The keyboard, as soon as the seat has one.
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Keyboard || self.keyboard.is_some() {
            return;
        }
        match self.seat.get_keyboard(qh, &seat, None) {
            Ok(keyboard) => self.keyboard = Some(keyboard),
            Err(e) => eprintln!("edel-shell-ui: no keyboard for the greeter: {e}"),
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Greeter {
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
        eprintln!("edel-shell-ui: greeter has the keyboard");
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
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

delegate_registry!(Greeter);
smithay_client_toolkit::delegate_dispatch2!(Greeter);
