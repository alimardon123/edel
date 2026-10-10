//! The focused window's title (M5.4a), for presets whose bar runs along
//! the top, as Hive's: one line in the panel text's colour, cut short
//! with an ellipsis, at most 40% of the panel; nothing, and no width,
//! while no window has the keyboard. The panel draws again only when the
//! title changes.

use accesskit::Role;
use edel::i18n::{n_, trf};

use super::{Canvas, Live, Widget, no_input};

pub const WIDGET: Widget = Widget {
    name: "title",
    title: n_("Window title"),
    needs: None,
    shows,
    width,
    draw,
    input: no_input,
    parts: super::no_parts,
    role: Role::Label,
    label,
};

/// The most of the panel the title takes.
const SHARE: f32 = 0.4;

/// The focused window's title, if one is focused and not minimized.
fn shows(live: &Live) -> String {
    live.windows
        .iter()
        .find(|task| task.focused && !task.minimized)
        .map(|task| task.title.clone())
        .unwrap_or_default()
}

fn label(shown: &str) -> String {
    trf("Focused window: {title}", &[("title", shown)])
}

/// The text's size and the room on each side of it, in the pixmap's
/// pixels, as the clock's.
fn sizes(canvas: &Canvas) -> (f32, f32) {
    (
        canvas.tokens.panel_text_size as f32 * canvas.scale,
        canvas.height * 0.4,
    )
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    if shown.is_empty() {
        return 0.0;
    }
    let (size, room) = sizes(canvas);
    let most = canvas.pixmap.width() as f32 * SHARE;
    let text = canvas.text.as_deref_mut();
    (text.map_or(0.0, |t| t.line(shown, size).width) + 2.0 * room).min(most)
}

/// Centred in the panel's height, cut short to fit.
fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let (size, room) = sizes(canvas);
    let room_for_text = canvas.pixmap.width() as f32 * SHARE - 2.0 * room;
    let y = canvas.top + (canvas.height - size * 1.25) / 2.0;
    let ink = canvas.tokens.panel_text;
    if let Some(text) = canvas.text.as_deref_mut() {
        let mut line = text.fit(shown, size, room_for_text);
        text.draw(canvas.pixmap, &mut line, x + room, y, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Task;

    fn task(title: &str, focused: bool, minimized: bool) -> Task {
        Task {
            title: title.into(),
            app_id: String::new(),
            focused,
            minimized,
        }
    }

    #[test]
    fn it_shows_the_focused_windows_title_or_nothing() {
        let mut live = Live {
            windows: vec![task("Files", false, false), task("Mail", true, false)],
            ..Live::default()
        };
        assert_eq!(shows(&live), "Mail");
        live.windows[1].minimized = true;
        assert_eq!(shows(&live), "");
        live.windows.clear();
        assert_eq!(shows(&live), "");
        assert_eq!(label("Mail"), "Focused window: Mail");
    }
}
