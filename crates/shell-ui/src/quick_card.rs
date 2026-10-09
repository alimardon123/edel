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
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::paint::{self, Face};
use crate::popup::Popup;
use crate::status::{self, Cmd, Msg};
use crate::{MARGIN, QUICK, SETTINGS, Shell, a11y, messages, quick, settings_texts};

/// The open card: its surface, the keyboard, what a screen reader reads,
/// and what it keeps besides what the machine says.
pub struct QuickCard {
    popup: Popup<quick::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    reader: a11y::Reader,
    state: quick::State,
    /// A drag on the slider is going on.
    dragging: bool,
    /// The last percent sent to the sound system, and whether a command
    /// setting the volume runs.
    sent: Option<u32>,
    setting_volume: bool,
    /// Where the card's parts were last logged, so a line says it only
    /// when it changed.
    logged: String,
}

impl QuickCard {
    /// Do not disturb is `on` now: its tile shows it.
    pub fn set_dnd(&mut self, on: bool) {
        self.state.dnd = on;
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
    fn run_and_read(&mut self, ran: Option<Cmd>, bluetooth: bool) {
        self.status_busy += 1;
        let tx = self.status_tx.clone();
        let features = std::path::PathBuf::from(edel::features::DIR);
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
            if !card.dragging && !card.setting_volume {
                card.state.volume = None;
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
        let size_px = self.tokens.panel_text_size as f32 - 1.0;
        let text = &mut self.text;
        let view = quick::view(
            &state,
            &self.live.status,
            &self.quick_tiles,
            self.quick_settings,
            |name| {
                quick::chip_width(
                    text.line_in(name, size_px, Face::MEDIUM).width,
                    quick::CHIP_MOST,
                )
            },
        );
        let size = quick::layout(&view).size;
        // Room for the list of outputs to open without a bigger pool.
        let most = (size.0, size.1 + quick::MOST_OUTPUTS as u32 * 44);
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
        self.quick = Some(QuickCard {
            popup,
            keyboard,
            reader: a11y::Reader::new(tr("Quick settings")),
            state,
            dragging: false,
            sent: None,
            setting_volume: false,
            logged: String::new(),
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
        self.draw_all();
        eprintln!("edel-shell-ui: quick settings hidden");
    }

    pub fn is_quick(&self, surface: &wl_surface::WlSurface) -> bool {
        self.quick.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// What the card shows now and where it lies.
    fn quick_view(&mut self) -> Option<(quick::View, quick::Layout)> {
        let card = self.quick.as_ref()?;
        let size = self.tokens.panel_text_size as f32 - 1.0;
        let text = &mut self.text;
        let view = quick::view(
            &card.state,
            &self.live.status,
            &self.quick_tiles,
            self.quick_settings,
            |name| {
                quick::chip_width(
                    text.line_in(name, size, Face::MEDIUM).width,
                    quick::CHIP_MOST,
                )
            },
        );
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
        let over = quick::hit(&layout, x, y);
        let compact = view.compact;
        match &event.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                card.state.hover = over;
                if card.dragging {
                    let percent = quick::volume_at(&layout, compact, x);
                    self.quick_volume(percent);
                } else {
                    self.draw_quick();
                }
            }
            PointerEventKind::Leave { .. } => {
                card.state.hover = None;
                if !card.dragging {
                    self.draw_quick();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(quick::Focus::Slider) => {
                    card.dragging = true;
                    let percent = quick::volume_at(&layout, compact, x);
                    self.quick_volume(percent);
                }
                Some(part) => {
                    card.state.focus = None;
                    let act = match part {
                        quick::Focus::Toggle(i) => quick::Act::Toggle(i),
                        quick::Focus::Page(i) => quick::Act::Page(i),
                        quick::Focus::Output => quick::Act::List,
                        quick::Focus::Choose(i) => quick::Act::Choose(i),
                        quick::Focus::Mute => quick::Act::Mute,
                        quick::Focus::Settings => quick::Act::Settings,
                        quick::Focus::Slider => return,
                    };
                    self.quick_act(act);
                }
                None => {}
            },
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => {
                card.dragging = false;
                self.send_volume();
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
                    card.state.focus = Some(quick::Focus::Output);
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
            Act::Settings => {
                self.launcher.spawn(&[SETTINGS.to_string()], "Settings");
                self.close_quick();
            }
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
