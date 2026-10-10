//! The shell's side of the status area and quick settings (M5.9a): reading
//! what the machine says on threads of their own and taking their answers
//! back into the event loop, and the card's surface: opened above the
//! status area with the keyboard, closed by Escape, a click outside or a
//! second click on the pill, kept (buffers, keyboard, screen reader) only
//! while it is open. What the card looks like and where its parts lie is
//! `quick.rs`; what it shows is `status.rs`.
//!
//! Commands (`nmcli`, `wpctl`, `bluetoothctl`) never run on the thread that
//! draws: a click changes what the card shows at once, a thread runs the
//! command and reads the machine again, and the card is drawn once more
//! with what it says. Dark style writes the person's settings file, as
//! Settings does, which is quick and which the compositor follows.

use edel::i18n::tr;
use edel::presets::Edge;
use edel::{places, settings};
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::popup::Popup;
use crate::status::{self, Cmd, Msg};
use crate::{MARGIN, QUICK, SETTINGS, Shell, a11y, messages, quick, settings_texts};
use crate::{mpris, paint, widgets};

/// The side of a cover's picture, pixels (M5.9d).
const COVER_PX: u32 = 128;

/// Which slider a drag is moving, if one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Idle,
    Volume,
    /// The brightness slider (M5.9c)
    Light,
}

/// The open card: its surface, the keyboard, what a screen reader reads,
/// and what it keeps besides what the machine says.
pub struct QuickCard {
    popup: Popup<quick::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    reader: a11y::Reader,
    state: quick::State,
    /// Which slider a drag is moving.
    drag: Drag,
    /// The last percent sent to the sound system, and whether a command
    /// setting the volume runs.
    sent: Option<u32>,
    setting_volume: bool,
    /// The same for the screen's brightness (M5.9c).
    sent_light: Option<u32>,
    setting_light: bool,
    /// Where the card's parts were last logged, so a line says it only
    /// when it changed.
    logged: String,
    /// The player's cover: the file it was read from, and the picture made
    /// of it, none when it did not load (so it is not read again). Kept
    /// while the card is open (M5.9d).
    cover: Option<(String, Option<Pixmap>)>,
    /// Follows the players while the card is open (M5.9d). Held only for its
    /// drop: the players are followed while the card lives.
    _follow: Option<mpris::Follow>,
    /// The page shown, by its player's bus name, so it stays on that player
    /// as others come and go; none for the first (M5.9i).
    page: Option<String>,
    /// A scroll or swipe on the player's card, added up into pages.
    scrolled: widgets::Scrolled,
    /// The last player line logged, so it says only what changed.
    player_logged: String,
}

impl QuickCard {
    /// Do not disturb is `on` now: its tile shows it.
    pub fn set_dnd(&mut self, on: bool) {
        self.state.dnd = on;
    }

    /// The sliders show the machine's levels again, not what a person or a
    /// key set last (M5.9c: a volume or brightness key changed them).
    pub fn show_machine_levels(&mut self) {
        self.state.volume = None;
        self.state.brightness = None;
    }

    /// The card's surface, for the handlers of the compositor's events.
    pub fn popup_mut(&mut self) -> &mut Popup<quick::View> {
        &mut self.popup
    }
}

impl Shell {
    // ---- Reading the machine ----

    /// Reads the status afresh on a thread of its own, unless one is
    /// running, in which case it is read once more when that ends; with
    /// Bluetooth when `bluetooth` says so.
    pub fn request_status(&mut self, bluetooth: bool) {
        if !self.status_wanted {
            return;
        }
        if self.status_busy > 0 {
            self.status_again = Some(self.status_again.unwrap_or(false) || bluetooth);
            return;
        }
        self.run_and_read(None, bluetooth);
    }

    /// A thread that runs `ran` if there is one, then reads the status.
    pub(crate) fn run_and_read(&mut self, ran: Option<Cmd>, bluetooth: bool) {
        self.status_busy += 1;
        let tx = self.status_tx.clone();
        let features = edel::places::found_shared(edel::features::DIR);
        let spawned = std::thread::Builder::new()
            .name("status".into())
            .spawn(move || {
                let ran = ran.map(|cmd| {
                    let said = cmd.run().err().map(|e| format!("{e:#}"));
                    (cmd, said)
                });
                let status = Box::new(status::read(&features, bluetooth));
                let _ = tx.send(Msg::Read {
                    status,
                    bluetooth,
                    ran,
                });
            });
        if let Err(e) = spawned {
            self.status_busy -= 1;
            eprintln!(
                "edel-shell-ui: {}",
                messages::quick_not_done("read the status", e)
            );
        }
    }

