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

/// A colour as GTK's CSS reads it: `#rrggbb`, or `rgba(...)` when it is
/// not opaque.
fn css_colour(c: edel::tokens::Colour) -> String {
    if c.a >= 1.0 {
        c.hex()
    } else {
        let [r, g, b, _] = c.bytes();
        format!("rgba({r}, {g}, {b}, {:.3})", c.a)
    }
}

/// The app's stylesheet from `t`: the window and its sidebar, the search
/// field, the pages' titles, the preset cards and the grouped rows.
pub fn css(t: &Tokens) -> String {
    let window = css_colour(t.window);
    let card = css_colour(t.card);
    let line = css_colour(t.line);
    let text = css_colour(t.title_text);
    let muted = css_colour(t.title_text_unfocused);
    let accent = css_colour(t.accent);
    let accent_text = css_colour(t.accent_text);
    let shadow = css_colour(t.shadow);
    let bar = css_colour(t.title_bar_focused);
    let bar_text = text.clone();
    // Shades of the text's colour, as the mockups have them: secondary
    // text, and the fills behind a field, a value's box and a switch.
    let secondary = format!("alpha({text}, 0.72)");
    let fill = format!("alpha({text}, 0.055)");
    let fill_strong = format!("alpha({text}, 0.09)");
    // A problem reads in the close button's red.
    let error = css_colour(t.title_close_hover);
    let font = &t.font;
    let size = t.text_size;
    let small = size.saturating_sub(1).max(1);
    let tiny = format!("{:.1}", f64::from(size) - 1.5);
    let key = format!("{:.1}", f64::from(size) - 2.5);
    let heading = size * 20 / 13;
    let hero = size * 22 / 13;
    let big = size + 1;
    let name = size * 34 / 13;
    let (rw, rc, rs) = (t.radius_window, t.radius_control, t.radius_small);
    format!(
        r#"/* Written from design/tokens.toml by edel-settings (style.rs), after the mockups' own CSS. */
window.edel {{ background-color: {window}; color: {text}; font-family: "{font}"; font-size: {size}px; letter-spacing: -0.08px; }}
.edel-sidebar {{ background-color: {card}; border-right: 1px solid {line}; padding: 10px 8px; }}
.edel-search {{ margin: 0 1px 10px; min-height: 30px; padding: 0 9px; border-radius: {rc}px; border: none; box-shadow: none;
  background-color: {fill}; color: {text}; font-size: {small}px; }}
.edel-search:focus-within {{ outline: 2px solid alpha({accent}, 0.4); outline-offset: 0; }}
.edel-search image {{ color: {muted}; }}
.edel-pages {{ background: none; }}
.edel-pages > row {{ min-height: 28px; margin: 0 0 1px; padding: 0 9px; border-radius: {rs}px; color: {text}; background: none; }}
.edel-pages > row:hover {{ background-color: {fill}; }}
.edel-pages > row:selected {{ background-color: {accent}; color: {accent_text}; font-weight: 550; }}
.edel-pages > row:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: -2px; }}
.edel-back {{ margin: 10px 0 0 20px; padding: 2px 8px 2px 4px; border-radius: {rs}px; background: none; box-shadow: none; color: {accent}; }}
.edel-back:hover {{ background-color: {fill}; }}
.edel-page-title {{ font-size: {heading}px; font-weight: 650; letter-spacing: -0.4px; }}
.edel-intro {{ color: {secondary}; margin-top: 4px; }}
.edel-error {{ color: {error}; margin-top: 8px; }}
.edel-section {{ font-weight: 600; font-size: {small}px; color: {secondary}; }}
.edel-source {{ color: {muted}; font-size: {small}px; }}
.edel-link {{ color: {accent}; font-size: {small}px; padding: 0 4px; min-height: 0; min-width: 0;
  background: none; box-shadow: none; border: none; }}
.edel-link:hover {{ background-color: alpha({accent}, 0.1); }}
.edel-copy {{ min-height: 24px; min-width: 24px; padding: 0; border-radius: {rs}px; background: none; box-shadow: none; color: {muted}; opacity: 0.45; }}
.edel-copy:hover, .edel-copy:focus-visible {{ opacity: 1; background-color: {fill}; }}
.edel-row .edel-copy {{ opacity: 0; }}
.edel-row:hover .edel-copy {{ opacity: 0.45; }}
.edel-row .edel-copy:hover, .edel-row .edel-copy:focus-visible {{ opacity: 1; }}
.edel-preset {{ padding: 7px; border-radius: {rw}px; background-image: none; background-color: {card};
  border: 1px solid {line}; box-shadow: none; color: {text}; }}
