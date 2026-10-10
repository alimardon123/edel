//! The shell's side of Edit panels (M5.31b): `editor.rs` lays the drawer
//! out and draws it; this opens it from the panel menu, holds its surface
//! and keyboard, keeps the panels' original and changed lines, and writes
//! them when Done is pressed. While it is open the panels show their
//! widgets as tiles (`paint::Look::editing`), and a press on a panel does
//! nothing: dragging comes in the editor's second part.

use edel::i18n::tr;
use edel::panel_edit::{self, PANELS};
use edel::places;
use edel::presets::{self, Edge, Style};
use edel::settings;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::a11y;
use crate::editor::{self, Act, Part, Step, Tile, View};
use crate::messages;
use crate::popup::Popup;
use crate::quick;
use crate::trayview::Key;
use crate::widgets;
use crate::{DOCK_MARGIN, EDITOR, MARGIN, Shell, fillets, paint, settings_texts};

/// The open drawer: its surface and what it shows, the keyboard, what a
/// screen reader reads, the last places logged, and the panels it started
/// from (`original`) and what they are now (`now`), as the editor changes
/// them.
pub struct Editor {
    pub popup: Popup<View>,
    pub view: View,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub reader: a11y::Reader,
    pub logged: String,
    pub original: Vec<presets::Panel>,
    pub now: Vec<presets::Panel>,
}

/// Marks each tile placed when a panel in `now` holds its widget.
fn mark_placed(view: &mut View, now: &[presets::Panel]) {
    for tile in &mut view.tiles {
        tile.placed = panel_edit::find(now, tile.name).is_some();
    }
}

/// The keyboard key the drawer takes for `sym`, if any.
fn key_of(sym: Keysym) -> Option<Key> {
    Some(match sym {
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
        Keysym::Tab => Key::Tab,
        Keysym::Return | Keysym::KP_Enter | Keysym::space => Key::Activate,
        Keysym::Escape => Key::Escape,
        _ => return None,
    })
}

impl Shell {
    /// Opens Edit panels (M5.31b): the drawer centred above the bottom panel
    /// (below the top one when there is no bottom panel), the keyboard
    /// exclusive, and every panel showing its widgets as tiles.
    pub fn open_editor(&mut self) {
        if self.editor.is_some() {
            return;
        }
        self.close_popups();
        // Above the bottom panel when there is one, else below the top one.
        let Some(i) = self
            .panels
            .iter()
            .position(|p| p.edge == Edge::Bottom)
            .or_else(|| self.panels.iter().position(|p| p.edge == Edge::Top))
        else {
            return;
        };
        let (edge, style, scale) = {
            let panel = &self.panels[i];
            (panel.edge, panel.style, panel.scale)
        };
        // Every widget the release has that this machine can show.
        let features = places::found_shared(edel::features::DIR);
        let names: Vec<String> = widgets::TABLE.iter().map(|w| w.name.to_string()).collect();
        let (usable, _) = widgets::usable(&names, &features);
        let original = self.panel_specs.clone();
        let now = original.clone();
        let width = self.screen_width();
        let compact = width > 0 && width < quick::COMPACT_BELOW;
        let mut view = View {
            tiles: usable
                .iter()
                .map(|w| Tile {
                    name: w.name,
                    title: tr(w.title).to_string(),
                    shown: (w.shows)(&self.live),
                    placed: false,
                })
                .collect(),
            compact,
            width,
            hover: None,
            focus: None,
        };
        mark_placed(&mut view, &now);
        let layout = editor::layout(&view);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(mut popup) = Popup::new(self, EDITOR, layout.size, layout.size, scale, room)
        else {
            return;
        };
        popup.set_cards(editor::cards(&layout, &self.tokens), &self.compositor);
        // The panel's height, and the dock's gap above it, with the margin
        // every popup keeps; the drawer is centred by the compositor.
        let above = paint::height(style, &self.tokens) as i32
            + if style == Style::Dock { DOCK_MARGIN } else { 0 }
            + MARGIN
            - room as i32;
        let surface = &popup.surface;
        match edge {
            Edge::Bottom => {
                surface.set_anchor(Anchor::BOTTOM);
                surface.set_margin(0, 0, above, 0);
            }
            Edge::Top => {
                surface.set_anchor(Anchor::TOP);
                surface.set_margin(above, 0, 0, 0);
            }
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
                    Box::new(|shell: &mut Shell, _, event| shell.editor_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the panel editor: {e}"))
                .ok()
        });
        self.editor = Some(Editor {
            popup,
            view,
            keyboard,
            reader: a11y::Reader::new(tr("Edit panels")),
            logged: String::new(),
            original,
            now,
        });
        // The panels show their widgets as tiles while the drawer is open.
        self.draw_all();
    }

