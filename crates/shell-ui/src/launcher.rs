//! The launcher (M5.3b): Super, tapped alone, or the menu button opens
//! it beside the panel's start, the preset's `[launcher] style = "menu"`.
//! A search line over the apps whose names, keywords or commands match
//! what was typed (`crate::apps`), the best first; Up and Down choose,
//! Return or a click starts one, Escape, Super again or a click
//! elsewhere closes it. It holds the keyboard, its buffers and the app
//! list only while open: shell-ui's idle memory stays the panel's.
//! Drawing is plain and tested without a display.

use std::process::{Child, Command, Stdio};

use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::apps::{self, App};
use crate::paint::{Text, fill, mix};
use crate::popup::{self, INSET, PAD, middle};

/// Its width in logical pixels, its search line's height and how many
/// apps it shows: the same however many match, so it never changes size
/// as people type.
pub const WIDTH: u32 = 360;
pub const ROWS: usize = 8;
const SEARCH: f32 = 40.0;

/// Its size in logical pixels: the search line and `ROWS` rows.
pub fn size(tokens: &Tokens) -> (u32, u32) {
    let rows = ROWS as f32 * tokens.row as f32;
    (WIDTH, (PAD + SEARCH + PAD + rows + PAD) as u32)
}

/// What the launcher shows: what was typed, the matching apps' names
/// and which is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub query: String,
    pub names: Vec<String>,
    pub selected: usize,
}

/// The row at `y` logical pixels from the launcher's top, if any.
pub fn row_at(y: f32, tokens: &Tokens) -> Option<usize> {
    let top = PAD + SEARCH + PAD;
    if y < top {
        return None;
    }
    let row = ((y - top) / tokens.row as f32) as usize;
    (row < ROWS).then_some(row)
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    popup::card(pixmap, tokens, s);
    let w = WIDTH as f32;
    // The search line, a little lighter than the launcher.
    let field = mix(tokens.panel, tokens.panel_text, 0.08);
    let r = tokens.radius_control as f32 * s;
    fill(
        pixmap,
        PAD * s,
        PAD * s,
        (w - 2.0 * PAD) * s,
        SEARCH * s,
        r,
        field,
    );
    let first = PAD + SEARCH + PAD;
    let chosen = (view.selected < view.names.len()).then_some(view.selected);
    let names = view.names.iter().take(ROWS).map(String::as_str);
    popup::rows(pixmap, tokens, text.as_deref_mut(), names, first, chosen, s);
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
    let left = (PAD + INSET) * s;
    text.draw(pixmap, &mut line, left, middle(PAD, SEARCH, size, s), ink);
    // Where the next letter goes: after the text, or just before the
    // hint.
    let x = if view.query.is_empty() {
        left - 4.0 * s
    } else {
        left + line.width + s
    };
    let (y, h) = ((PAD + 10.0) * s, (SEARCH - 20.0) * s);
    fill(pixmap, x, y, 2.0 * s, h, s, tokens.accent);
    if view.names.is_empty() {
        let mut line = text.fit("No app matches", size, room);
        let y = middle(first, tokens.row as f32, size, s);
        text.draw(pixmap, &mut line, left, y, dim);
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

    /// How many apps it offers.
    pub fn count(&self) -> usize {
        self.apps.len()
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
        let (_, height) = size(&tokens);
        let high = tokens.row;
        let mut pixmap = Pixmap::new(WIDTH, height).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
        let back = pixel(&pixmap, WIDTH / 2, height - 4);
        assert_eq!(back, tokens.panel.bytes());
        // Row 1 is lit, row 0 is not; 4 px in from the left of each.
        let first = (PAD + SEARCH + PAD) as u32;
        let row = |i: u32| pixel(&pixmap, PAD as u32 + 4, first + i * high + high / 2);
        assert_eq!(row(0), back);
        assert_ne!(row(1), back);
        assert_eq!(row_at(0.0, &tokens), None);
        let y = PAD + SEARCH + PAD + high as f32 * 1.5;
        assert_eq!(row_at(y, &tokens), Some(1));
        assert_eq!(row_at(height as f32, &tokens), None);
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
