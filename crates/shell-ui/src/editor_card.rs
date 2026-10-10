//! The shell's side of Edit panels (M5.31b): `editor.rs` lays the drawer
//! out and draws it; this opens it from the panel menu, holds its surface
//! and keyboard, keeps the panels' original and changed lines, and writes
//! them when Done is pressed. While it is open the panels show their
//! widgets as tiles (`paint::Look::editing`). A press on a widget's place
//! on a panel, or on a tile that is not placed, starts a drag: the drag
//! says where the widget would land (`editor::landing`), the panels and the
//! drawer show it, and the release moves, takes out or adds the widget
//! through `panel_edit`. Done opens the Undo bar (`undo_bar.rs`), which
//! puts the panels back for ten seconds.

use edel::i18n::tr;
use edel::panel_edit::{self, Group, PANELS, Spot};
use edel::places;
use edel::presets::{self, Edge, Size, Style};
use edel::settings;
use smithay_client_toolkit::reexports::calloop::RegistrationToken;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, LayerSurface};
use std::time::Duration;

use crate::a11y;
use crate::editor::{self, Act, DRAG_START, Landing, Part, Step, Tile, View};
use crate::messages;
use crate::paint;
use crate::popup::Popup;
use crate::quick;
use crate::trayview::Key;
use crate::undo_bar;
use crate::widgets;
use crate::{DOCK_MARGIN, EDITOR, MARGIN, Panel, Shell, UNDO, fillets, settings_texts};

/// What `save_panels` did, as the log lines say it.
const WRITTEN: &str = "layout.panels written";
const TAKEN: &str = "layout.panels taken out";
pub(crate) const NOTHING: &str = "nothing to write";

/// The open drawer: its surface and what it shows, the keyboard, what a
/// screen reader reads, the last places logged, and the panels it started
/// from (`original`) and what they are now (`now`), as the editor changes
/// them. `edge` and `above` say where the drawer lies (above the bottom
/// panel, or below the top one), and `drag` is the drag in progress, if any.
pub struct Editor {
    pub popup: Popup<View>,
    pub view: View,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub reader: a11y::Reader,
    pub logged: String,
    pub original: Vec<presets::Panel>,
    pub now: Vec<presets::Panel>,
    pub edge: Edge,
    pub above: i32,
    pub drag: Option<Drag>,
}

/// A press that may become a drag (M5.31b): the widget's `name`, the panel
/// it was lifted from (`None` from the drawer), where it went down in
/// screen logical pixels, whether the pointer has moved far enough to make
/// it a move, and where the pointer is over now.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub name: &'static str,
    pub from: Option<usize>,
    pub start: (f32, f32),
    pub moving: bool,
    pub over: Over,
}

/// What a moving drag is over: nothing, a panel (its index in `Shell::panels`
/// and where it would land there), or the drawer.
#[derive(Debug, Clone, PartialEq)]
pub enum Over {
    Nothing,
    Panel(usize, Landing),
    Drawer,
}

/// The Undo bar after Done (M5.31b): its surface, what it shows, the
/// panels as they were before the editor opened (`original`), the timer
/// that ends it, the places last logged and what a screen reader reads.
pub struct UndoBar {
    pub popup: Popup<undo_bar::View>,
    pub view: undo_bar::View,
    pub original: Vec<presets::Panel>,
    pub timer: Option<RegistrationToken>,
    pub logged: String,
    pub reader: a11y::Reader,
}

/// Marks each tile placed when a panel in `now` holds its widget.
fn mark_placed(view: &mut View, now: &[presets::Panel]) {
    for tile in &mut view.tiles {
        tile.placed = panel_edit::find(now, tile.name).is_some();
    }
}

