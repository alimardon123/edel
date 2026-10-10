//! The workspace switcher's drag (M5.2p): a left press on a shown button of
//! the numbers look waits for the release, as an app's cell does
//! (`apps_drag.rs`). Let go without having moved, it is the click it always
//! was: the button's workspace is shown. Moved, and let go over the same
//! switcher, the workspace moves to the place its button lies over
//! (`widgets::workspaces::landing`), with its windows, name and policy
//! (`link.move_workspace`), and the names in the person's settings file
//! follow the new order (`layout.workspace_names`, written as Settings
//! writes a key, ADR-008). Dragged over the switcher, the panel's caret
//! shows where the button would land. Let go anywhere else, nothing
//! happens, and the pointer leaving the panel cancels the press.

use edel::places;
use edel::settings::{self, WORKSPACE_NAMES};
use toml::Value;

use crate::editor::DRAG_START;
use crate::messages;
use crate::widgets::Input;
use crate::widgets::workspaces as switcher;
use crate::{Shell, settings_texts};

/// A left press on a shown button that may become a drag (M5.2p): the
/// panel, the switcher's index on it, the place of the button pressed, and
/// where the press went down and where the pointer is now, logical pixels
/// along the panel. `slot` is the buttons' width as the press measured it,
/// which the caret and the drop use too.
pub struct WorkspacePress {
    pub panel: usize,
    pub widget: usize,
    pub from: usize,
    pub x: f32,
    pub y: f32,
    pub at: (f32, f32),
    pub slot: f32,
}

/// Whether a press has moved far enough to be a drag: `DRAG_START` px along
/// the panel, as the editor's drag counts it (M5.31b).
pub fn moved_along(from: (f32, f32), to: (f32, f32)) -> bool {
    (to.0 - from.0).hypot(to.1 - from.1) >= DRAG_START
}

/// The names to write for the person's `layout.workspace_names` after a
/// move, as a TOML array (M5.2p): none when `list` is what the machine's
/// file gives without the person's (ADR-008: writers never write a
/// default), so the person's line is reset; an empty list over a machine's
/// names says none.
pub fn names_to_write(list: &[String], machine: &[String]) -> Option<String> {
    (list != machine).then(|| {
        Value::Array(
            list.iter()
                .map(|name| Value::String(name.clone()))
                .collect(),
        )
        .to_string()
    })
}

