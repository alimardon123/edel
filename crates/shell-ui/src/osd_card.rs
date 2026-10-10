//! The shell's side of the volume and brightness pop-up (M5.9c). A volume
//! or brightness key sets the level one step: it is shown at once in the
//! live status, a thread tells the machine (`quick_card.rs`'s
//! `run_and_read`), and the pop-up shows it, made when it is not up and
//! let go 1.5 s after the last key (a timer on the event loop, not a
//! thread). Quick settings, when it is open, shows the level in its own
//! sliders instead. The pop-up never takes the keyboard, and the pointer
//! holds it while it is over it. What it looks like is `osd.rs`.

use edel::i18n::tr;
use edel::presets::Edge;
use smithay_client_toolkit::reexports::calloop::RegistrationToken;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};
use std::time::Duration;

use crate::osd::{self, Kind, Part};
use crate::paint;
use crate::popup::Popup;
use crate::status::Cmd;
use crate::{MARGIN, OSD, SETTINGS, Shell, a11y, quick};

/// The open pop-up: its surface, what it shows, what a screen reader
/// reads, and the timer that ends it.
pub struct OsdCard {
    pub popup: Popup<osd::View>,
    pub view: osd::View,
    pub timer: Option<RegistrationToken>,
    /// Where the parts were last logged, so a line says it only when it
    /// changed.
    pub logged: String,
    pub reader: a11y::Reader,
}

impl Shell {
    // ---- What the keys do ----

    /// A volume or brightness key the compositor sent: `key` is one of
    /// `volume_up`, `volume_down`, `mute`, `brightness_up` or
    /// `brightness_down`. The level moves one step, is shown and sent.
    pub fn media_key(&mut self, key: &str) {
        match key {
            "volume_up" | "volume_down" | "mute" => {
                let (percent, muted) = match &self.live.status.volume {
                    Some(v) => (v.percent, v.muted),
                    None => {
                        eprintln!("edel-shell-ui: media key {key}, but there is no sound output");
                        return;
                    }
                };
                match key {
                    "volume_up" => self.set_volume((percent + quick::STEP).min(100)),
                    "volume_down" => self.set_volume(percent.saturating_sub(quick::STEP)),
                    _ => self.set_mute(!muted),
                }
            }
            "brightness_up" | "brightness_down" => {
                let Some(now) = self.live.status.brightness else {
                    eprintln!("edel-shell-ui: media key {key}, but this screen has no backlight");
                    return;
                };
                let to = if key == "brightness_up" {
                    (now + quick::STEP).min(100)
                } else {
                    now.saturating_sub(quick::STEP)
                };
                self.set_brightness(to);
            }
            _ => eprintln!("edel-shell-ui: media key {key} is not one it knows"),
        }
    }

    /// The volume is `percent` now: shown at once and sent by a thread.
    /// Above silence the sound is heard, as the machine does it.
    fn set_volume(&mut self, percent: u32) {
        if let Some(volume) = &mut self.live.status.volume {
            volume.percent = percent;
            if percent > 0 {
                volume.muted = false;
            }
        }
        self.run_and_read(Some(Cmd::Volume(percent)), false);
        self.draw_all();
        self.show_media(Kind::Volume);
    }

    /// The sound is muted or not now: shown at once and sent.
    fn set_mute(&mut self, on: bool) {
        if let Some(volume) = &mut self.live.status.volume {
            volume.muted = on;
        }
        self.run_and_read(Some(Cmd::Mute(on)), false);
        self.draw_all();
        self.show_media(Kind::Volume);
    }

    /// The screen's brightness is `percent` now: shown at once and sent.
    fn set_brightness(&mut self, percent: u32) {
        self.live.status.brightness = Some(percent);
        self.run_and_read(Some(Cmd::Brightness(percent)), false);
        self.draw_all();
        self.show_media(Kind::Brightness);
    }

    /// Where a level a key or a press set is shown: quick settings' sliders
    /// show the machine's level when it is open, else the pop-up does.
    fn show_media(&mut self, kind: Kind) {
        if let Some(card) = &mut self.quick {
            card.show_machine_levels();
            return self.draw_quick();
        }
        self.show_osd(kind);
    }

    // ---- The pop-up ----

    /// The pop-up's view of `kind` from the live status.
    fn osd_view(&self, kind: Kind) -> osd::View {
        let screen = self.screen_width();
        let (percent, muted) = match kind {
            Kind::Volume => self
                .live
                .status
                .volume
                .as_ref()
                .map_or((0, false), |v| (v.percent, v.muted)),
            Kind::Brightness => (self.live.status.brightness.unwrap_or(0), false),
        };
        osd::View {
            kind,
            percent,
            muted,
            compact: screen > 0 && screen < quick::COMPACT_BELOW,
            hover: None,
        }
    }