/// The keyboard key the drawer takes for `sym`, if any.
pub fn key_of(sym: Keysym) -> Option<Key> {
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

/// The widgets of one group of a panel.
fn list_of(panel: &presets::Panel, group: Group) -> &[String] {
    match group {
        Group::Start => &panel.start,
        Group::Centre => &panel.centre,
        Group::End => &panel.end,
    }
}

/// Where `widget` lies on panel `panel` of `now`, if it is there.
fn spot_in(now: &[presets::Panel], panel: usize, widget: &str) -> Option<Spot> {
    let p = now.get(panel)?;
    Group::ALL.into_iter().find_map(|group| {
        list_of(p, group)
            .iter()
            .position(|w| w == widget)
            .map(|index| Spot {
                panel,
                group,
                index,
            })
    })
}

/// The landing's place among `now`'s groups: its group, and the index of
/// the widget it lands before (the group's length when none).
fn spot_at_landing(now: &[presets::Panel], panel: usize, landing: &Landing) -> Option<Spot> {
    let list = list_of(now.get(panel)?, landing.group);
    let index = landing
        .before
        .and_then(|before| list.iter().position(|w| w == before))
        .unwrap_or(list.len());
    Some(Spot {
        panel,
        group: landing.group,
        index,
    })
}

/// Hangs the drawer or the Undo bar `above` logical pixels from the
/// screen's edge. It takes no notice of the panels' exclusive zones, which
/// the compositor would otherwise add to the margin (the margin already
/// clears the panel), so the drag's geometry can place it.
fn anchor_above(surface: &LayerSurface, edge: Edge, above: i32) {
    surface.set_exclusive_zone(-1);
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
}

/// The panel's widgets as the landing wants them: each with its group and
/// its place from the panel's left, in the row's order.
fn landing_row(panel: &Panel) -> Vec<(&'static str, Group, f32, f32)> {
    let groups = [
        (Group::Start, &panel.row.start),
        (Group::Centre, &panel.row.centre),
        (Group::End, &panel.row.end),
    ];
    groups
        .into_iter()
        .flat_map(|(group, widgets)| widgets.iter().map(move |w| (w.name, group)))
        .zip(panel.places.iter())
        .map(|((name, group), &(left, width))| (name, group, left, width))
        .collect()
}

impl Shell {
    /// How far above its edge a popup on a panel of this shape starts, in
    /// logical pixels: the panel's height, the gap a dock or a floating bar
    /// keeps above it, and the margin every popup keeps, less the `room`
    /// round the popup's own card. The drawer, the Undo bar and the panel
    /// menu share it (M5.31b, M5.31c).
    pub(crate) fn above_panel(&self, style: Style, size: Size, floating: bool, room: u32) -> i32 {
        paint::height(style, size, &self.tokens) as i32
            + match (style, floating) {
                (Style::Dock, _) => DOCK_MARGIN,
                (Style::Bar, true) => self.tokens.gap as i32,
                (Style::Bar, false) => 0,
            }
            + MARGIN
            - room as i32
    }

    /// Opens Edit panels (M5.31b): the drawer centred above the bottom panel
    /// (below the top one when there is no bottom panel), the keyboard
    /// exclusive, and every panel showing its widgets as tiles. The Undo
    /// bar of an earlier Done goes.
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
        let (edge, style, size, floating, scale) = {
            let panel = &self.panels[i];
            (
                panel.edge,
                panel.style,
                panel.size,
                panel.floating,
                panel.scale,
            )
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
            lifted: None,
            taking: false,
        };
        mark_placed(&mut view, &now);
        let layout = editor::layout(&view);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(mut popup) = Popup::new(self, EDITOR, layout.size, layout.size, scale, room)
        else {
            return;
        };
        popup.set_cards(editor::cards(&layout, &self.tokens), &self.compositor);
        // The drawer is centred by the compositor; its height above the
        // panel is the panel's.
        let above = self.above_panel(style, size, floating, room);
        anchor_above(&popup.surface, edge, above);
        popup
            .surface
            .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        popup.surface.commit();
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
            edge,
            above,
            drag: None,
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
    /// are written to the person's settings file, and the Undo bar opens
    /// when that wrote or took out a line; without it (Escape, or the
    /// keyboard leaving) the panels go back to what they were. Then the
    /// panels stop showing tiles and the drawer is gone.
    pub fn close_editor(&mut self, write: bool) {
        let Some(Editor {
            popup,
            keyboard,
            now,
            original,
            edge,
            above,
            ..
        }) = self.editor.take()
        else {
            return;
        };
        let scale = popup.scale() as u32;
        if let Some(keyboard) = &keyboard {
            keyboard.release();
        }
        drop(popup);
        if write {
            if let Some(what) = self.save_panels(&now) {
                eprintln!("edel-shell-ui: panel editor done, {what}");
                if what != NOTHING {
                    self.open_undo_bar(edge, above, scale, original);
                }
            }
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
    /// default, ADR-008). Returns what it did, `WRITTEN`, `TAKEN` or
    /// `NOTHING`, or `None` after saying why it could not.
    pub(crate) fn save_panels(&self, now: &[presets::Panel]) -> Option<&'static str> {
        let (machine, person) = settings_texts();
        let value = match panel_edit::to_write(now, machine.as_deref(), person.as_deref()) {
            Ok(value) => value,
            Err(e) => {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::panels_not_kept(format!("{e:#}"))
                );
                return None;
            }
        };
        let Some(path) = places::person_settings().map(|p| places::found(&p)) else {
            eprintln!("edel-shell-ui: {}", messages::PANELS_NO_HOME);
            return None;
        };
        match value {
            Some(value) => match settings::write(&path, PANELS, Some(&value)) {
                Ok(()) => Some(WRITTEN),
                Err(e) => {
                    eprintln!(
                        "edel-shell-ui: {}",
                        messages::panels_not_kept(format!("{e:#}"))
                    );
                    None
                }
            },
            None => {
                // The person's own line, if any, is what applies otherwise.
                let had = person
                    .as_deref()
                    .is_some_and(|text| settings::unset(text, PANELS).is_ok_and(|u| u != text));
                if !had {
                    return Some(NOTHING);
                }
                match settings::write(&path, PANELS, None) {
                    Ok(()) => Some(TAKEN),
                    Err(e) => {
                        eprintln!(
                            "edel-shell-ui: {}",
                            messages::panels_not_kept(format!("{e:#}"))
                        );
                        None
                    }
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

    /// The pointer on the drawer: it lights what it is over, a left click on
    /// Undo or Done acts, and a press on a tile that is not placed starts
    /// a drag. A drag's motion and release come here too.
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
        // Screen logical pixels: the drawer's origin plus the pointer's place.
        let (rx, ry, _, _) = self.drawer_rect().unwrap_or_default();
        let at = (rx + event.position.0 as f32, ry + event.position.1 as f32);
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                self.drag_moved(at);
                if let Some(editor) = &mut self.editor {
                    editor.view.hover = over;
                }
                self.draw_editor();
            }
            PointerEventKind::Leave { .. } => {
                self.cancel_drag();
                if let Some(editor) = &mut self.editor {
                    editor.view.hover = None;
                }
                self.draw_editor();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Part::Undo) => self.undo_editor(),
                Some(Part::Done) => self.close_editor(true),
                Some(Part::Tile(tile)) => self.start_tile_drag(tile, at),
                None => {}
            },
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => self.end_drag(),
            _ => {}
        }
    }

    /// Whether `surface` is the open drawer's.
    pub fn is_editor(&self, surface: &wl_surface::WlSurface) -> bool {
        self.editor.as_ref().is_some_and(|e| e.popup.is(surface))
    }

    // ---- Geometry, in screen logical pixels ----

    /// The screen's size: the first output's logical size, else the widest
    /// panel's width and no height, as far as is known.
    pub fn screen_size(&self) -> (f32, f32) {
        let logical = self
            .outputs
            .outputs()
            .find_map(|output| self.outputs.info(&output))
            .and_then(|info| info.logical_size);
        match logical {
            Some((w, h)) => (w as f32, h as f32),
            None => (self.screen_width() as f32, 0.0),
        }
    }

    /// Panel `i`'s place on the screen: a bar spans the screen's width
    /// along its edge, a dock its width centred, `DOCK_MARGIN` from the
    /// edge, and a floating bar `size.gap` from its edge and both sides
    /// (M5.31c); each is its surface, the fillets' strip included, at its
    /// size. This is the main screen's: a panel on another screen is
    /// placed by that screen (`screens = "every"`).
    pub fn panel_rect(&self, i: usize) -> (f32, f32, f32, f32) {
        let (sw, sh) = self.screen_size();
        let Some(panel) = self.panels.get(i) else {
            return (0.0, 0.0, 0.0, 0.0);
        };
        let bottom = panel.edge == Edge::Bottom;
        let gap = if panel.floating {
            self.tokens.gap as f32
        } else {
            0.0
        };
        let h = (paint::height(panel.style, panel.size, &self.tokens)
            + paint::strip(panel.style, panel.floating, &self.tokens)) as f32;
        let (x, w) = match (panel.style, panel.floating) {
            (Style::Dock, _) => {
                let w = panel.width as f32;
                (((sw - w) / 2.0).round(), w)
            }
            (Style::Bar, true) => (gap, sw - 2.0 * gap),
            (Style::Bar, false) => (0.0, sw),
        };
        let y = match (panel.style, bottom) {
            (Style::Dock, true) => sh - h - DOCK_MARGIN as f32,
            (Style::Dock, false) => DOCK_MARGIN as f32,
            (Style::Bar, true) => sh - h - gap,
            (Style::Bar, false) => gap,
        };
        (x, y, w, h)
    }

    /// The drawer's surface on the screen, room for its shadow included:
    /// centred, `above` from its edge. `None` while no drawer is open.
    pub fn drawer_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let editor = self.editor.as_ref()?;
        let room = editor.popup.room() as f32;
        let (w, h) = editor.popup.size();
        let (w, h) = (w as f32 + 2.0 * room, h as f32 + 2.0 * room);
        let (sw, sh) = self.screen_size();
        let x = ((sw - w) / 2.0).round();
        let y = match editor.edge {
            Edge::Bottom => sh - editor.above as f32 - h,
            Edge::Top => editor.above as f32,
        };
        Some((x, y, w, h))
    }

    /// What a drag at screen position `at` is over: the drawer first, then
    /// each panel, where `editor::landing` says where the widget would land
    /// (a widget lifted off a panel is not a place to land by). Nothing else.
    pub fn over_at(&self, at: (f32, f32)) -> Over {
        let inside = |(x, y, w, h): (f32, f32, f32, f32)| {
            at.0 >= x && at.0 < x + w && at.1 >= y && at.1 < y + h
        };
        if self.drawer_rect().is_some_and(inside) {
            return Over::Drawer;
        }
        let lifted = self
            .editor
            .as_ref()
            .and_then(|e| e.drag.as_ref())
            .filter(|d| d.from.is_some())
            .map(|d| d.name);
        for (i, panel) in self.panels.iter().enumerate() {
            let rect = self.panel_rect(i);
            if !inside(rect) {
                continue;
            }
            let x = at.0 - rect.0;
            return match editor::landing(&landing_row(panel), panel.empty, x, lifted) {
                Some(landing) => Over::Panel(i, landing),
                None => Over::Nothing,
            };
        }
        Over::Nothing
    }

    // ---- Drags ----

    /// What a drag shows on panel `i`: the widget lifted off it, and the
    /// caret where a moving drag would land there.
    pub fn drag_marks(&self, i: usize) -> (Option<&'static str>, Option<f32>) {
        // An app dragged in the apps widget shows its landing too (M5.31d).
        if let Some(caret) = self.app_caret(i) {
            return (None, Some(caret));
        }
        let Some(drag) = self
            .editor
            .as_ref()
            .and_then(|e| e.drag.as_ref())
            .filter(|d| d.moving)
        else {
            return (None, None);
        };
        let lifted = (drag.from == Some(i)).then_some(drag.name);
        let caret = match &drag.over {
            Over::Panel(j, landing) if *j == i => Some(landing.caret),
            _ => None,
        };
        (lifted, caret)
    }

    /// Starts a drag on panel `i`'s widget at `x` logical pixels along it,
    /// where `at` is the pointer on the screen.
    fn start_panel_drag(&mut self, i: usize, x: f32, at: (f32, f32)) {
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let Some(j) = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))
        else {
            return;
        };
        let Some(widget) = panel.row.widget(j) else {
            return;
        };
        let name = widget.name;
        self.begin_drag(Drag {
            name,
            from: Some(i),
            start: at,
            moving: false,
            over: Over::Nothing,
        });
    }

    /// Starts a drag of drawer tile `tile`, if no panel holds it; `at` is
    /// the pointer on the screen.
    fn start_tile_drag(&mut self, tile: usize, at: (f32, f32)) {
        let Some(tile) = self.editor.as_ref().and_then(|e| e.view.tiles.get(tile)) else {
            return;
        };
        if tile.placed {
            return;
        }
        let name = tile.name;
        self.begin_drag(Drag {
            name,
            from: None,
            start: at,
            moving: false,
            over: Over::Nothing,
        });
    }

    /// Makes `drag` the drawer's drag. A press always starts a new one: a
    /// drag left over from a release that never came is dropped here.
    fn begin_drag(&mut self, drag: Drag) {
        if let Some(editor) = &mut self.editor {
            editor.drag = Some(drag);
        }
    }

    /// The pointer is at `at` on the screen during a drag: once it has
    /// moved `DRAG_START` the drag is a move, logged once, and says what it
    /// is over. Nothing happens without a drag.
    fn drag_moved(&mut self, at: (f32, f32)) {
        let Some(drag) = self.editor.as_ref().and_then(|e| e.drag.as_ref()) else {
            return;
        };
        let was = drag.moving;
        let far = (at.0 - drag.start.0).hypot(at.1 - drag.start.1) >= DRAG_START;
        if !was && !far {
            return;
        }
        let over = self.over_at(at);
        let Some(drag) = self.editor.as_mut().and_then(|e| e.drag.as_mut()) else {
            return;
        };
        if !was {
            eprintln!("edel-shell-ui: panel editor drag {}", drag.name);
        }
        drag.moving = true;
        drag.over = over;
        self.show_drag();
    }

    /// Shows the drag as it is: the drawer's tile lifted or the card
    /// outlined, the panels' lifted widget and carets. Redraws what changed.
    fn show_drag(&mut self) {
        let Some(editor) = &self.editor else {
            return;
        };
        let (lifted, taking) = match editor.drag.as_ref().filter(|d| d.moving) {
            Some(drag) => (
                match drag.from {
                    None => editor.view.tiles.iter().position(|t| t.name == drag.name),
                    Some(_) => None,
                },
                drag.from.is_some() && drag.over == Over::Drawer,
            ),
            None => (None, false),
        };
        if let Some(editor) = &mut self.editor {
            editor.view.lifted = lifted;
            editor.view.taking = taking;
        }
        self.draw_all();
        self.draw_editor();
    }

    /// The pointer let go: a drag that never moved was a click and does
    /// nothing; a moved one is dropped where it is over.
    fn end_drag(&mut self) {
        let Some(drag) = self.editor.as_mut().and_then(|e| e.drag.take()) else {
            return;
        };
        if drag.moving {
            self.drop_drag(drag);
        }
        self.show_drag();
    }

    /// The pointer left the surface during a drag: the drag is dropped,
    /// and nothing changes.
    fn cancel_drag(&mut self) {
        let dropped = self.editor.as_mut().and_then(|e| e.drag.take()).is_some();
        if dropped {
            self.show_drag();
        }
    }

    /// The index in `now` of the panel that takes panel `i`'s edge.
    fn spec_panel(&self, now: &[presets::Panel], i: usize) -> Option<usize> {
        let edge = self.panels.get(i)?.edge;
        now.iter().position(|p| p.edge == edge)
    }

    /// Where the drag's widget lies in `now`, when it was lifted from panel
    /// `i` (named by `widget`).
    fn spot_on_edge(&self, now: &[presets::Panel], i: usize, widget: &str) -> Option<Spot> {
        let panel = self.spec_panel(now, i)?;
        spot_in(now, panel, widget)
    }

    /// Where a landing on panel `j` puts a widget in `now`.
    fn spot_of_landing(&self, now: &[presets::Panel], j: usize, landing: &Landing) -> Option<Spot> {
        let panel = self.spec_panel(now, j)?;
        spot_at_landing(now, panel, landing)
    }

    /// Does what the drop of the drag says: moves a widget, takes it out, or
    /// adds it, through `panel_edit`. The panels change at once when the
    /// result differs; a refused one says why and the panels stay.
    fn drop_drag(&mut self, drag: Drag) {
        let Some(editor) = &self.editor else {
            return;
        };
        let now = editor.now.clone();
        let done = match (drag.from, drag.over) {
            (Some(i), Over::Panel(j, landing)) => {
                let Some(from) = self.spot_on_edge(&now, i, drag.name) else {
                    return;
                };
                let Some(mut to) = self.spot_of_landing(&now, j, &landing) else {
                    return;
                };
                if from.panel == to.panel && from.group == to.group && from.index < to.index {
                    to.index -= 1;
                }
                panel_edit::move_widget(&now, from, to).map(|new| (new, "moved"))
            }
            (Some(i), _) => {
                let Some(from) = self.spot_on_edge(&now, i, drag.name) else {
                    return;
                };
                panel_edit::remove_widget(&now, from).map(|new| (new, "took out"))
            }
            (None, Over::Panel(j, landing)) => {
                let Some(to) = self.spot_of_landing(&now, j, &landing) else {
                    return;
                };
                panel_edit::add_widget(&now, drag.name, to).map(|new| (new, "added"))
            }
            (None, _) => return,
        };
        match done {
            Ok((new, what)) => {
                eprintln!("edel-shell-ui: panel editor {what} {}", drag.name);
                if new != now {
                    self.take_panels(new);
                }
            }
            Err(e) => eprintln!(
                "edel-shell-ui: {}",
                messages::panel_not_changed(format!("{e:#}"))
            ),
        }
    }

    /// The panels the editor made: kept as what Done writes, and shown at
    /// once.
    fn take_panels(&mut self, now: Vec<presets::Panel>) {
        if let Some(editor) = &mut self.editor {
            editor.now = now.clone();
            mark_placed(&mut editor.view, &editor.now);
        }
        self.panels_changed(now);
    }

    /// The pointer on panel `i` while the editor is open (M5.31b): a left
    /// press on a widget's place starts a drag, the motion and the release
    /// carry it on, and the rest does nothing.
    pub fn editor_panel_pointer(&mut self, i: usize, event: &PointerEvent) {
        let (ex, ey) = (event.position.0 as f32, event.position.1 as f32);
        let (ox, oy, _, _) = self.panel_rect(i);
        let at = (ox + ex, oy + ey);
        match &event.kind {
            PointerEventKind::Motion { .. } => self.drag_moved(at),
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                self.start_panel_drag(i, ex, at);
            }
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => self.end_drag(),
            PointerEventKind::Leave { .. } => self.cancel_drag(),
            _ => {}
        }
    }

    // ---- The Undo bar ----

    /// Opens the Undo bar where the drawer was, for `undo_bar::SECONDS`:
    /// it puts `original` back when Undo is pressed.
    pub(crate) fn open_undo_bar(
        &mut self,
        edge: Edge,
        above: i32,
        scale: u32,
        original: Vec<presets::Panel>,
    ) {
        let view = undo_bar::View {
            hover: false,
            width: self.screen_width(),
        };
        let layout = undo_bar::layout(&view);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(mut popup) = Popup::new(self, UNDO, layout.size, layout.size, scale, room) else {
            return;
        };
        popup.set_cards(undo_bar::cards(&layout, &self.tokens), &self.compositor);
        anchor_above(&popup.surface, edge, above);
        // The bar never takes the keyboard from what a person is typing in.
        popup
            .surface
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        popup.surface.commit();
        self.undo_bar = Some(UndoBar {
            popup,
            view,
            original,
            timer: None,
            logged: String::new(),
            reader: a11y::Reader::new(tr("Panels changed")),
        });
        let timer = self.undo_timer();
        if let Some(bar) = &mut self.undo_bar {
            bar.timer = timer;
        }
        self.draw_undo_bar();
    }

    /// A timer that ends the Undo bar `undo_bar::SECONDS` from now.
    fn undo_timer(&mut self) -> Option<RegistrationToken> {
        self.handle
            .insert_source(
                Timer::from_duration(Duration::from_secs(undo_bar::SECONDS)),
                |_, _, shell: &mut Shell| {
                    if let Some(bar) = &mut shell.undo_bar {
                        bar.timer = None;
                    }
                    shell.hide_undo_bar();
                    TimeoutAction::Drop
                },
            )
            .inspect_err(|e| eprintln!("edel-shell-ui: no timer for the undo bar: {e}"))
            .ok()
    }

    /// Closes the Undo bar and lets go of its timer, surface and reader.
    pub fn hide_undo_bar(&mut self) {
        let Some(bar) = self.undo_bar.take() else {
            return;
        };
        if let Some(token) = bar.timer {
            self.handle.remove(token);
        }
        drop(bar);
        eprintln!("edel-shell-ui: panels undo bar hidden");
    }

    /// Draws the Undo bar if what it shows changed. Its first drawing logs
    /// that it is shown, and where Undo lies, which CI reads.
    pub fn draw_undo_bar(&mut self) {
        let Some(bar) = &mut self.undo_bar else {
            return;
        };
        let view = bar.view.clone();
        let layout = undo_bar::layout(&view);
        let Some(mut pixmap) = bar.popup.canvas(&view) else {
            return;
        };
        let scale = bar.popup.scale();
        undo_bar::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let first = bar
            .popup
            .show(view.clone(), &pixmap, &self.tokens, "undo bar", &self.qh);
        if first {
            eprintln!("edel-shell-ui: panels undo bar shown");
        }
        let places = undo_bar::places(&layout);
        if places != bar.logged {
            eprintln!("edel-shell-ui: panels undo bar places {places}");
            bar.logged = places;
        }
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        bar.reader.update(size, undo_bar::nodes(&layout));
    }

    /// Undo on the bar: the panels' line is written back as it was before
    /// the editor opened, through the same writer as Done, and the panels
    /// show it; then the bar closes. Nothing changes when the line cannot
    /// be written.
    fn undo_panels(&mut self) {
        let Some(original) = self.undo_bar.as_ref().map(|b| b.original.clone()) else {
            return;
        };
        if let Some(what) = self.save_panels(&original) {
            self.panels_changed(original);
            eprintln!("edel-shell-ui: panels undone, {what}");
        }
        self.hide_undo_bar();
    }

    /// The pointer on the Undo bar: it lights Undo under the pointer, and a
    /// left click on Undo undoes.
    pub fn undo_pointer(&mut self, event: &PointerEvent) {
        let Some(bar) = &self.undo_bar else {
            return;
        };
        // From the card's corner, inside the shadow's room.
        let room = bar.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let layout = undo_bar::layout(&bar.view);
        let over = undo_bar::hit(&layout, x, y);
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                // The bar waits while the pointer rests on it, so it never
                // goes from under a hand reaching for Undo.
                let token = self.undo_bar.as_mut().and_then(|bar| {
                    bar.view.hover = over;
                    bar.timer.take()
                });
                if let Some(token) = token {
                    self.handle.remove(token);
                }
                self.draw_undo_bar();
            }
            PointerEventKind::Leave { .. } => {
                let waiting = self
                    .undo_bar
                    .as_ref()
                    .is_some_and(|bar| bar.timer.is_none());
                let timer = if waiting { self.undo_timer() } else { None };
                if let Some(bar) = &mut self.undo_bar {
                    bar.view.hover = false;
                    if waiting {
                        bar.timer = timer;
                    }
                }
                self.draw_undo_bar();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT && over => {
                self.undo_panels();
            }
            _ => {}
        }
    }

    /// Whether `surface` is the open Undo bar's.
    pub fn is_undo_bar(&self, surface: &wl_surface::WlSurface) -> bool {
        self.undo_bar.as_ref().is_some_and(|b| b.popup.is(surface))
    }
}
