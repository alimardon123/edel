//! The greeter (M5.10b): `edel-shell-ui --greeter --cmd COMMAND`, which
//! greetd runs on the greeter's own compositor. It is a small client of
//! its own, with none of the panels, the tray or the session bus: on every
//! screen a layer surface over everything, drawn as the lock screen is
//! (`lock::paint`). The machine's people stand in a row (`people.rs`, M5.10c),
//! the chosen one in the middle, with Other at the end. Left and right
//! choose (while the field is empty), and so does a click on a person or on
//! Other. A chosen person's password is asked at once. Other keeps two
//! steps: the user name, then the password with the person's initial and
//! name over it. Return logs in through greetd (`greetd::log_in`) on a
//! thread; once greetd starts the session (`COMMAND` as one word, which
//! greetd runs through the shell itself, so its quoting holds) the greeter
//! ends, and so does its compositor, as greetd expects. A refusal says
//! "Wrong user name or password" and asks again. Escape goes back a step.
//! Power sits in the corner (`corner.rs`) and opens its menu, as the lock
//! screen's does. Logs `greeter shown, N screens`, `greeter places ...`,
//! `greeter: chose LOGIN` or `greeter: chose other`, `greeter: power menu
//! shown, 3 rows`, `greeter: power menu hidden`, `greeter: power: NAME`,
//! `greeter: logging USER in`, `greeter: wrong user name or password` and
//! `greeter: session started for USER`, then `greeter: desktop ID`.
//! With two or more desktop sessions installed (M5.10c3b) a chip in the
//! corner names the one the login starts and opens a menu of them, each
//! person's choice is remembered in `places::GREETER_SESSIONS`, and the
//! session menu logs `greeter: session menu shown, N sessions`, `greeter:
//! session menu hidden` and `greeter: session NAME chosen`.

use std::path::Path;

use anyhow::{Context, Result};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel;
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

use edel::lock_screen::Placed;
use edel::places;
use edel::tokens::Tokens;

use crate::corner;
use crate::greetd::{self, Login};
use crate::lock::{self, Field, Password, View};
use crate::messages;
use crate::paint::{self, Text};
use crate::people;
use crate::power;
use crate::sessions::{self, Session};

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
    pointer: Option<wl_pointer::WlPointer>,
    tokens: Tokens,
    text: Text,
    /// The row of people and the one chosen (M5.10c).
    row: people::Row,
    /// The user name typed, the password typed, and whether the password
    /// is asked for (the second step, or at once for a chosen person).
    name: String,
    password: Password,
    asking_password: bool,
    field: Field,
    /// A login under way: the password's letters, for its dots.
    busy: Option<usize>,
    /// Whether Caps Lock is on (M5.10c).
    caps: bool,
    /// Power's menu is open, and whether an update waits for a restart.
    menu: bool,
    restart_waiting: bool,
    /// The corner's button or row under the pointer.
    corner_hover: Option<corner::Hit>,
    /// The last places line logged, so only a change is logged.
    logged: String,
    /// The desktop sessions offered, the default's index, and the one the
    /// login starts (M5.10c3b). The chip's name is measured for it in
    /// `chip_w`, and the session menu is open while `session_menu`.
    sessions: Vec<Session>,
    default_session: usize,
    session: usize,
    chip_w: f32,
    session_menu: bool,
    /// The remembered choices as the file held them at start.
    remembered: String,
    /// Where the login's answer comes back.
    tx: channel::Sender<Login>,
    exit: bool,
    /// The parts the machine's file says, read once at start (people have
    /// no files of their own here), and the pictures they show.
    parts: Option<Vec<Placed>>,
    pictures: lock::Pictures,
}

