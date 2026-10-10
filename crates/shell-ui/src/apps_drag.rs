//! The apps widget's drag (M5.31d): a left press on an app's cell waits for
//! the release, as a kept tray icon's does (`tray_card.rs`). Let go without
//! having moved, it is the click it always was: the app starts, or its
//! window comes forward or goes down. Dragged over the same widget, the app
//! is pinned where it is let go, and the panel's caret shows where while it
//! is dragged; dragged off the panel, a pinned app is unpinned. The list is
//! `panels.pinned` in the person's settings file, written as Settings writes a
//! key (ADR-008), and the widget shows it at once, without waiting for the
//! file's news.

use edel::apps;
use edel::panel_edit::{self, PINNED};
use edel::places;
use edel::presets::Edge;
use edel::settings;

use crate::messages;
use crate::tray_card::moved;
use crate::widgets::Pin;
use crate::widgets::apps as cells;
use crate::{Shell, paint, settings_texts};

/// A left press on an app's cell that may become a drag (M5.31d): the panel,
/// the apps widget's index on it, the cell's app id, the pin as the list
/// names it when the cell is pinned, and where the press went down and where
/// the pointer is now, logical pixels along the panel.
pub struct AppPress {
    pub panel: usize,
    pub widget: usize,
    pub id: String,
    pub pin: Option<String>,
    pub from: (f32, f32),
    pub at: (f32, f32),
}

/// `pins` resolved against the installed apps (M5.31d): the apps the apps
/// widget shows for them, in order, and each one's index in `pins`. A pin
/// no installed app matches is left out, as the widget always left it out.
pub fn resolve_pins(pins: &[String], installed: &[apps::App]) -> (Vec<Pin>, Vec<usize>) {
    pins.iter()
        .enumerate()
        .filter_map(|(k, pin)| apps::pinned(installed, pin).map(|app| (Pin::from(app), k)))
        .unzip()
}

/// The apps pinned as the list names them, `settings, terminal`, or `none`
/// (M5.31d): what `pinned apps` logs.
pub fn pins_text(list: &[String]) -> String {
    if list.is_empty() {
        "none".to_string()
    } else {
        list.join(", ")
    }
}

/// Logs the apps pinned: `edel-shell-ui: pinned apps settings, terminal`.
pub fn log_pins(list: &[String]) {
    eprintln!("edel-shell-ui: pinned apps {}", pins_text(list));
}

