//! Settings' look (M5.6a): one stylesheet written from the design tokens
//! (`design/tokens.toml`, through `edel::tokens`) when the app starts and
//! again when the desktop turns light or dark, so every colour, corner,
//! the font and the text's size are the tokens', and a changed token
//! changes the app with no line here. Only spacing, which is the layout's
//! own, is written here.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use edel::tokens::{Scheme, Tokens};

/// The tokens the app is drawn with now, and what to draw again when
/// they change.
pub struct Theme {
    tokens: RefCell<Tokens>,
    provider: gtk::CssProvider,
    watchers: RefCell<Vec<Box<dyn Fn()>>>,
}

impl Theme {
    /// The tokens in the scheme the desktop shows, as GTK learns it from
    /// the settings portal (M5.5c), following it when it changes.
    pub fn new() -> Rc<Theme> {
        let manager = adw::StyleManager::default();
        let theme = Rc::new(Theme {
            tokens: RefCell::new(load(manager.is_dark())),
            provider: gtk::CssProvider::new(),
            watchers: RefCell::new(Vec::new()),
        });
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &theme.provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        theme
            .provider
            .load_from_string(&css(&theme.tokens.borrow()));
        let weak = Rc::downgrade(&theme);
        manager.connect_dark_notify(move |manager| {
            if let Some(theme) = weak.upgrade() {
                theme.change(manager.is_dark());
            }
        });
        theme
    }

    fn change(&self, dark: bool) {
        *self.tokens.borrow_mut() = load(dark);
        self.provider.load_from_string(&css(&self.tokens.borrow()));
        for watcher in self.watchers.borrow().iter() {
            watcher();
        }
    }

    /// The tokens now.
    pub fn tokens(&self) -> Ref<'_, Tokens> {
        self.tokens.borrow()
    }

    /// Calls `f` whenever the tokens change, as a drawing that reads
    /// them must be drawn again.
    pub fn watch(&self, f: impl Fn() + 'static) {
        self.watchers.borrow_mut().push(Box::new(f));
    }
}

/// The image's tokens in the dark or the light scheme; what could not be
/// read is said on standard error, and the built-in value is used.
fn load(dark: bool) -> Tokens {
    let scheme = if dark { Scheme::Dark } else { Scheme::Light };
    let (tokens, notes) = edel::tokens::load(scheme);
    for note in notes {
        eprintln!("edel-settings: design tokens: {note}");
    }
    tokens
}