.edel-preset:hover {{ background-color: {card}; border-color: alpha({accent}, 0.45); }}
.edel-preset:checked {{ background-color: {card}; color: {text}; border-color: {accent}; box-shadow: 0 0 0 1px {accent}; }}
.edel-preset:focus-visible {{ outline: 2px solid alpha({accent}, 0.45); outline-offset: 2px; }}
.edel-preset-name {{ font-weight: 550; font-size: {small}px; margin-left: 2px; }}
.edel-badge {{ background-color: {accent}; color: {accent_text}; border-radius: 999px; min-width: 14px; min-height: 14px; margin-right: 2px; }}
.edel-badge image {{ margin: 1px; }}
.edel-group {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; }}
.edel-row {{ padding: 6px 14px; min-height: 34px; }}
.edel-row + .edel-row {{ border-top: 1px solid {line}; }}
.edel-row-subtitle {{ color: {muted}; font-size: {tiny}px; }}
.edel-key {{ font-size: {key}px; font-weight: 500; color: {secondary}; padding: 0 5px; min-height: 15px; min-width: 9px;
  border-radius: {rs}px; background-color: {fill}; border: 1px solid {line}; border-bottom-width: 2px; }}
.edel-value {{ color: {secondary}; }}
.edel-preview-row {{ padding: 18px 14px; }}
.edel-mini-window {{ background-color: {window}; border: 1px solid {line}; border-radius: {rw}px;
  box-shadow: 0 1px 2px alpha({shadow}, 0.15), 0 8px 18px -10px alpha({shadow}, 0.35); }}
.edel-bar {{ min-height: 28px; padding: 0 5px; background-color: {bar}; border-bottom: 1px solid {line};
  border-radius: {rw}px {rw}px 0 0; }}
.edel-bar label {{ font-weight: 500; font-size: {small}px; color: {bar_text}; }}
.edel-bar-button {{ min-width: 24px; min-height: 20px; border-radius: {rs}px; color: {secondary}; }}
.edel-bar-button image {{ margin: 4px 6px; }}
.edel-mini-body {{ min-height: 40px; }}
.edel switch {{ min-width: 36px; min-height: 21px; padding: 0; border: none; border-radius: 11px; background-image: none;
  background-color: {fill_strong}; box-shadow: none; }}
.edel switch:hover {{ background-color: alpha({text}, 0.12); }}
.edel switch:checked {{ background-color: {accent}; }}
.edel switch > slider {{ min-width: 17px; min-height: 17px; margin: 2px; border-radius: 999px; border: none; background-image: none;
  background-color: #ffffff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.08), 0 1px 2px rgba(0, 0, 0, 0.2); }}
.edel switch image {{ opacity: 0; }}
.edel scale {{ min-height: 20px; }}
.edel scale trough {{ min-height: 5px; border-radius: 999px; border: none; background-image: none; background-color: {fill_strong}; }}
.edel scale highlight {{ border-radius: 999px; border: none; background-image: none; background-color: {accent}; }}
.edel scale slider {{ min-width: 17px; min-height: 17px; margin: -6px; border-radius: 999px; border: none; background-image: none;
  background-color: #ffffff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.08), 0 1px 2px rgba(0, 0, 0, 0.2); }}
.edel scale:focus-visible slider {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 1px; }}
.edel-action {{ min-height: 26px; padding: 0 12px; border-radius: {rs}px; background-image: none; background-color: {fill};
  box-shadow: none; border: none; color: {text}; font-weight: 500; }}
.edel-action:hover {{ background-color: {fill_strong}; }}
.edel-action:disabled {{ opacity: 0.5; }}
.edel-action:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 1px; }}
.edel-action-main {{ background-color: {accent}; color: {accent_text}; font-weight: 550; }}
.edel-action-main:hover {{ background-color: alpha({accent}, 0.88); }}
.edel-output, .edel-output.edel-error {{ font-size: {tiny}px; margin-top: 0; }}
.edel-choice {{ background: none; box-shadow: none; padding: 0; }}
.edel-choice > button {{ min-height: 26px; padding: 0 8px 0 10px; border-radius: {rs}px; background-image: none;
  background-color: {fill}; box-shadow: none; border: none; color: {text}; font-weight: 400; }}
.edel-choice > button:hover, .edel-choice > button:checked {{ background-color: {fill_strong}; }}
.edel-choice > button label {{ font-weight: 400; }}
.edel-choice image {{ color: {secondary}; }}
popover.edel-choice-list > contents {{ padding: 4px; border-radius: {rc}px; background-color: {window};
  box-shadow: 0 0 0 1px {line}, 0 8px 22px -8px alpha({shadow}, 0.5); }}
.edel-choices {{ background: none; }}
.edel-choices > row {{ min-height: 26px; padding: 0 10px 0 6px; border-radius: {rs}px; color: {text}; }}
.edel-choices > row:hover {{ background-color: {accent}; color: {accent_text}; }}
.edel-segments {{ background-color: {fill}; border-radius: {rs}px; padding: 2px; }}
.edel-segment {{ min-height: 22px; min-width: 38px; padding: 0 8px; border-radius: {rs}px; background-image: none;
  background-color: transparent; box-shadow: none; border: none; color: {text}; font-weight: 400; }}
.edel-segment:hover {{ background-color: {fill_strong}; }}
.edel-segment:checked {{ background-color: {accent}; color: {accent_text}; font-weight: 550; }}
.edel-segment:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 1px; }}
.edel-spin {{ min-height: 26px; border-radius: {rs}px; background-image: none; background-color: {fill};
  box-shadow: none; border: none; color: {text}; }}