    /// Shows the pop-up with `kind`'s level: its surface is made when it is
    /// not up, else what it shows changes, and its timer starts again.
    fn show_osd(&mut self, kind: Kind) {
        let mut view = self.osd_view(kind);
        let layout = osd::layout(&view);
        if let Some(card) = &mut self.osd {
            view.hover = card.view.hover;
            card.popup.set_cards(osd::cards(&layout), &self.compositor);
            card.popup.resize(layout.size, &self.compositor);
            card.view = view;
            self.restart_osd_timer();
            return self.draw_osd();
        }
        let (edge, scale) = self.panel_for("status");
        let room = paint::shadow_room(&self.tokens, !crate::fillets());
        let Some(mut popup) = Popup::new(self, OSD, layout.size, layout.size, scale, room) else {
            return;
        };
        popup.set_cards(osd::cards(&layout), &self.compositor);
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let m = MARGIN - room as i32;
        popup.surface.set_anchor(side | Anchor::RIGHT);
        popup.surface.set_margin(m, m, m, m);
        // A pop-up never takes the keyboard from what a person is typing in.
        popup
            .surface
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        popup.surface.commit();
        self.osd = Some(OsdCard {
            popup,
            view,
            timer: None,
            logged: String::new(),
            reader: a11y::Reader::new(tr("Volume and brightness")),
        });
        self.restart_osd_timer();
        self.draw_osd();
    }

    /// A timer that ends the pop-up `osd::MS` from now.
    fn osd_timer(&mut self) -> Option<RegistrationToken> {
        self.handle
            .insert_source(
                Timer::from_duration(Duration::from_millis(osd::MS)),
                |_, _, shell: &mut Shell| {
                    if let Some(card) = &mut shell.osd {
                        card.timer = None;
                    }
                    shell.hide_osd();
                    TimeoutAction::Drop
                },
            )
            .inspect_err(|e| eprintln!("edel-shell-ui: no timer for the pop-up: {e}"))
            .ok()
    }

    /// The pop-up's timer starts again: it ends `osd::MS` from now. Nothing
    /// happens while no pop-up is up.
    fn restart_osd_timer(&mut self) {
        if self.osd.is_none() {
            return;
        }
        if let Some(token) = self.osd.as_mut().and_then(|c| c.timer.take()) {
            self.handle.remove(token);
        }
        let timer = self.osd_timer();
        if let Some(card) = &mut self.osd {
            card.timer = timer;
        }
    }

    /// Closes the pop-up and lets go of its buffers and reader.
    pub fn hide_osd(&mut self) {
        let Some(card) = self.osd.take() else {
            return;
        };
        if let Some(token) = card.timer {
            self.handle.remove(token);
        }
        drop(card);
        eprintln!("edel-shell-ui: osd hidden");
    }

    /// Whether `surface` is the open pop-up's.
    pub fn is_osd(&self, surface: &wl_surface::WlSurface) -> bool {
        self.osd.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// Draws the pop-up if what it shows changed.
    pub fn draw_osd(&mut self) {
        let Some(card) = &mut self.osd else {
            return;
        };
        let Some(mut pixmap) = card.popup.canvas(&card.view) else {
            return;
        };
        let scale = card.popup.scale();
        osd::paint(
            &mut pixmap,
            &card.view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let first = card
            .popup
            .show(card.view.clone(), &pixmap, &self.tokens, "pop-up", &self.qh);
        let layout = osd::layout(&card.view);
        if first {
            let what = match card.view.kind {
                Kind::Volume => format!("volume {}", card.view.percent),
                Kind::Brightness => format!("brightness {}", card.view.percent),
            };
            let muted = if card.view.muted { ", muted" } else { "" };
            eprintln!("edel-shell-ui: osd shown, {what}{muted}");
        }
        let places = osd::places(&layout);
        if places != card.logged {
            eprintln!("edel-shell-ui: osd places {places}");
            card.logged = places;
        }
        let nodes = osd::nodes(&card.view, &layout);
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        card.reader.update(size, nodes);
    }

    /// The pointer on the pop-up: it holds the pop-up while it is over it
    /// and lights its button; a press on the slider sets the level where
    /// it is, as a key does; a press on the button opens the page for the
    /// kind in Settings and the pop-up goes.
    pub fn osd_pointer(&mut self, event: &PointerEvent) {
        let Some(card) = &self.osd else {
            return;
        };
        let room = card.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let layout = osd::layout(&card.view);
        let over = osd::hit(&layout, x, y);
        let kind = card.view.kind;
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                // Held while the pointer is over it.
                if let Some(token) = self.osd.as_mut().and_then(|c| c.timer.take()) {
                    self.handle.remove(token);
                }
                if let Some(card) = &mut self.osd {
                    card.view.hover = over;
                }
                self.draw_osd();
            }
            PointerEventKind::Leave { .. } => {
                if let Some(card) = &mut self.osd {
                    card.view.hover = None;
                }
                self.restart_osd_timer();
                self.draw_osd();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Part::Track) => match kind {
                    Kind::Volume => self.set_volume(osd::level_at(&layout, x)),
                    Kind::Brightness => self.set_brightness(osd::level_at(&layout, x)),
                },
                Some(Part::More) => {
                    let argv = [
                        SETTINGS.to_string(),
                        "--page".to_string(),
                        osd::page(kind).to_string(),
                    ];
                    self.launcher.spawn(&argv, "Settings");
                    self.hide_osd();
                }
                None => {}
            },
            _ => {}
        }
    }
}