impl Shell {
    /// The apps widget under `x` logical pixels along panel `i`: its index on
    /// the panel, its left edge and width, and what it shows.
    fn apps_widget(&self, i: usize, x: f32) -> Option<(usize, f32, f32, String)> {
        let panel = self.panels.get(i)?;
        let j = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))?;
        if panel.row.widget(j)?.name != "apps" {
            return None;
        }
        let shown = panel.drawn.as_ref()?.shown.get(j)?.clone();
        let (left, width) = panel.places[j];
        Some((j, left, width, shown))
    }

    /// Whether `y` on panel `i` lies off it: more than its height beyond its
    /// edge, below a top panel and above a bottom one, as a dragged tray icon
    /// is taken off (M5.31d).
    pub fn off_panel(&self, i: usize, y: f32) -> bool {
        let Some(panel) = self.panels.get(i) else {
            return false;
        };
        let height = paint::height(panel.style, panel.size, &self.tokens) as f32;
        match panel.edge {
            Edge::Bottom => y < -height,
            Edge::Top => y > 2.0 * height,
        }
    }

    /// Starts a press on an app's cell in panel `i` at `x`, `y` logical
    /// pixels, which waits for the release. False when the press is not on an
    /// app's cell, which then takes the press as it always did.
    pub fn press_app(&mut self, i: usize, x: f32, y: f32) -> bool {
        let Some((widget, left, width, shown)) = self.apps_widget(i, x) else {
            return false;
        };
        let Some(cell) = cells::cell_at(&shown, x - left, cells::cell_side(&shown, width)) else {
            return false;
        };
        let id = cells::read(&shown)[cell].id.to_string();
        // The pinned cells come first, in the list's order (`widgets::apps`).
        let pin = self.pin_slots.get(cell).map(|&k| self.pins[k].clone());
        self.app_press = Some(AppPress {
            panel: i,
            widget,
            id,
            pin,
            from: (x, y),
            at: (x, y),
        });
        true
    }

    /// The pointer moved over panel `i` while an app's press waits: where it
    /// is now, so the caret follows it (M5.31d).
    pub fn app_moved(&mut self, i: usize, x: f32, y: f32) {
        let Some(press) = self.app_press.as_mut() else {
            return;
        };
        if press.panel != i || press.at == (x, y) {
            return;
        }
        press.at = (x, y);
        self.draw_all();
    }

    /// Where a dragged app would land on panel `i`, as a caret in logical
    /// pixels along the panel: when the press has moved and the pointer is
    /// over its own apps widget, not off the panel. `None` otherwise, and
    /// when it would be unpinned.
    pub fn app_caret(&self, i: usize) -> Option<f32> {
        let press = self
            .app_press
            .as_ref()
            .filter(|p| p.panel == i && moved(p.from, p.at))?;
        let (x, y) = press.at;
        if self.off_panel(i, y) {
            return None;
        }
        let (widget, left, width, shown) = self.apps_widget(i, x)?;
        if widget != press.widget {
            return None;
        }
        let cell = cells::cell_side(&shown, width);
        let boundary = cells::landing(x - left, cell, self.live.pinned.len());
        Some(left + cells::boundary_x(cell, boundary))
    }

    /// The left button let go on panel `i` at `x`, `y` logical pixels: the
    /// press on an app's cell ends (M5.31d). Not moved, it is the click. Off
    /// the panel, a pinned app is unpinned. Over its own apps widget, it is
    /// pinned where it is let go. Anything else does nothing.
    pub fn app_release(&mut self, i: usize, x: f32, y: f32) {
        let Some(press) = self.app_press.take() else {
            return;
        };
        if press.panel != i {
            return;
        }
        if !moved(press.from, (x, y)) {
            self.open_app(&press.id);
            return;
        }
        if self.off_panel(i, y) {
            if let Some(pin) = &press.pin {
                self.unpin_app(pin);
            }
            return;
        }
        let Some((widget, left, width, shown)) = self.apps_widget(i, x) else {
            return;
        };
        if widget != press.widget {
            return;
        }
        let cell = cells::cell_side(&shown, width);
        let boundary = cells::landing(x - left, cell, self.live.pinned.len());
        self.pin_app(&press, boundary);
    }

    /// Pins the dragged app at `boundary` among the pinned cells. The list's
    /// place for it is counted after the app is taken out, as
    /// `panel_edit::pin_at` takes its index (M5.31d). Logs `pinned NAME at N`.
    fn pin_app(&mut self, press: &AppPress, boundary: usize) {
        let mut at = self
            .pin_slots
            .get(boundary)
            .copied()
            .unwrap_or(self.pins.len());
        if let Some(pin) = &press.pin {
            if let Some(from) = self.pins.iter().position(|p| p == pin) {
                if from < at {
                    at -= 1;
                }
            }
        }
        let name = press.pin.clone().unwrap_or_else(|| press.id.clone());
        let list = panel_edit::pin_at(&self.pins, &name, at);
        let index = list.iter().position(|p| *p == name).unwrap_or(0);
        eprintln!("edel-shell-ui: pinned {name} at {index}");
        self.save_pins(list);
    }

    /// Takes `pin` off the apps widget's list, logging `unpinned NAME`.
    fn unpin_app(&mut self, pin: &str) {
        eprintln!("edel-shell-ui: unpinned {pin}");
        let list = panel_edit::unpin(&self.pins, pin);
        self.save_pins(list);
    }

    /// Writes `list` as the person's `panels.pinned`, with what applies without
    /// it taken out, as writers never write a default (ADR-008), then makes it
    /// what the widget shows at once.
    fn save_pins(&mut self, list: Vec<String>) {
        let (machine, person) = settings_texts();
        let value = panel_edit::pins_to_write(&list, machine.as_deref(), person.as_deref());
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => {
                if let Err(e) = settings::write(&path, PINNED, value.as_deref()) {
                    eprintln!(
                        "edel-shell-ui: {}",
                        messages::pins_not_kept(format!("{e:#}"))
                    );
                }
            }
            None => eprintln!("edel-shell-ui: {}", messages::PINS_NO_HOME),
        }
        self.set_pins(list);
    }

    /// Makes `list` the apps the apps widget pins (M5.31d): the pinned cells
    /// are resolved as at start, the panels are drawn again and `pinned apps`
    /// is logged, when the list changed. `settings_changed` calls it with what
    /// the files say, and the widget's own changes with what it wrote.
    pub fn set_pins(&mut self, list: Vec<String>) {
        if list == self.pins {
            return;
        }
        let installed = apps::read_all(&apps::dirs());
        (self.live.pinned, self.pin_slots) = resolve_pins(&list, &installed);
        self.pins = list;
        log_pins(&self.pins);
        self.draw_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, categories: &str) -> apps::App {
        apps::parse(&format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec={id}\nCategories={categories};\n"
        ))
        .map(|mut app| {
            app.id = id.to_string();
            app
        })
        .unwrap()
    }

    #[test]
    fn a_pin_resolves_to_its_installed_app_and_keeps_its_place_in_the_list() {
        let installed = vec![
            app("org.gnome.Nautilus", "Files", "FileManager"),
            app("foot", "Foot", "TerminalEmulator"),
        ];
        let pins: Vec<String> = ["terminal", "mail", "org.gnome.Nautilus", "gone"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (shown, slots) = resolve_pins(&pins, &installed);
        let names: Vec<&str> = shown.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Foot", "Files"], "mail and gone are not installed");
        assert_eq!(slots, [0, 2], "the list index of each shown pin");
    }

    #[test]
    fn the_log_names_the_pins_as_the_list_writes_them() {
        let list = ["settings".to_string(), "terminal".to_string()];
        assert_eq!(pins_text(&list), "settings, terminal");
        assert_eq!(pins_text(&[]), "none");
    }
}
