//! The launcher (M5.3b): Super, tapped alone, or the menu button opens
//! it beside the panel's start, the preset's `[launcher] style = "menu"`.
//! A search line over the apps whose names, keywords or commands match
//! what was typed (`crate::apps`), the best first; Up and Down choose,
//! Return or a click starts one, Escape, Super again or a click
//! elsewhere closes it. It holds the keyboard, its buffers and the app
//! list only while open: shell-ui's idle memory stays the panel's.
//! Drawing is plain and tested without a display.

use std::process::{Child, Command, Stdio};

use tiny_skia::{FillRule, Pixmap, Transform};

use edel::tokens::{Colour, Tokens};

use crate::apps::{self, App};
use crate::paint::{Text, mix, paint_of, rounded};

/// Its size in logical pixels: a search line and room for `ROWS` apps,
/// the same however many match, so it never changes size as people type.
pub const WIDTH: u32 = 360;
pub const ROWS: usize = 8;
const PAD: f32 = 8.0;
const SEARCH: f32 = 40.0;
const ROW: f32 = 36.0;
pub const HEIGHT: u32 = (PAD + SEARCH + PAD + ROWS as f32 * ROW + PAD) as u32;
/// Its corners (a menu's, by the mockups) and its controls'.
const RADIUS: f32 = 12.0;
const CONTROL: f32 = 7.0;
/// Text inside a row or the search line, from its left.
const INSET: f32 = 12.0;

/// What the launcher shows: what was typed, the matching apps' names
/// and which is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub query: String,
    pub names: Vec<String>,
    pub selected: usize,
}

/// The row at `y` logical pixels from the launcher's top, if any.
pub fn row_at(y: f32) -> Option<usize> {
    let top = PAD + SEARCH + PAD;
    if y < top {
        return None;
    }
    let row = ((y - top) / ROW) as usize;
    (row < ROWS).then_some(row)
}