/// The app's stylesheet from `t`: the window and its sidebar, the search
/// field, the pages' titles, the preset cards and the grouped rows.
pub fn css(t: &Tokens) -> String {
    let window = t.window.hex();
    let card = t.card.hex();
    let line = t.line.hex();
    let sidebar = t.panel.hex();
    let text = t.title_text.hex();
    let muted = t.title_text_unfocused.hex();
    let accent = t.accent.hex();
    let shadow = t.shadow.hex();
    // A problem reads in the close button's red.
    let error = t.title_close_hover.hex();
    let font = &t.font;
    let size = t.text_size;
    let small = size.saturating_sub(1).max(1);
    let heading = size * 17 / 10;
    let (rw, rm, rc) = (t.radius_window, t.radius_menu, t.radius_control);
    format!(
        r#"/* Written from design/tokens.toml by edel-settings (style.rs). */
window.edel {{ background-color: {window}; color: {text}; font-family: "{font}"; font-size: {size}px; }}
.edel-sidebar {{ background-color: {sidebar}; border-right: 1px solid {line}; }}
.edel-search {{ margin: 12px 12px 6px; min-height: 30px; padding: 0 8px; border-radius: {rc}px;
  background-color: {card}; border: 1px solid {line}; box-shadow: none; color: {text}; }}
.edel-search:focus-within {{ border-color: {accent}; outline: 2px solid alpha({accent}, 0.35); outline-offset: 0; }}
.edel-search image {{ color: {muted}; }}
.edel-pages {{ background: none; padding: 4px 0; }}
.edel-pages > row {{ margin: 1px 10px; padding: 6px 10px; border-radius: {rc}px; color: {text}; background: none; }}
.edel-pages > row:hover {{ background-color: alpha({text}, 0.06); }}
.edel-pages > row:selected {{ background-color: {accent}; color: #ffffff; }}
.edel-pages > row:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: -2px; }}
.edel-back {{ margin: 10px 0 0 20px; padding: 2px 8px 2px 4px; border-radius: {rc}px; background: none; box-shadow: none; color: {accent}; }}
.edel-back:hover {{ background-color: alpha({accent}, 0.1); }}
.edel-page-title {{ font-size: {heading}px; font-weight: 650; letter-spacing: -0.2px; }}
.edel-intro {{ color: {muted}; margin-top: 6px; }}
.edel-error {{ color: {error}; margin-top: 8px; }}
.edel-section {{ font-weight: 600; }}
.edel-source {{ color: {muted}; font-size: {small}px; }}
.edel-link {{ color: {accent}; font-size: {small}px; padding: 0 4px; min-height: 0; min-width: 0;
  background: none; box-shadow: none; border: none; }}
.edel-link:hover {{ background-color: alpha({accent}, 0.1); }}
.edel-copy {{ min-height: 24px; min-width: 24px; padding: 0; border-radius: {rc}px; background: none; box-shadow: none; color: {muted}; opacity: 0.45; }}
.edel-copy:hover, .edel-copy:focus-visible {{ opacity: 1; background-color: alpha({text}, 0.07); }}
.edel-preset {{ padding: 6px 6px 9px; border-radius: {rm}px; background-image: none; background-color: {card};
  border: 1px solid {line}; box-shadow: 0 1px 2px alpha({shadow}, 0.25); color: {text}; }}
.edel-preset:hover {{ border-color: alpha({accent}, 0.55); }}
.edel-preset:checked {{ border: 2px solid {accent}; padding: 5px 5px 8px; background-color: {card}; color: {text}; }}
.edel-preset:focus-visible {{ outline: 2px solid alpha({accent}, 0.45); outline-offset: 2px; }}
.edel-preset-name {{ font-weight: 500; margin-left: 4px; }}
.edel-badge {{ background-color: {accent}; color: #ffffff; border-radius: 999px; min-width: 18px; min-height: 18px; margin-right: 2px; }}
.edel-badge image {{ margin: 3px; }}
.edel-group {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; box-shadow: 0 1px 2px alpha({shadow}, 0.2); }}
.edel-row {{ padding: 10px 14px; min-height: 38px; }}
.edel-row + .edel-row {{ border-top: 1px solid {line}; }}
.edel-row-title {{ font-weight: 500; }}
.edel-row-subtitle {{ color: {muted}; font-size: {small}px; }}
.edel-value {{ color: {muted}; }}
.edel switch {{ border-radius: 999px; }}
.edel switch:checked {{ background-color: {accent}; }}
.edel dropdown > button {{ border-radius: {rc}px; min-height: 28px; padding: 0 8px; font-weight: 500;
  background-color: alpha({text}, 0.06); box-shadow: none; }}
.edel dropdown > button:hover {{ background-color: alpha({text}, 0.1); }}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stylesheet_is_the_tokens() {
        let mut t = Tokens::built_in();
        let css = css(&t);
        assert!(css.contains(&format!("background-color: {};", t.window.hex())));
        assert!(css.contains(&format!("font-family: \"{}\";", t.font)));
        // A changed token changes the sheet: no colour is written here.
        t.accent = edel::tokens::Colour::parse("#e5484d").unwrap();
        t.radius_menu = 3;
        let changed = super::css(&t);
        assert!(changed.contains("background-color: #e5484d;"));
        assert!(changed.contains("border-radius: 3px;"));
        assert!(!changed.contains(&Tokens::built_in().accent.hex()));
    }
}