/// One screen's surface.
struct Screen {
    surface: LayerSurface,
    output: wl_output::WlOutput,
    size: Option<(u32, u32)>,
    scale: u32,
    pool: Option<SlotPool>,
    drawn: Option<(View, corner::View)>,
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
    // The first person is chosen, and their password asked at once; with no
    // people, Other is chosen (index 0 equals the count) and the name asked.
    let row = people::Row {
        people: people::from_users(&edel::users::read(None)),
        chosen: 0,
    };
    let (name, asking_password) = match row.people.first() {
        Some(person) => (person.login.clone(), true),
        None => (String::new(), false),
    };
    let parts = lock::parts_of(crate::settings_texts().0.as_deref(), None);
    // The sessions installed, with the one `--cmd` names as the default
    // (M5.10c3b). A machine with no session files offers only `--cmd`.
    let (sessions, default_session) =
        sessions::with_default(sessions::read(Path::new(places::WAYLAND_SESSIONS)), &cmd);
    // A missing or unreadable file means every person's default.
    let remembered = std::fs::read_to_string(places::GREETER_SESSIONS).unwrap_or_default();
    let mut greeter = Greeter {
        parts,
        pictures: lock::Pictures::default(),
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        compositor,
        layers,
        shm,
        qh: qh.clone(),
        screens: Vec::new(),
        keyboard: None,
        pointer: None,
        text: Text::load(&tokens.font),
        tokens,
        row,
        name,
        password: Password::default(),
        asking_password,
        field: Field::Ready,
        busy: None,
        caps: false,
        menu: false,
        restart_waiting: false,
        corner_hover: None,
        logged: String::new(),
        sessions,
        default_session,
        session: default_session,
        chip_w: 0.0,
        session_menu: false,
        remembered,
        tx,
        exit: false,
    };
    greeter.session_for_row();
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
        let now = jiff::Zoned::now();
        let (time, date) = lock::time_and_date(&now);
        let (field, dots) = match self.busy {
            Some(dots) => (Field::Checking, dots),
            None => (self.field, self.password.letters()),
        };
        View {
            dots,
            field,
            asking_name: !self.asking_password,
            caps: self.caps,
            people: self.people_shown().then(|| self.row.clone()),
            when: Some(lock::When::of(&now)),
            ..View::new(time, date, self.name.clone())
        }
    }

    /// What the corner shows now: Power, and the session chip when two or
    /// more sessions are installed, with the open menus.
    fn corner_view(&self) -> corner::View {
        let many = self.sessions.len() > 1;
        corner::View {
            layout: None,
            menu: self.menu,
            restart_waiting: self.restart_waiting,
            hover: self.corner_hover,
            session: many
                .then(|| self.sessions.get(self.session))
                .flatten()
                .map(|s| s.name.clone()),
            chip_w: self.chip_w,
            sessions: (many && self.session_menu).then(|| {
                let names = self.sessions.iter().map(|s| s.name.clone()).collect();
                (names, self.session)
            }),
        }
    }

    /// Makes the session `index` the one the login starts, and measures
    /// the chip for its name.
    fn set_session(&mut self, index: usize) {
        self.session = index;
        if let Some(session) = self.sessions.get(index) {
            self.chip_w = corner::chip_width(&mut self.text, &session.name);
        }
    }

    /// The session a person's login starts with: their remembered choice
    /// when it is still installed, else the default (M5.10c3b). Other, with
    /// no person chosen, starts the default.
    fn session_for_row(&mut self) {
        let remembered = self
            .row
            .people
            .get(self.row.chosen)
            .and_then(|person| sessions::remembered(&self.remembered, &person.login));
        let index = remembered
            .and_then(|id| self.sessions.iter().position(|s| s.id == id))
            .unwrap_or(self.default_session);
        self.set_session(index);
    }

    /// Opens or closes the session menu, logging it. It opens only with the
    /// power menu shut, and draws again.
    fn set_session_menu(&mut self, open: bool) {
        if open {
            self.menu = false;
            self.session_menu = true;
            eprintln!(
                "edel-shell-ui: greeter: session menu shown, {} sessions",
                self.sessions.len()
            );
        } else {
            self.session_menu = false;
            eprintln!("edel-shell-ui: greeter: session menu hidden");
        }
        self.draw();
    }

    /// A session row chosen: the login starts it, and the menu closes.
    fn choose_session(&mut self, index: usize) {
        let Some(session) = self.sessions.get(index) else {
            return;
        };
        eprintln!("edel-shell-ui: greeter: session {} chosen", session.name);
        self.set_session(index);
        self.session_menu = false;
        self.draw();
    }

    /// Writes the session a login started as the person's choice, or takes
    /// their line out when it is the default (writers never write a
    /// default). Nothing is written when the file already says so. A
    /// failure is logged and never stops the login (Reliable).
    fn remember_choice(&self) {
        let login = self.name.trim();
        let id = if self.session == self.default_session {
            None
        } else {
            self.sessions.get(self.session).map(|s| s.id.as_str())
        };
        let path = Path::new(places::GREETER_SESSIONS);
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if sessions::remembered(&text, login).as_deref() == id {
            return;
        }
        let tmp = path.with_extension("new");
        let written = std::fs::write(&tmp, sessions::remember(&text, login, id))
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = written {
            eprintln!("edel-shell-ui: {}", messages::session_not_kept(e));
        }
    }

    /// Whether the row of people is shown: a person is chosen, or Other is
    /// chosen and its name step shows. Other's password step shows the
    /// person's initial and name instead.
    fn people_shown(&self) -> bool {
        self.row.chosen < self.row.people.len() || !self.asking_password
    }

    /// Whether nothing is typed in the field in use: no password letters,
    /// or on Other's name step no name.
    fn field_empty(&self) -> bool {
        if self.asking_password {
            self.password.letters() == 0
        } else {
            self.name.is_empty()
        }
    }

    /// Chooses entry `index` of the row, when the field is empty. A person's
    /// password is asked at once; Other asks for the user name. The caller
    /// draws.
    fn choose(&mut self, index: usize) {
        if index == self.row.chosen || !self.field_empty() {
            return;
        }
        self.row.chosen = index;
        self.password.clear();
        self.field = Field::Ready;
        match self.row.people.get(index) {
            Some(person) => {
                eprintln!("edel-shell-ui: greeter: chose {}", person.login);
                self.name = person.login.clone();
                self.asking_password = true;
            }
            None => {
                eprintln!("edel-shell-ui: greeter: chose other");
                self.name.clear();
                self.asking_password = false;
            }
        }
        self.session_for_row();
    }

    /// Opens or closes Power's menu, logging it, and draws again. Opening
    /// reads whether an update waits for a restart, and shuts the session
    /// menu.
    fn set_menu(&mut self, open: bool) {
        if open {
            self.restart_waiting = power::restart_waiting();
            self.menu = true;
            self.session_menu = false;
            eprintln!(
                "edel-shell-ui: greeter: power menu shown, {} rows",
                power::CHOICES.len()
            );
        } else {
            self.menu = false;
            eprintln!("edel-shell-ui: greeter: power menu hidden");
        }
        self.draw();
    }

    /// Closes Power's menu, then runs `choice` on a thread of its own, so
    /// the drawing never waits for it.
    fn run_power(&mut self, choice: power::Choice) {
        self.menu = false;
        eprintln!("edel-shell-ui: greeter: power: {}", choice.name());
        let spawned = std::thread::Builder::new()
            .name("power".into())
            .spawn(move || {
                if let Err(e) = power::run(choice) {
                    eprintln!("edel-shell-ui: {e}");
                }
            });
        if let Err(e) = spawned {
            eprintln!(
                "edel-shell-ui: {}",
                messages::power_failed(choice.name(), e)
            );
        }
        self.draw();
    }

    /// Draws every configured screen whose view changed.
    fn draw(&mut self) {
        let shown = (self.view(), self.corner_view());
        let mut first = false;
        // Each screen's layout first, from the machine's parts, with the row
        // of people when it shows and the corner's places kept clear.
        let people = self.people_shown();
        let mut laid = Vec::with_capacity(self.screens.len());
        for screen in &self.screens {
            laid.push(screen.size.map(|(w, h)| {
                let (fw, fh) = (w as f32, h as f32);
                let corner_at = corner::layout(fw, fh, &shown.1);
                let at = lock::screen_layout(
                    fw,
                    fh,
                    &self.tokens,
                    false,
                    self.parts.as_deref(),
                    None,
                    people,
                    &corner_at,
                    &mut self.text,
                );
                (at, corner_at)
            }));
        }
        for (screen, laid) in self.screens.iter_mut().zip(laid) {
            let Some((w, h)) = screen.size else {
                continue;
            };
            let Some((at, corner_at)) = laid else {
                continue;
            };
            if screen.drawn.as_ref() == Some(&shown) {
                continue;
            }
            let s = screen.scale;
            let (pw, ph) = (w * s, h * s);
            let Some(mut pixmap) = Pixmap::new(pw, ph) else {
                continue;
            };
            let fw = w as f32;
            lock::paint(
                &mut pixmap,
                &shown.0,
                &at,
                &self.tokens,
                &mut self.text,
                None,
                &mut self.pictures,
                s as f32,
            );
            corner::paint(
                &mut pixmap,
                &shown.1,
                &corner_at,
                &self.tokens,
                &mut self.text,
                s as f32,
            );
            let spots = people::spots(&self.row, fw, at.avatar.1);
            let places = format!(
                "{}, {}, {}",
                people::places(&spots, &self.row),
                lock::places(&at),
                corner::places(&corner_at)
            );
            if places != self.logged {
                eprintln!("edel-shell-ui: greeter places {places}");
                self.logged = places;
            }
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
            screen.drawn = Some(shown.clone());
        }
        if first && self.screens.iter().all(|s| s.drawn.is_some()) {
            eprintln!(
                "edel-shell-ui: greeter shown, {} screens",
                self.screens.len()
            );
        }
    }

    /// A key: typed letters go to the field in use; Return moves on or
    /// logs in; Backspace takes a letter back; Escape closes Power's menu or
    /// goes back a step; left and right choose a person while the field is
    /// empty.
    fn key(&mut self, event: KeyEvent) {
        if self.busy.is_some() {
            return;
        }
        match event.keysym {
            Keysym::Escape if self.menu => return self.set_menu(false),
            Keysym::Escape if self.session_menu => return self.set_session_menu(false),
            Keysym::Left | Keysym::Right if self.field_empty() => {
                let by = if event.keysym == Keysym::Left { -1 } else { 1 };
                self.choose(people::step(&self.row, by));
            }
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
            // A person's password has no step back; Other's goes to its name.
            Keysym::Escape if self.asking_password && self.row.chosen < self.row.people.len() => {
                self.password.clear();
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
        // The chosen session's command, its words as greetd takes them.
        let cmd: Vec<String> = self
            .sessions
            .get(self.session)
            .map_or_else(Vec::new, |s| s.cmd.clone());
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("greetd".into())
            .spawn(move || {
                let login = match greetd::Greetd::connect() {
                    Ok(mut greetd) => {
                        let words = password.text().unwrap_or_default();
                        let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
                        greetd::log_in(&mut greetd, &user, words, &cmd)
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
                self.remember_choice();
                eprintln!(
                    "edel-shell-ui: greeter: session started for {}",
                    self.name.trim()
                );
                if let Some(session) = self.sessions.get(self.session) {
                    eprintln!("edel-shell-ui: greeter: desktop {}", session.id);
                }
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
        // Asked again: a chosen person's password at once, else Other's user
        // name, which stays typed.
        self.password.clear();
        self.asking_password = self.row.chosen < self.row.people.len();
        self.draw();
    }
}

impl Greeter {
    /// A pointer event on a screen `w` by `h` (logical pixels): the corner's
    /// button or row under the pointer lights up, a left press acts.
    fn pointer_event(&mut self, event: &PointerEvent) {
        let Some((w, h)) = self
            .screens
            .iter()
            .find(|s| s.surface.wl_surface() == &event.surface)
            .and_then(|s| s.size)
        else {
            return;
        };
        let (x, y) = (event.position.0 as f32, event.position.1 as f32);
        let (w, h) = (w as f32, h as f32);
        let hit = corner::hit(&corner::layout(w, h, &self.corner_view()), x, y);
        match event.kind {
            PointerEventKind::Press { button, .. } if button == BTN_LEFT => self.press(x, y, w, h),
            PointerEventKind::Leave { .. } => {
                if self.corner_hover.take().is_some() {
                    self.draw();
                }
            }
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. }
                if self.corner_hover != hit =>
            {
                self.corner_hover = hit;
                self.draw();
            }
            _ => {}
        }
    }

    /// A left press: the corner first (Power and the session chip toggle
    /// their menus, a row runs its choice), then with a menu open only a
    /// close, then a person (chosen as the arrows choose), then the go
    /// button, which logs in when the password has letters.
    fn press(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if self.busy.is_some() {
            return;
        }
        let hit = corner::hit(&corner::layout(w, h, &self.corner_view()), x, y);
        match hit {
            Some(corner::Hit::Button(corner::Button::Power)) => return self.set_menu(!self.menu),
            Some(corner::Hit::Button(corner::Button::Session)) => {
                return self.set_session_menu(!self.session_menu);
            }
            Some(corner::Hit::Row(i)) => {
                if let Some(&choice) = power::CHOICES.get(i) {
                    return self.run_power(choice);
                }
            }
            Some(corner::Hit::Session(i)) => return self.choose_session(i),
            _ => {}
        }
        if self.menu {
            return self.set_menu(false);
        }
        if self.session_menu {
            return self.set_session_menu(false);
        }
        let people = self.people_shown();
        let corner_at = corner::layout(w, h, &self.corner_view());
        let at = lock::screen_layout(
            w,
            h,
            &self.tokens,
            false,
            self.parts.as_deref(),
            None,
            people,
            &corner_at,
            &mut self.text,
        );
        if people {
            let spots = people::spots(&self.row, w, at.avatar.1);
            if let Some(index) = people::hit(&spots, x, y) {
                self.choose(index);
                return self.draw();
            }
        }
        if lock::on_go(&at, x, y) && self.password.letters() > 0 {
            self.log_in();
        }
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

    /// The keyboard and the pointer, as soon as the seat has them.
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(e) => eprintln!("edel-shell-ui: no keyboard for the greeter: {e}"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(e) => eprintln!("edel-shell-ui: no pointer for the greeter: {e}"),
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
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
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
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        if self.caps != modifiers.caps_lock {
            self.caps = modifiers.caps_lock;
            self.draw();
        }
    }
}

impl PointerHandler for Greeter {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            self.pointer_event(event);
        }
    }
}

delegate_registry!(Greeter);
smithay_client_toolkit::delegate_dispatch2!(Greeter);