    /// Draws the drawer if what it shows changed. Its first drawing logs
    /// that it is shown, with the number of widgets in it.
    pub fn draw_editor(&mut self) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let view = editor.view.clone();
        let layout = editor::layout(&view);
        let Some(mut pixmap) = editor.popup.canvas(&view) else {
            return;
        };
        let scale = editor.popup.scale();
        editor::paint(
            &mut pixmap,
            &view,
            &layout,
            &self.tokens,
            Some(&mut self.icons),
            Some(&mut self.text),
            scale,
        );
        let places = editor::places(&layout, &view);
        if places != editor.logged {
            eprintln!("edel-shell-ui: panel editor places {places}");
            editor.logged = places;
        }
        let first = editor.popup.show(
            view.clone(),
            &pixmap,
            &self.tokens,
            "panel editor",
            &self.qh,
        );
        if first {
            eprintln!(
                "edel-shell-ui: panel editor shown, {} widgets in the drawer",
                view.tiles.len()
            );
        }
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        editor.reader.update(size, editor::nodes(&view, &layout));
    }

    /// Undo: the panels go back to what they were when the editor opened;
    /// the drawer stays open.
    fn undo_editor(&mut self) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        editor.now = editor.original.clone();
        let wanted = editor.original.clone();
        mark_placed(&mut editor.view, &editor.now);
        self.panels_changed(wanted);
        self.draw_editor();
        eprintln!("edel-shell-ui: panel editor undo");
    }

    /// Closes the drawer. With `write` (Done) the panels as they now are
    /// are written to the person's settings file; without it (Escape, or
    /// the keyboard leaving) the panels go back to what they were. Then the
    /// panels stop showing tiles and the drawer is gone.
    pub fn close_editor(&mut self, write: bool) {
        let Some(Editor {
            popup,
            keyboard,
            now,
            original,
            ..
        }) = self.editor.take()
        else {
            return;
        };
        if let Some(keyboard) = &keyboard {
            keyboard.release();
        }
        drop(popup);
        if write {
            self.save_panels(&now);
        } else {
            self.panels_changed(original);
            eprintln!("edel-shell-ui: panel editor undone");
        }
        self.draw_all();
        eprintln!("edel-shell-ui: panel editor hidden");
    }

    /// Writes the panels `now` to the person's settings file as
    /// `layout.panels`, or takes the person's own line out when what
    /// applies without it is what they are now (writers never write a
    /// default, ADR-008).
    fn save_panels(&mut self, now: &[presets::Panel]) {
        let (machine, person) = settings_texts();
        let value = match panel_edit::to_write(now, machine.as_deref(), person.as_deref()) {
            Ok(value) => value,
            Err(e) => {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::panels_not_kept(format!("{e:#}"))
                );
                return;
            }
        };
        let Some(path) = places::person_settings().map(|p| places::found(&p)) else {
            eprintln!("edel-shell-ui: {}", messages::PANELS_NO_HOME);
            return;
        };
        match value {
            Some(value) => match settings::write(&path, PANELS, Some(&value)) {
                Ok(()) => eprintln!("edel-shell-ui: panel editor done, layout.panels written"),
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::panels_not_kept(format!("{e:#}"))
                ),
            },
            None => {
                // The person's own line, if any, is what applies otherwise.
                let had = person
                    .as_deref()
                    .is_some_and(|text| settings::unset(text, PANELS).is_ok_and(|u| u != text));
                if had {
                    match settings::write(&path, PANELS, None) {
                        Ok(()) => {
                            eprintln!("edel-shell-ui: panel editor done, layout.panels taken out")
                        }
                        Err(e) => eprintln!(
                            "edel-shell-ui: {}",
                            messages::panels_not_kept(format!("{e:#}"))
                        ),
                    }
                } else {
                    eprintln!("edel-shell-ui: panel editor done, nothing to write");
                }
            }
        }
    }

    /// A key while the drawer is open, as `editor::key` says it acts.
    pub fn editor_key(&mut self, event: KeyEvent) {
        let Some(key) = key_of(event.keysym) else {
            return;
        };
        let Some(editor) = &self.editor else {
            return;
        };
        match editor::key(&editor.view, editor.view.focus, key) {
            Step::Focus(part) => {
                if let Some(editor) = &mut self.editor {
                    editor.view.focus = Some(part);
                }
                self.draw_editor();
            }
            Step::Act(Act::Undo) => self.undo_editor(),
            Step::Act(Act::Done) => self.close_editor(true),
            Step::Act(Act::Close) => self.close_editor(false),
            Step::Nothing => {}
        }
    }

    /// The pointer on the drawer: it lights what it is over, and a left
    /// click on Undo or Done acts. A tile takes no click yet.
    pub fn editor_pointer(&mut self, event: &PointerEvent) {
        let Some(editor) = &self.editor else {
            return;
        };
        // From the card's corner, inside the shadow's room.
        let room = editor.popup.room() as f32;
        let layout = editor::layout(&editor.view);
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let over: Option<Part> = editor::hit(&layout, x, y);
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if let Some(editor) = &mut self.editor {
                    editor.view.hover = over;
                }
                self.draw_editor();
            }
            PointerEventKind::Leave { .. } => {
                if let Some(editor) = &mut self.editor {
                    editor.view.hover = None;
                }
                self.draw_editor();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Part::Undo) => self.undo_editor(),
                Some(Part::Done) => self.close_editor(true),
                _ => {}
            },
            _ => {}
        }
    }

    /// Whether `surface` is the open drawer's.
    pub fn is_editor(&self, surface: &wl_surface::WlSurface) -> bool {
        self.editor.as_ref().is_some_and(|e| e.popup.is(surface))
    }
}