.edel-spin > button {{ background: none; box-shadow: none; border: none; color: {text}; }}
.edel-arrangement {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; }}
.edel-hero {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; padding: 24px 26px 26px; }}
.edel-hero-icon {{ color: {accent}; }}
.edel-hero-icon.edel-problem {{ color: {error}; }}
.edel-hero-title {{ font-size: {hero}px; font-weight: 650; letter-spacing: -0.4px; }}
.edel-hero-sub {{ color: {secondary}; }}
.edel-buttons {{ background: none; }}
.edel-buttons > flowboxchild {{ padding: 0; margin: 0; background: none; border-radius: {rc}px; }}
.edel-buttons > flowboxchild:focus-visible {{ outline: none; }}
.edel-big {{ min-height: 44px; min-width: 168px; padding: 0 24px; border-radius: {rc}px; background-image: none; background-color: transparent;
  box-shadow: none; border: 1px solid alpha({text}, 0.22); color: {text}; font-size: {big}px; font-weight: 550; }}
.edel-big:hover {{ background-color: {fill}; }}
.edel-big:disabled {{ opacity: 0.5; }}
.edel-big:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 2px; }}
.edel-big-main {{ background-color: {accent}; color: {accent_text}; border-color: transparent; font-weight: 600; }}
.edel-big-main:hover {{ background-color: alpha({accent}, 0.88); }}
.edel-name {{ font-size: {name}px; font-weight: 650; letter-spacing: -0.6px; }}
.edel-name-version {{ color: {secondary}; font-size: {big}px; }}
.edel-details > title {{ margin: 0 0 10px; padding: 4px 0; }}
.edel-details > title label {{ font-weight: 600; font-size: {small}px; color: {secondary}; }}
.edel-details > title arrow {{ color: {muted}; }}
.edel-change-bar {{ padding: 10px 20px 10px 32px; background-color: {card}; color: {text}; border-top: 1px solid {line}; }}
.edel-change-mark {{ min-width: 22px; min-height: 22px; border-radius: 999px; background-color: {accent}; color: {accent_text}; }}
.edel-change-text {{ font-weight: 600; }}
.edel-change-detail {{ color: {muted}; font-size: {tiny}px; }}
.edel-change-undo, .edel-change-keep {{ min-height: 28px; padding: 0 14px; border-radius: {rc}px; background-image: none; box-shadow: none; border: none; }}
.edel-change-undo {{ background-color: {fill}; color: {text}; }}
.edel-change-undo:hover {{ background-color: {fill_strong}; }}
.edel-change-keep {{ background-color: {accent}; color: {accent_text}; font-weight: 550; }}
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
        assert!(css.contains(&format!("background-color: {};", css_colour(t.window))));
        assert!(css.contains(&format!("font-family: \"{}\";", t.font)));
        // A changed token changes the sheet: no colour is written here.
        t.accent = edel::tokens::Colour::parse("#e5484d").unwrap();
        t.radius_window = 3;
        let changed = super::css(&t);
        assert!(changed.contains("background-color: #e5484d;"));
        assert!(changed.contains("border-radius: 3px;"));
        assert!(!changed.contains(&Tokens::built_in().accent.hex()));
    }
}