    /// What a thread that read or ran something, or the system bus, said.
    pub fn status_msg(&mut self, msg: Msg) {
        let Msg::Read {
            status,
            bluetooth,
            ran,
        } = msg
        else {
            return self.request_status(false);
        };
        self.status_busy = self.status_busy.saturating_sub(1);
        if let Some((cmd, said)) = &ran {
            if let Some(said) = said {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::quick_not_done(&cmd.what(), said)
                );
            }
            self.command_done(cmd);
        }
        let status = (*status).merged(&self.live.status, bluetooth);
        let changed = status != self.live.status;
        self.live.status = status;
        if let Some(card) = &mut self.quick {
            // What a person chose has been confirmed or refused by now: the
            // machine's word shows again, unless a drag is still going.
            if card.drag != Drag::Volume && !card.setting_volume {
                card.state.volume = None;
            }
            if card.drag != Drag::Light && !card.setting_light {
                card.state.brightness = None;
            }
        }
        if changed {
            self.draw_all();
        }
        self.draw_quick();
        if self.status_busy == 0 {
            if let Some(bluetooth) = self.status_again.take() {
                self.request_status(bluetooth);
            }
        }
    }

    /// `cmd` ended: what was waiting for it shows the machine's word, and
    /// the volume goes on to where the slider has got to since.
    fn command_done(&mut self, cmd: &Cmd) {
        let Some(card) = &mut self.quick else {
            return;
        };
        match cmd {
            Cmd::Wifi(_) | Cmd::Bluetooth(_) | Cmd::Airplane { .. } => {
                card.state.pending.clear();
            }
            Cmd::Mute(_) => card.state.muted = None,
            Cmd::Volume(_) => {
                card.setting_volume = false;
                self.send_volume();
            }
            Cmd::Brightness(_) => {
                card.setting_light = false;
                self.send_brightness();
            }
            Cmd::Output(_) => {}
        }
    }

    /// Sends the volume the slider is at, unless one is on its way (its
    /// end sends the latest) or the sound system has it already.
    fn send_volume(&mut self) {
        let Some(card) = &mut self.quick else {
            return;
        };
        if card.setting_volume {
            return;
        }
        let Some(percent) = card.state.volume.filter(|p| card.sent != Some(*p)) else {
            return;
        };
        card.sent = Some(percent);
        card.setting_volume = true;
        self.run_and_read(Some(Cmd::Volume(percent)), false);
    }

    /// Sends the brightness the slider is at, one at a time, as the volume
    /// is sent: the end of a command sends the latest.
    fn send_brightness(&mut self) {
        let Some(card) = &mut self.quick else {
            return;
        };
        if card.setting_light {
            return;
        }
        let Some(percent) = card
            .state
            .brightness
            .filter(|p| card.sent_light != Some(*p))
        else {
            return;
        };
        card.sent_light = Some(percent);
        card.setting_light = true;
        self.run_and_read(Some(Cmd::Brightness(percent)), false);
    }

    // ---- The card ----

    /// The width of the screen the panels lie on, logical pixels: a bar
    /// spans it; 0 before any panel is configured.
    pub fn screen_width(&self) -> u32 {
        use edel::presets::Style;
        self.panels
            .iter()
            .filter(|p| p.style == Style::Bar)
            .map(|p| p.width)
            .max()
            .or_else(|| self.panels.iter().map(|p| p.width).max())
            .unwrap_or(0)
    }

    /// A click on the status area: the card opens, or closes if open.
    pub fn toggle_quick(&mut self) {
        if self.quick.is_some() {
            self.close_quick();
        } else {
            self.open_quick();
        }
    }

    /// Opens the card above the status area's panel (below it on a panel
    /// along the top), a sheet across the screen when Compact, with the
    /// keyboard; and reads the machine again, Bluetooth too.
    fn open_quick(&mut self) {
        self.close_launcher();
        self.close_styles();
        self.close_centre();
        self.close_tray_grid();
        // The pop-up's level would show over the card, so it goes.
        self.hide_osd();
        let Some(panel) = self
            .panels
            .iter()
            .find(|p| p.row.all().any(|w| w.name == "status"))
        else {
            return;
        };
        let (edge, scale) = (panel.edge, panel.scale);
        let screen = self.screen_width();
        let compact = screen > 0 && screen < quick::COMPACT_BELOW;
        let width = if compact { screen } else { quick::WIDTH };
        let (machine, person) = settings_texts();
        let state = quick::State {
            width,
            compact,
            dark: quick::is_dark(machine.as_deref(), person.as_deref()),
            dnd: crate::notify::do_not_disturb(machine.as_deref(), person.as_deref()),
            ..quick::State::default()
        };
        let view = self.card_view(&state);
        let size = quick::layout(&view).size;
        // Room for the list of outputs to open without a bigger pool.
        let most = quick::most_size(size);
        let room = paint::shadow_room(&self.tokens, !crate::fillets());
        let Some(popup) = Popup::new(self, QUICK, size, most, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let surface = &popup.surface;
        let m = MARGIN - room as i32;
        if compact {
            // Across the screen, its shadow's room past both sides.
            surface.set_anchor(side | Anchor::LEFT | Anchor::RIGHT);
            surface.set_margin(m, -(room as i32), m, -(room as i32));
        } else {
            // At the screen's side, the card keeping its place.
            surface.set_anchor(side | Anchor::RIGHT);
            surface.set_margin(m, m, m, m);
        }
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.quick_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for quick settings: {e}"))
                .ok()
        });
        let follow = self
            ._portal
            .as_ref()
            .zip(self.player_tx.clone())
            .map(|(connection, events)| mpris::follow(connection, events));
        self.quick = Some(QuickCard {
            popup,
            keyboard,
            reader: a11y::Reader::new(tr("Quick settings")),
            state,
            drag: Drag::Idle,
            sent: None,
            setting_volume: false,
            sent_light: None,
            setting_light: false,
            logged: String::new(),
            cover: None,
            page: None,
            scrolled: widgets::Scrolled::default(),
            player_logged: String::new(),
            _follow: follow,
        });
        // The pill lights while the card is open.
        self.live.quick = true;
        self.draw_all();
        self.request_status(true);
    }

    /// Closes the card and lets go of its keyboard, buffers and reader.
    pub fn close_quick(&mut self) {
        let Some(card) = self.quick.take() else {
            return;
        };
        if let Some(keyboard) = &card.keyboard {
            keyboard.release();
        }
        drop(card);
        self.live.quick = false;
        self.live.players.clear();
        self.draw_all();
        eprintln!("edel-shell-ui: quick settings hidden");
    }

    /// What plays changed while quick settings is open (M5.9d). A change
    /// that comes after the card closed is dropped: nothing is kept then.
    /// The page shown stays on its player while it is there (M5.9i).
    pub fn player_changed(&mut self, event: mpris::Event) {
        if self.quick.is_none() {
            return;
        }
        let mpris::Event::Players(players) = event;
        if players != self.live.players && players.len() > 1 {
            let names: Vec<&str> = players.iter().map(|p| p.identity.as_str()).collect();
            eprintln!(
                "edel-shell-ui: players: {} ({})",
                players.len(),
                names.join(", ")
            );
        }
        self.live.players = players;
        self.show_player_page(None);
    }

    /// The page of the player shown now: the one the card keeps (by its bus
    /// name) while it is there, else the first (M5.9i).
    fn player_page(&self) -> usize {
        let held = self.quick.as_ref().and_then(|c| c.page.as_deref());
        held.and_then(|bus| self.live.players.iter().position(|p| p.bus == bus))
            .unwrap_or(0)
    }

    /// Shows page `to` (or keeps the one shown when none), logs the player
    /// shown when it changes, and draws.
    fn show_player_page(&mut self, to: Option<usize>) {
        let page = to
            .filter(|i| *i < self.live.players.len())
            .unwrap_or_else(|| self.player_page());
        let shown = self.live.players.get(page).cloned();
        let Some(card) = &mut self.quick else {
            return;
        };
        let line = match &shown {
            Some(p) => format!(
                "player: {} by {} ({}), {}{}",
                p.title,
                p.artist,
                p.identity,
                if p.playing { "playing" } else { "paused" },
                if self.live.players.len() > 1 {
                    format!(", page {} of {}", page + 1, self.live.players.len())
                } else {
                    String::new()
                }
            ),
            None => "player: none".to_string(),
        };
        if card.player_logged != line {
            eprintln!("edel-shell-ui: {line}");
            card.player_logged = line;
        }
        card.page = shown.map(|p| p.bus);
        self.draw_quick();
    }

    /// The player on the page shown, if any.
    fn shown_player(&self) -> Option<&mpris::Player> {
        self.live.players.get(self.player_page())
    }

    /// `steps` of a scroll or swipe on the player's card, right or down
    /// towards the later pages: a page a step, the end pages stopping it
    /// (M5.9i).
    fn swipe_players(&mut self, steps: i32) {
        let count = self.live.players.len() as i64;
        let page = self.player_page() as i64;
        if count < 2 || steps == 0 {
            return;
        }
        let to = (page + i64::from(steps)).clamp(0, count - 1) as usize;
        if to as i64 != page {
            self.show_player_page(Some(to));
        }
    }

    /// The shown player's app icon at `px` pixels: its desktop file's icon,
    /// else an icon named as its desktop file or its app (M5.9i).
    fn player_icon(&mut self, px: u32) -> Option<Pixmap> {
        let player = self.shown_player()?;
        let installed = self
            .live
            .installed
            .iter()
            .find(|pin| !player.desktop.is_empty() && pin.id.eq_ignore_ascii_case(&player.desktop))
            .map(|pin| pin.icon.clone());
        let names = [
            installed.unwrap_or_default(),
            player.desktop.clone(),
            player.identity.to_lowercase().replace(' ', "-"),
        ];
        names
            .iter()
            .filter(|name| !name.is_empty())
            .find_map(|name| self.icons.get(name, px).cloned())
    }

    pub fn is_quick(&self, surface: &wl_surface::WlSurface) -> bool {
        self.quick.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// The card with `state`, what the machine says, and the player (M5.9d)
    /// with its cover still to be decoded; the card hangs below the player
    /// when the status area's panel lies along the top.
    fn card_view(&self, state: &quick::State) -> quick::View {
        let mut view = quick::view(
            state,
            &self.live.status,
            &self.quick_tiles,
            self.quick_settings,
        );
        view.pages = self
            .live
            .players
            .iter()
            .map(|p| p.identity.clone())
            .collect();
        view.page = self.player_page();
        view.player = self.shown_player().map(|p| quick::PlayerView {
            title: p.title.clone(),
            artist: p.artist.clone(),
            identity: p.identity.clone(),
            playing: p.playing,
            can_previous: p.can_previous,
            can_next: p.can_next,
            cover: None,
            app_icon: None,
        });
        view.player_below = self
            .panels
            .iter()
            .any(|p| p.row.all().any(|w| w.name == "status") && p.edge == Edge::Top);
        view
    }

    /// The player's cover as a picture, read from its file once and kept
    /// while the card is open; a file that is not a PNG, or does not load,
    /// gives none (M5.9d).
    fn player_cover(&mut self) -> Option<Pixmap> {
        let path = self.shown_player().and_then(|p| p.cover.clone());
        let card = self.quick.as_mut()?;
        let Some(path) = path else {
            card.cover = None;
            return None;
        };
        if card.cover.as_ref().is_none_or(|(held, _)| *held != path) {
            let picture = load_cover(&path);
            card.cover = Some((path, picture));
        }
        card.cover.as_ref().and_then(|(_, picture)| picture.clone())
    }

    /// What the card shows now and where it lies.
    fn quick_view(&mut self) -> Option<(quick::View, quick::Layout)> {
        let cover = self.player_cover();
        let scale = self.quick.as_ref()?.popup.scale();
        let icon = self.player_icon((quick::APP_ICON * scale).round() as u32);
        let card = self.quick.as_ref()?;
        let mut view = self.card_view(&card.state);
        if let Some(player) = &mut view.player {
            player.cover = cover;
            player.app_icon = icon;
        }
        let layout = quick::layout(&view);
        Some((view, layout))
    }

    /// Draws the card if what it shows changed, after asking the
    /// compositor for a new size if it needs one.
    pub fn draw_quick(&mut self) {
        let Some((view, layout)) = self.quick_view() else {
            return;
        };
        let Some(card) = &mut self.quick else {
            return;
        };
        if card.popup.size() != layout.size {
            card.popup.resize(layout.size, &self.compositor);
            return;
        }
        // The card is the rounded shape the mockups draw, its shadow round it.
        card.popup
            .set_cards(quick::cards(&layout), &self.compositor);
        let Some(mut pixmap) = card.popup.canvas(&view) else {
            return;
        };
        let scale = card.popup.scale();
        quick::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let first = card.popup.show(
            view.clone(),
            &pixmap,
            &self.tokens,
            "quick settings",
            &self.qh,
        );
        let places = quick::places(&view, &layout);
        if first {
            eprintln!(
                "edel-shell-ui: quick settings shown, {} tiles",
                view.tiles.len()
            );
        }
        if places != card.logged {
            eprintln!("edel-shell-ui: quick places {places}");
            card.logged = places;
        }
        let (items, focused) = quick::items(&view, &layout, (0.0, 0.0));
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        card.reader.update_focused(size, items, focused);
    }

    // ---- What a person does ----

    /// A key while the card is open.
    pub fn quick_key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Escape => quick::Key::Escape,
            Keysym::Tab => quick::Key::Tab(false),
            Keysym::ISO_Left_Tab => quick::Key::Tab(true),
            Keysym::Left => quick::Key::Left,
            Keysym::Right => quick::Key::Right,
            Keysym::Up => quick::Key::Up,
            Keysym::Down => quick::Key::Down,
            Keysym::Home => quick::Key::Home,
            Keysym::End => quick::Key::End,
            Keysym::Return | Keysym::KP_Enter | Keysym::space => quick::Key::Activate,
            _ => return,
        };
        let Some((view, _)) = self.quick_view() else {
            return;
        };
        let Some(card) = &mut self.quick else {
            return;
        };
        let (focus, act) = quick::key(&view, card.state.focus, key);
        card.state.focus = focus;
        match act {
            Some(act) => self.quick_act(act),
            None => self.draw_quick(),
        }
    }

    /// The pointer on the card: it lights what it is over, a press does
    /// what the part does, and a drag on the slider sets the volume.
    pub fn quick_pointer(&mut self, event: &PointerEvent) {
        let Some((view, layout)) = self.quick_view() else {
            return;
        };
        let Some(card) = &mut self.quick else {
            return;
        };
        // From the card's corner, inside the shadow's room.
        let room = card.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let over = quick::hit(&view, &layout, x, y);
        match &event.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                card.state.hover = over;
                match card.drag {
                    Drag::Volume => self.quick_volume(quick::volume_at(&layout, x)),
                    Drag::Light => self.quick_brightness(quick::brightness_at(&layout, x)),
                    Drag::Idle => self.draw_quick(),
                }
            }
            // A scroll or swipe over the player's card turns its pages,
            // sideways or with a wheel (M5.9i).
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } if layout.player.is_some_and(|p| p.contains(x, y)) => {
                let n = card.scrolled.steps(
                    vertical.value120 + horizontal.value120,
                    vertical.discrete + horizontal.discrete,
                    vertical.absolute + horizontal.absolute,
                );
                self.swipe_players(n);
            }
            PointerEventKind::Leave { .. } => {
                card.scrolled.reset();
                card.state.hover = None;
                if card.drag == Drag::Idle {
                    self.draw_quick();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(quick::Focus::Slider) => {
                    card.drag = Drag::Volume;
                    let percent = quick::volume_at(&layout, x);
                    self.quick_volume(percent);
                }
                Some(quick::Focus::Light) => {
                    card.drag = Drag::Light;
                    let percent = quick::brightness_at(&layout, x);
                    self.quick_brightness(percent);
                }
                Some(part) => {
                    card.state.focus = None;
                    let act = match part {
                        quick::Focus::Toggle(i) => quick::Act::Toggle(i),
                        quick::Focus::Page(i) => quick::Act::Page(i),
                        quick::Focus::Sound => quick::Act::List,
                        quick::Focus::Choose(i) => quick::Act::Choose(i),
                        quick::Focus::SoundPage => quick::Act::SoundPage,
                        quick::Focus::Settings => quick::Act::Settings,
                        quick::Focus::Previous => quick::Act::Player("Previous"),
                        quick::Focus::PlayPause => quick::Act::Player("PlayPause"),
                        quick::Focus::Next => quick::Act::Player("Next"),
                        quick::Focus::Dot(i) => quick::Act::PlayerPage(i),
                        quick::Focus::Slider | quick::Focus::Light => return,
                    };
                    self.quick_act(act);
                }
                None => {}
            },
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => {
                let drag = std::mem::replace(&mut card.drag, Drag::Idle);
                match drag {
                    Drag::Volume => self.send_volume(),
                    Drag::Light => self.send_brightness(),
                    Drag::Idle => {}
                }
            }
            // A right click opens the page: the slider's Sound settings, a
            // round toggle's Settings page (the mockups' rule).
            PointerEventKind::Press { button, .. } if *button == BTN_RIGHT => {
                let act = match over {
                    Some(quick::Focus::Slider) => Some(quick::Act::SoundPage),
                    Some(quick::Focus::Light) => Some(quick::Act::DisplaysPage),
                    Some(quick::Focus::Toggle(i)) => view
                        .tiles
                        .get(i)
                        .filter(|t| !t.tile.pill() && t.tile.page().is_some())
                        .map(|_| quick::Act::Page(i)),
                    _ => None,
                };
                if let Some(act) = act {
                    card.state.focus = None;
                    self.quick_act(act);
                }
            }
            _ => {}
        }
    }

    /// The slider is at `percent`: shown at once, and sent.
    fn quick_volume(&mut self, percent: u32) {
        let Some(card) = &mut self.quick else {
            return;
        };
        if card.state.volume != Some(percent) {
            card.state.volume = Some(percent);
            self.send_volume();
        }
        self.draw_quick();
    }

    /// The brightness slider is at `percent`: shown at once, and sent (M5.9c).
    fn quick_brightness(&mut self, percent: u32) {
        let Some(card) = &mut self.quick else {
            return;
        };
        if card.state.brightness != Some(percent) {
            card.state.brightness = Some(percent);
            self.send_brightness();
        }
        self.draw_quick();
    }

    /// Does what a click or a key on the card asked.
    fn quick_act(&mut self, act: quick::Act) {
        use quick::Act;
        let Some((view, _)) = self.quick_view() else {
            return;
        };
        match act {
            Act::Close => self.close_quick(),
            Act::List => {
                if let Some(card) = &mut self.quick {
                    card.state.list = !card.state.list;
                    // A list of the outputs as they are now.
                    card.state.focus = card.state.focus.filter(|_| !card.state.list);
                }
                self.request_status(false);
                self.draw_quick();
            }
            Act::Choose(i) => {
                let id = self.live.status.outputs.get(i).map(|o| o.id);
                if let Some(card) = &mut self.quick {
                    card.state.list = false;
                    card.state.focus = Some(quick::Focus::Sound);
                }
                if let Some(id) = id {
                    self.run_and_read(Some(Cmd::Output(id)), false);
                }
                self.draw_quick();
            }
            Act::Mute => {
                let muted = view.volume.as_ref().is_some_and(|v| v.muted);
                if let Some(card) = &mut self.quick {
                    card.state.muted = Some(!muted);
                }
                self.run_and_read(Some(Cmd::Mute(!muted)), false);
                self.draw_quick();
            }
            Act::Volume(percent) => self.quick_volume(percent),
            Act::Brightness(percent) => self.quick_brightness(percent),
            Act::Toggle(i) => {
                let Some(tile) = view.tiles.get(i) else {
                    return;
                };
                let target = !tile.on;
                self.toggle_tile(tile.tile, target);
            }
            Act::Page(i) => {
                let Some(page) = view.tiles.get(i).and_then(|t| t.tile.page()) else {
                    return;
                };
                let argv = [SETTINGS.to_string(), "--page".to_string(), page.to_string()];
                self.launcher.spawn(&argv, "Settings");
                self.close_quick();
            }
            Act::SoundPage => {
                let argv = [
                    SETTINGS.to_string(),
                    "--page".to_string(),
                    quick::SOUND_PAGE.to_string(),
                ];
                self.launcher.spawn(&argv, "Settings");
                self.close_quick();
            }
            Act::DisplaysPage => {
                let argv = [
                    SETTINGS.to_string(),
                    "--page".to_string(),
                    quick::DISPLAYS_PAGE.to_string(),
                ];
                self.launcher.spawn(&argv, "Settings");
                self.close_quick();
            }
            Act::Settings => {
                self.launcher.spawn(&[SETTINGS.to_string()], "Settings");
                self.close_quick();
            }
            // The player's change comes back through `player_changed`, and
            // the card stays open.
            Act::Player(method) => {
                if let (Some(player), Some(connection)) = (self.shown_player(), &self._portal) {
                    mpris::call(connection, &player.bus, method);
                }
            }
            Act::PlayerPage(i) => self.show_player_page(Some(i)),
        }
    }

    /// Switches `tile` to `on`: shown at once, then done by a thread.
    fn toggle_tile(&mut self, tile: quick::Tile, on: bool) {
        use quick::Tile;
        match tile {
            // Lines of the person's settings file, written as Settings does.
            Tile::DarkStyle => return self.toggle_dark(on),
            Tile::DoNotDisturb => return self.set_dnd(on),
            _ => {}
        }
        let status = &self.live.status;
        let (wifi, bluetooth) = (status.has_wifi(), status.bluetooth.is_some());
        let Some(card) = &mut self.quick else {
            return;
        };
        let cmd = match tile {
            Tile::Wifi => {
                card.state.pending.push((Tile::Wifi, on));
                Cmd::Wifi(on)
            }
            Tile::Bluetooth => {
                card.state.pending.push((Tile::Bluetooth, on));
                Cmd::Bluetooth(on)
            }
            Tile::Airplane => {
                // Flight mode is every radio off: the tiles of the radios
                // show it with it.
                card.state.pending.push((Tile::Airplane, on));
                if wifi {
                    card.state.pending.push((Tile::Wifi, !on));
                }
                if bluetooth {
                    card.state.pending.push((Tile::Bluetooth, !on));
                }
                Cmd::Airplane {
                    on,
                    wifi,
                    bluetooth,
                }
            }
            Tile::DarkStyle | Tile::DoNotDisturb => return,
        };
        self.run_and_read(Some(cmd), true);
        self.draw_quick();
    }

    /// Dark style: `appearance.mode` in the person's settings file, as
    /// Settings writes it (the key is taken out when what applies without
    /// it is already what was asked); the compositor follows the file and
    /// starts shell-ui again in the new colours.
    fn toggle_dark(&mut self, dark: bool) {
        let (machine, _) = settings_texts();
        let value = quick::mode_to_write(machine.as_deref(), dark);
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => match settings::write(&path, quick::MODE, value) {
                Ok(()) => {
                    eprintln!(
                        "edel-shell-ui: dark style {}",
                        if dark { "on" } else { "off" }
                    );
                    if let Some(card) = &mut self.quick {
                        card.state.dark = dark;
                    }
                }
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::dark_style_not_kept(format!("{e:#}"))
                ),
            },
            None => eprintln!("edel-shell-ui: {}", messages::DARK_STYLE_NO_HOME),
        }
        self.draw_quick();
    }
}

/// The cover at `path` as a square picture of `COVER_PX` pixels, when it is
/// a PNG the app gave as a local file (M5.9d). Anything else, JPEG included,
/// and a file that does not load give None, quietly: the card then shows the
/// app's initial.
fn load_cover(path: &str) -> Option<Pixmap> {
    let source = Pixmap::load_png(path).ok()?;
    let mut square = Pixmap::new(COVER_PX, COVER_PX)?;
    // Cropped from its middle to a square, never stretched, as album art
    // is not always square.
    let (w, h) = (source.width() as f32, source.height() as f32);
    let k = COVER_PX as f32 / w.min(h);
    let scale = Transform::from_scale(k, k).post_translate(
        (COVER_PX as f32 - w * k) / 2.0,
        (COVER_PX as f32 - h * k) / 2.0,
    );
    let smooth = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    square.draw_pixmap(0, 0, source.as_ref(), &smooth, scale, None);
    Some(square)
}