impl Shell {
    /// The switcher under `x` logical pixels along panel `i`: its index on
    /// the panel, its left edge and width, and what it shows.
    fn switcher_under(&self, i: usize, x: f32) -> Option<(usize, f32, f32, String)> {
        let panel = self.panels.get(i)?;
        let j = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))?;
        if panel.row.widget(j)?.name != "workspaces" {
            return None;
        }
        let shown = panel.drawn.as_ref()?.shown.get(j)?.clone();
        let (left, width) = panel.places[j];
        Some((j, left, width, shown))
    }

    /// The buttons' width of the numbers look for `shown` on panel `i`,
    /// measured as the widget draws them at the panel's scale.
    fn switcher_slot(&mut self, i: usize, shown: &str) -> f32 {
        let scale = self.panels[i].scale as f32;
        let names = switcher::read(shown).names;
        switcher::slot(&names, &mut |words, face| {
            switcher::measure(Some(&mut self.text), &self.tokens, scale, words, face)
        })
    }

    /// How many workspaces the switcher of panel `i` shows: the screen's own
    /// when each screen has its own (M5.2o), else all of them.
    fn switcher_count(&self, i: usize) -> usize {
        let panel = &self.panels[i];
        self.workspaces
            .names(panel.output.as_ref().or(panel.entered.as_ref()))
            .len()
    }

    /// Starts a press on a shown button of the numbers look in panel `i` at
    /// `x`, `y` logical pixels, which waits for the release (M5.2p). False
    /// when the press is not on one, which then takes the press as it
    /// always did.
    pub fn press_workspace(&mut self, i: usize, x: f32, y: f32) -> bool {
        let Some((widget, left, _width, shown)) = self.switcher_under(i, x) else {
            return false;
        };
        let slot = self.switcher_slot(i, &shown);
        let Some(from) = switcher::button_at(&shown, slot, x - left) else {
            return false;
        };
        self.workspace_press = Some(WorkspacePress {
            panel: i,
            widget,
            from,
            x,
            y,
            at: (x, y),
            slot,
        });
        true
    }

    /// The pointer moved over panel `i` while a switcher button's press
    /// waits: where it is now, so the caret follows it (M5.2p).
    pub fn workspace_moved(&mut self, i: usize, x: f32, y: f32) {
        let Some(press) = self.workspace_press.as_mut() else {
            return;
        };
        if press.panel != i || press.at == (x, y) {
            return;
        }
        press.at = (x, y);
        self.draw_all();
    }

    /// Where a dragged button would land on panel `i`, as a caret in logical
    /// pixels along the panel: the left edge of the landing button, when the
    /// press has moved and the pointer is over its own switcher, not off the
    /// panel. `None` otherwise.
    pub fn workspace_caret(&self, i: usize) -> Option<f32> {
        let press = self
            .workspace_press
            .as_ref()
            .filter(|p| p.panel == i && moved_along((p.x, p.y), p.at))?;
        let (x, y) = press.at;
        if self.off_panel(i, y) {
            return None;
        }
        let (widget, left, _width, shown) = self.switcher_under(i, x)?;
        if widget != press.widget {
            return None;
        }
        let to = switcher::landing(&shown, press.slot, x - left)?;
        let view = switcher::read(&shown);
        switcher::buttons(&view, press.slot)
            .iter()
            .find(|b| b.place == to)
            .map(|b| left + b.left)
    }

    /// The left button let go on panel `i` at `x`, `y` logical pixels: the
    /// press on a switcher's button ends (M5.2p). Not moved, it is the click
    /// it was: the button's workspace is shown. Moved, and over the same
    /// switcher, the workspace moves to the place the button lies over, and
    /// the names follow. Anything else does nothing.
    pub fn workspace_release(&mut self, i: usize, x: f32, y: f32) {
        let Some(press) = self.workspace_press.take() else {
            return;
        };
        if press.panel != i {
            return;
        }
        if !moved_along((press.x, press.y), (x, y)) {
            if let Some((action, left, width)) = self.action_at(i, press.x, Input::Click) {
                self.run_action(i, press.x, action, left, width);
            }
            return;
        }
        self.drop_workspace(i, &press, x, y);
        // The caret goes with the press.
        self.draw_all();
    }

    /// Moves the workspace of `press` to where the button is let go at `x`,
    /// `y` on panel `i`, when that is over the same switcher and another
    /// place (M5.2p). Logs `moved workspace F to T`.
    fn drop_workspace(&mut self, i: usize, press: &WorkspacePress, x: f32, y: f32) {
        if self.off_panel(i, y) {
            return;
        }
        let Some((widget, left, _width, shown)) = self.switcher_under(i, x) else {
            return;
        };
        if widget != press.widget {
            return;
        }
        let Some(to) = switcher::landing(&shown, press.slot, x - left) else {
            return;
        };
        let from = press.from;
        if to == from {
            return;
        }
        self.link.move_workspace(from, to);
        eprintln!("edel-shell-ui: moved workspace {} to {}", from + 1, to + 1);
        let count = self.switcher_count(i);
        self.move_names(from, to, count);
    }

    /// Writes the person's `layout.workspace_names` in their new order after
    /// a move from place `from` to `to` among `count` workspaces (M5.2p).
    /// Nothing is written when there are no names at all; a failure says
    /// what stays as it was.
    fn move_names(&self, from: usize, to: usize, count: usize) {
        let (machine, person) = settings_texts();
        let names = settings::texts(WORKSPACE_NAMES, machine.as_deref(), person.as_deref())
            .unwrap_or_default();
        if names.is_empty() {
            return;
        }
        let without =
            settings::texts(WORKSPACE_NAMES, machine.as_deref(), None).unwrap_or_default();
        let list = switcher::reordered_names(&names, count, from, to);
        let value = names_to_write(&list, &without);
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => {
                if let Err(e) = settings::write(&path, WORKSPACE_NAMES, value.as_deref()) {
                    eprintln!(
                        "edel-shell-ui: {}",
                        messages::names_not_kept(format!("{e:#}"))
                    );
                }
            }
            None => eprintln!("edel-shell-ui: {}", messages::NAMES_NO_HOME),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_must_move_the_editor_s_drag_distance_to_be_a_drag() {
        assert!(!moved_along((10.0, 10.0), (10.0, 10.0)));
        assert!(!moved_along((10.0, 10.0), (13.0, 13.0)), "about 4 px");
        assert!(moved_along((10.0, 10.0), (16.0, 10.0)), "6 px is a drag");
        assert!(moved_along((0.0, 0.0), (0.0, -20.0)), "either way");
    }

    #[test]
    fn the_names_written_are_the_list_as_toml_or_none_when_the_machine_gives_it() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            names_to_write(&names(&["", "", "Mail"]), &[]).as_deref(),
            Some(r#"["", "", "Mail"]"#)
        );
        assert_eq!(
            names_to_write(&names(&["Mail", "Code"]), &names(&["Mail", "Code"])),
            None,
            "what the machine gives is not written"
        );
        assert_eq!(
            names_to_write(&[], &[]),
            None,
            "an empty result resets the line"
        );
        assert_eq!(
            names_to_write(&[], &names(&["Mail"])).as_deref(),
            Some("[]"),
            "an empty list says none over the machine's names"
        );
    }
}