/// Draws `view` at `scale` into `pixmap`, `WIDTH` by `HEIGHT` times
/// `scale`.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let s = scale;
    let fill = |pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Colour| {
        if let Some(path) = rounded(x * s, y * s, w * s, h * s, r * s) {
            pixmap.fill_path(
                &path,
                &paint_of(c),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    };
    let (w, h) = (WIDTH as f32, HEIGHT as f32);
    fill(pixmap, 0.0, 0.0, w, h, RADIUS, tokens.panel);
    // The search line, a little lighter than the launcher.
    let field = mix(tokens.panel, tokens.panel_text, 0.08);
    fill(pixmap, PAD, PAD, w - 2.0 * PAD, SEARCH, CONTROL, field);
    let selected = Colour {
        a: 0.18,
        ..tokens.accent
    };
    let first = PAD + SEARCH + PAD;
    if view.selected < view.names.len() {
        let y = first + view.selected as f32 * ROW;
        fill(pixmap, PAD, y, w - 2.0 * PAD, ROW, CONTROL, selected);
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    let room = (w - 2.0 * (PAD + INSET)) * s;
    let dim = mix(tokens.panel_text, tokens.panel, 0.45);
    let (words, ink) = if view.query.is_empty() {
        ("Type to search", dim)
    } else {
        (view.query.as_str(), tokens.panel_text)
    };
    let mut line = text.fit(words, size, room);
    let middle = |top: f32, height: f32| (top + height / 2.0) * s - size * 0.625;
    text.draw(
        pixmap,
        &mut line,
        (PAD + INSET) * s,
        middle(PAD, SEARCH),
        ink,
    );
    // Where the next letter goes: after the text, or just before the
    // hint.
    let x = if view.query.is_empty() {
        (PAD + INSET - 4.0) * s
    } else {
        (PAD + INSET) * s + line.width + s
    };
    if let Some(path) = rounded(x, (PAD + 10.0) * s, 2.0 * s, (SEARCH - 20.0) * s, s) {
        pixmap.fill_path(
            &path,
            &paint_of(tokens.accent),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    if view.names.is_empty() {
        let mut line = text.fit("No app matches", size, room);
        text.draw(
            pixmap,
            &mut line,
            (PAD + INSET) * s,
            middle(first, ROW),
            dim,
        );
    }
    for (i, name) in view.names.iter().take(ROWS).enumerate() {
        let mut line = text.fit(name, size, room);
        let top = first + i as f32 * ROW;
        text.draw(
            pixmap,
            &mut line,
            (PAD + INSET) * s,
            middle(top, ROW),
            tokens.panel_text,
        );
    }
}

/// What the launcher knows while open, and the apps it started, until
/// they end.
#[derive(Default)]
pub struct Launcher {
    apps: Vec<App>,
    pub query: String,
    pub selected: usize,
    children: Vec<Child>,
}

impl Launcher {
    /// Reads the apps afresh, so one just installed is there.
    pub fn open(&mut self) {
        self.apps = apps::read_all(&apps::dirs());
        self.query.clear();
        self.selected = 0;
        self.reap();
    }

    /// Forgets the apps, to keep nothing while closed.
    pub fn close(&mut self) {
        self.apps = Vec::new();
        self.query = String::new();
        self.reap();
    }

    fn matches(&self) -> Vec<&App> {
        let mut found = apps::search(&self.apps, &self.query);
        found.truncate(ROWS);
        found
    }

    pub fn view(&self) -> View {
        let names: Vec<String> = self.matches().iter().map(|a| a.name.clone()).collect();
        View {
            selected: self.selected.min(names.len().saturating_sub(1)),
            query: self.query.clone(),
            names,
        }
    }

    /// Something typed: the search starts again from the best match.
    pub fn typed(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected = 0;
    }

    pub fn erase(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Up (-1) or down (1), staying on the list.
    pub fn step(&mut self, by: i32) {
        let last = self.matches().len().saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + by).clamp(0, last.max(0)) as usize;
    }

    /// Starts the app on `row`, else the chosen one; its name if one
    /// started.
    pub fn start(&mut self, row: Option<usize>) -> Option<String> {
        let row = row.unwrap_or(self.selected);
        let app = (*self.matches().get(row)?).clone();
        let argv = apps::command(&app);
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(home) = std::env::var_os("HOME") {
            command.current_dir(home);
        }
        match command.spawn() {
            Ok(child) => {
                self.children.push(child);
                eprintln!("edel-shell-ui: launched {} ({})", app.name, argv.join(" "));
                Some(app.name)
            }
            Err(e) => {
                eprintln!("edel-shell-ui: {} did not start: {e}", app.name);
                None
            }
        }
    }

    /// Collects the apps that ended, so none lingers as a zombie.
    pub fn reap(&mut self) {
        self.children
            .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn the_chosen_row_is_lit_and_the_corners_are_round() {
        let tokens = Tokens::built_in();
        let view = View {
            query: "fo".into(),
            names: vec!["Foot".into(), "Foot Client".into()],
            selected: 1,
        };
        let mut pixmap = Pixmap::new(WIDTH, HEIGHT).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
        let back = pixel(&pixmap, WIDTH / 2, HEIGHT - 4);
        assert_eq!(back, tokens.panel.bytes());
        // Row 1 is lit, row 0 is not; 4 px in from the left of each.
        let first = (PAD + SEARCH + PAD) as u32;
        let row = |i: u32| {
            pixel(
                &pixmap,
                PAD as u32 + 4,
                first + i * ROW as u32 + ROW as u32 / 2,
            )
        };
        assert_eq!(row(0), back);
        assert_ne!(row(1), back);
        assert_eq!(row_at(0.0), None);
        assert_eq!(row_at(PAD + SEARCH + PAD + ROW * 1.5), Some(1));
        assert_eq!(row_at(HEIGHT as f32), None);
    }

    #[test]
    fn typing_narrows_and_the_choice_stays_on_the_list() {
        let app = |name: &str| {
            apps::parse(&format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec=true\n"
            ))
            .unwrap()
        };
        let mut launcher = Launcher {
            apps: vec![app("Files"), app("Foot"), app("Text Editor")],
            ..Launcher::default()
        };
        assert_eq!(launcher.view().names.len(), 3);
        launcher.step(5);
        assert_eq!(launcher.selected, 2);
        launcher.typed("fo");
        assert_eq!(launcher.view().names, ["Foot"]);
        assert_eq!(launcher.selected, 0);
        launcher.step(1);
        assert_eq!(launcher.selected, 0, "one match");
        launcher.erase();
        assert_eq!(launcher.query, "f");
        launcher.typed("zz");
        assert!(launcher.view().names.is_empty());
        assert_eq!(launcher.start(None), None, "nothing to start");
        // `true` starts and ends at once.
        launcher.query.clear();
        assert_eq!(launcher.start(Some(1)), Some("Foot".to_string()));
        launcher.children[0].wait().unwrap();
        launcher.reap();
        assert!(launcher.children.is_empty());
        launcher.close();
        assert!(launcher.view().names.is_empty());
    }
}
