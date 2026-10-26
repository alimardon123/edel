//! Settings' look (M5.6a): one stylesheet written from the design tokens
//! (`design/tokens.toml`, through `edel::tokens`) when the app starts and
//! again when the desktop turns light or dark, so every colour, corner,
//! the font and the text's size are the tokens', and a changed token
//! changes the app with no line here. Only spacing, which is the layout's
//! own, is written here.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use edel::tokens::{Scheme, Tokens};
use gtk::gio;
use gtk::prelude::*;

/// The tokens the app is drawn with now, and what to draw again when
/// they change.
pub struct Theme {
    tokens: RefCell<Tokens>,
    provider: gtk::CssProvider,
    watchers: RefCell<Vec<Box<dyn Fn()>>>,
    /// The settings files, watched so that an accent or a colour changed in
    /// them reads the tokens again (M5.12b1).
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

impl Theme {
    /// The tokens in the scheme the desktop shows, as GTK learns it from
    /// the settings portal (M5.5c), following it when it changes, and the
    /// person's accent and colours as the settings files give them
    /// (M5.12b1).
    pub fn new() -> Rc<Theme> {
        let manager = adw::StyleManager::default();
        let theme = Rc::new(Theme {
            tokens: RefCell::new(load(manager.is_dark())),
            provider: gtk::CssProvider::new(),
            watchers: RefCell::new(Vec::new()),
            monitors: RefCell::new(Vec::new()),
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
            .load_from_string(&css(&theme.tokens.borrow(), manager.is_dark()));
        let weak = Rc::downgrade(&theme);
        manager.connect_dark_notify(move |manager| {
            if let Some(theme) = weak.upgrade() {
                theme.change(manager.is_dark());
            }
        });
        *theme.monitors.borrow_mut() = watch_files(&theme);
        theme
    }

    /// Reads the tokens in `dark` or the light scheme again, and writes the
    /// stylesheet and calls the watchers when they differ (M5.12b1); a
    /// change that leaves them as they were does nothing.
    fn change(&self, dark: bool) {
        let tokens = load(dark);
        if tokens == *self.tokens.borrow() {
            return;
        }
        *self.tokens.borrow_mut() = tokens;
        self.provider
            .load_from_string(&css(&self.tokens.borrow(), dark));
        for watcher in self.watchers.borrow().iter() {
            watcher();
        }
    }

    /// [`Theme::change`] in the scheme the portal says now (M5.12b1).
    fn reload(&self) {
        self.change(adw::StyleManager::default().is_dark());
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

/// Watches the machine's settings file and the person's, as Settings'
/// other pages do, so a change to the accent or the colours reads the
/// tokens again (M5.12b1); a file that cannot be watched is not followed.
fn watch_files(theme: &Rc<Theme>) -> Vec<gio::FileMonitor> {
    let mut monitors = Vec::new();
    let machine = Some(edel::places::machine_settings());
    let person = edel::places::person_settings();
    for path in [machine, person].into_iter().flatten() {
        let file = gio::File::for_path(edel::places::found(&path));
        if let Ok(monitor) =
            file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            let weak = Rc::downgrade(theme);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(theme) = weak.upgrade() {
                    theme.reload();
                }
            });
            monitors.push(monitor);
        }
    }
    monitors
}

/// The image's tokens in the dark or the light scheme, as this machine's
/// people see them (M5.12b1); what could not be read is said on standard
/// error, and the built-in value is used.
pub fn load(dark: bool) -> Tokens {
    let scheme = if dark { Scheme::Dark } else { Scheme::Light };
    let (tokens, notes) = edel::tokens::on_this_machine(scheme);
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

/// The app's stylesheet from `t`, drawn in the dark or the light scheme
/// `dark` names: the window and its sidebar, the search field, the pages'
/// titles, the preset cards and the grouped rows.
pub fn css(t: &Tokens, dark: bool) -> String {
    let window = css_colour(t.window);
    let card = css_colour(t.card);
    let line = css_colour(t.line);
    let text = css_colour(t.title_text);
    let muted = css_colour(t.title_text_unfocused);
    let accent = css_colour(t.accent);
    let accent_text = css_colour(t.accent_text);
    let shadow = css_colour(t.shadow);
    // The badge over a lock's picture, in the lock's own colours.
    let deep = css_colour(t.backdrop_deep);
    let deep_text = css_colour(t.backdrop_text);
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
    // Touch mode's rows (M9.6), a finger's target.
    let target = edel::touch::TARGET;
    let size = t.text_size;
    // The Power page's figure (M5.32): the large charge, as the board draws it.
    let power = size * 28 / 13;
    let small = size.saturating_sub(1).max(1);
    let tiny = format!("{:.1}", f64::from(size) - 1.5);
    let key = format!("{:.1}", f64::from(size) - 2.5);
    // The board's 12.5 px and 9.5 px words: the text size less half a point and less 3.5.
    let caption = format!("{:.1}", f64::from(size) - 0.5);
    let badge = format!("{:.1}", f64::from(size) - 3.5);
    let badge_deep = css_colour(t.lock_backdrop_deep);
    let heading = size * 20 / 13;
    let hero = size * 22 / 13;
    let current = size * 17 / 13;
    let big = size + 1;
    let name = size * 34 / 13;
    let (rw, rc, rs) = (t.radius_window, t.radius_control, t.radius_small);
    // A frame's corners outside its picture's: the window's radius plus its padding and border.
    let rw_plus = rw + 4;
    // A look's picture corners and its ring (M5.10c7b): the menus' radius.
    let rm = t.radius_menu;
    // The accents the Updates page's badge and the Users page's avatars
    // take, each in the scheme drawn: green is ok, yellow is a restart's
    // amber and red a problem's (the accent's own shade otherwise).
    let scheme = if dark {
        edel::tokens::Scheme::Dark
    } else {
        edel::tokens::Scheme::Light
    };
    let named = |name: &str| {
        edel::tokens::accent_named(name, scheme).map_or_else(|| accent.clone(), css_colour)
    };
    let (ok, amber, problem) = (named("green"), named("yellow"), named("red"));
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
/* A group's heading (M5.32b): not selectable, so no fill, the small text, semibold, with 6 px below and 18 px above unless it is first. */
.edel-pages > row.edel-heading {{ min-height: 0; margin: 0 0 6px; padding: 0 9px; background: none; color: {secondary};
  font-size: {small}px; font-weight: 600; }}
.edel-pages > row.edel-heading:hover {{ background: none; }}
.edel-pages > row.edel-apart {{ margin-top: 18px; }}
/* The installer's row (M6.6a) stands out while it is not chosen: accent text on an accent wash, and 10 px more below it. Its icon is drawn in the text's colour, so it turns accent too. */
.edel-pages > row.edel-install-row:not(:selected) {{ background-color: alpha({accent}, 0.12); color: {accent}; font-weight: 600; }}
.edel-pages > row.edel-install-row {{ margin-bottom: 10px; }}
.edel-back {{ margin: 10px 0 0 20px; padding: 2px 8px 2px 4px; border-radius: {rs}px; background: none; box-shadow: none; color: {accent}; }}
.edel-back:hover {{ background-color: {fill}; }}
.edel-page-title {{ font-size: {heading}px; font-weight: 700; letter-spacing: -0.5px; }}
.edel-intro {{ color: {secondary}; margin-top: 4px; margin-bottom: 8px; }}
.edel-error {{ color: {error}; margin-top: 8px; }}
.edel-section {{ font-weight: 600; font-size: {small}px; color: {secondary}; }}
.edel-step-number {{ background-color: {accent}; color: {accent_text}; border-radius: 11px; font-size: {small}px; font-weight: 700; min-width: 22px; min-height: 22px; }}
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
/* A look on the Lock screen page (M5.10c7b): no card around the picture; the chosen one is ringed 3 px outside it, a hovered one thinly. */
.edel-look-card, .edel-look-card:hover, .edel-look-card:checked, .edel-look-card:active {{ padding: 0; border: none; border-radius: {rm}px; background: none; background-image: none; box-shadow: none; color: {text}; }}
.edel-look-card:focus-visible {{ outline: 2px solid alpha({accent}, 0.45); outline-offset: 2px; }}
.edel-look-picture {{ border-radius: {rm}px; }}
.edel-look-shot {{ border-radius: {rm}px; }}
.edel-look-card:hover .edel-look-picture {{ outline: 1px solid alpha({accent}, 0.35); outline-offset: 3px; }}
.edel-look-card:checked .edel-look-picture {{ outline: 2px solid {accent}; outline-offset: 3px; }}
.edel-look-name {{ font-size: {small}px; font-weight: 500; }}
.edel-look-card:checked .edel-look-name {{ font-weight: 600; }}
.edel-wallpaper-thumb {{ border-radius: {rs}px; }}
.edel-badge {{ background-color: {accent}; color: {accent_text}; border-radius: 999px; min-width: 14px; min-height: 14px; margin-right: 2px; }}
.edel-badge image {{ margin: 1px; }}
.edel-group {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; }}
.edel-row {{ padding: 6px 14px; min-height: 34px; }}
.edel-touch .edel-row {{ padding: 8px 16px; min-height: {target}px; }}
.edel-touch .edel-pages > row {{ min-height: {target}px; margin-bottom: 4px; }}
.edel-row + .edel-row {{ border-top: 1px solid {line}; }}
.edel-row-subtitle {{ color: {muted}; font-size: {tiny}px; }}
.edel-row-subtitle.edel-error {{ color: {error}; margin-top: 0; }}
.edel-row-subtitle.edel-warn {{ color: {amber}; }}
.edel-key {{ font-size: {key}px; font-weight: 500; color: {secondary}; padding: 0 5px; min-height: 15px; min-width: 9px;
  border-radius: {rs}px; background-color: {fill}; border: 1px solid {line}; border-bottom-width: 2px; }}
.edel-value {{ color: {secondary}; }}
.edel-preview-row {{ padding: 18px 14px; }}
.edel-shape-line {{ background-color: {line}; min-height: 1px; }}
.edel-shape-actions .edel-copy {{ opacity: 0; }}
.edel-shape-actions.edel-hover .edel-copy {{ opacity: 0.45; }}
.edel-shape-actions .edel-copy:hover, .edel-shape-actions .edel-copy:focus-visible {{ opacity: 1; }}
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
.edel-action.edel-danger {{ color: {error}; font-weight: 550; }}
.edel-action.edel-danger:hover {{ background-color: alpha({error}, 0.12); }}
.edel-prompt-title {{ font-size: {big}px; font-weight: 650; }}
.edel-code {{ font-size: {hero}px; font-weight: 650; letter-spacing: 6px; font-feature-settings: "tnum"; margin: 10px 0 6px; }}
.edel-prompt-detail {{ color: {secondary}; }}
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
.edel-segments-tight > .edel-segment {{ min-width: 24px; padding: 0 4px; }}
.edel-segment:hover {{ background-color: {fill_strong}; }}
.edel-segment:checked {{ background-color: {accent}; color: {accent_text}; font-weight: 550; }}
.edel-segment:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 1px; }}
.edel-spin {{ min-height: 26px; border-radius: {rs}px; background-image: none; background-color: {fill};
  box-shadow: none; border: none; color: {text}; }}
.edel-spin > button {{ background: none; box-shadow: none; border: none; color: {text}; }}
.edel-arrangement {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; }}
.edel-hero {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; padding: 24px 26px 26px; }}
.edel-hero.edel-hero-bare {{ background: none; border: none; padding: 8px 0 4px; }}
.edel-hero-title {{ font-size: {hero}px; font-weight: 650; letter-spacing: -0.4px; }}
.edel-hero-sub {{ color: {secondary}; }}
.edel-buttons {{ background: none; }}
.edel-buttons > flowboxchild {{ padding: 0; margin: 0; background: none; border-radius: {rc}px; }}
.edel-buttons > flowboxchild:focus-visible {{ outline: none; }}
.edel-swatch {{ background: none; background-image: none; box-shadow: none; border: none; padding: 0; min-width: 0; min-height: 0; border-radius: {rc}px; }}
.edel-swatch:hover, .edel-swatch:checked {{ background: none; background-image: none; }}
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
.edel-glance {{ background-color: {card}; border: 1px solid {line}; border-radius: {rw}px; padding: 20px 22px; }}
.edel-glance-icon {{ color: {accent}; }}
.edel-glance-icon.edel-off {{ color: {muted}; }}
.edel-glance-title {{ font-size: {heading}px; font-weight: 650; letter-spacing: -0.3px; }}
.edel-glance-sub {{ color: {secondary}; }}
.edel-bars {{ min-width: 20px; }}
.edel-bar-part {{ min-width: 3px; border-radius: 1px; background-color: {fill_strong}; }}
.edel-bar-part.edel-lit {{ background-color: {accent}; }}
.edel-power-card {{ background-color: {card}; border: 1px solid {line}; border-radius: {rm}px; padding: 20px; }}
.edel-battery {{ border: 2px solid {text}; border-radius: 7px; }}
.edel-battery-nub {{ background-color: {text}; border-radius: 0 2px 2px 0; }}
.edel-power-fill {{ border-radius: 3px; background-color: {fill_strong}; }}
.edel-power-fill.edel-ok {{ background-color: {ok}; }}
.edel-power-fill.edel-low {{ background-color: {amber}; }}
.edel-power-fill.edel-empty {{ background-color: {problem}; }}
.edel-power-percent {{ font-size: {power}px; font-weight: 650; letter-spacing: -0.5px; }}
.edel-power-sub {{ color: {secondary}; }}
.edel-power-dim {{ color: {secondary}; font-size: {caption}px; }}
.edel-power-health {{ font-size: {current}px; font-weight: 650; }}
.edel-power-axis {{ color: {secondary}; font-size: {caption}px; }}
.edel-day-bar {{ border-radius: {rs}px {rs}px 0 0; background-color: alpha({accent}, 0.3); }}
.edel-day-bar.edel-charging {{ background-color: alpha({ok}, 0.4); }}
.edel-day-bar.edel-low {{ background-color: alpha({problem}, 0.4); }}
.edel-day-bar.edel-gap {{ background-color: {fill_strong}; }}
.edel-day-bar.edel-now {{ background-color: {accent}; }}
.edel-day-bar.edel-now.edel-charging {{ background-color: {ok}; }}
.edel-lock {{ color: {muted}; }}
.edel-entry {{ min-height: 26px; padding: 0 8px; border-radius: {rs}px; background-image: none; background-color: {fill};
  box-shadow: none; border: none; color: {text}; }}
.edel-entry:focus-within {{ outline: 2px solid alpha({accent}, 0.4); outline-offset: 0; }}
.edel-entry text {{ background: none; color: {text}; }}
.edel-fold > title {{ margin: 0 0 10px; padding: 4px 0; }}
.edel-fold > title label {{ font-weight: 600; font-size: {small}px; color: {secondary}; }}
.edel-fold > title arrow {{ color: {muted}; }}
.edel-current {{ padding: 16px; }}
.edel-current-title {{ font-size: {current}px; font-weight: 600; letter-spacing: -0.3px; }}
.edel-current-button {{ min-height: 34px; padding: 0 16px; border-radius: 999px; }}
.edel-in-use {{ background-color: {accent}; color: {accent_text}; font-size: {tiny}px; font-weight: 600;
  border-radius: 999px; padding: 2px 9px 2px 7px; box-shadow: 0 1px 2px alpha({accent}, 0.35); }}
.edel-built-in {{ background-color: alpha({deep}, 0.55); color: {deep_text}; font-size: {tiny}px; font-weight: 600;
  border-radius: 999px; padding: 2px 8px 2px 7px; box-shadow: inset 0 0 0 1px alpha({deep_text}, 0.22); }}
.edel-new-card {{ padding: 0; border-radius: {rw}px; background: none; background-image: none; box-shadow: none; border: none; }}
.edel-new-card:focus-visible {{ outline: 2px solid alpha({accent}, 0.45); outline-offset: 2px; }}
.edel-new-look {{ border: 1.5px dashed {line}; border-radius: {rw}px; }}
.edel-new-card:hover .edel-new-look {{ border-color: alpha({accent}, 0.5); }}
.edel-new-circle {{ min-width: 36px; min-height: 36px; border-radius: 999px; background-color: alpha({accent}, 0.14); }}
.edel-new-plus {{ color: {accent}; font-size: {hero}px; font-weight: 300; }}
.edel-new-hint {{ color: {muted}; font-size: {small}px; }}
.edel-more {{ min-width: 36px; min-height: 36px; padding: 0; border-radius: 999px; background-image: none;
  background-color: {card}; border: 1px solid {line}; box-shadow: none; color: {text}; }}
.edel-menu-item {{ min-height: 26px; padding: 0 10px; border-radius: {rs}px; background: none; background-image: none;
  box-shadow: none; border: none; color: {text}; font-weight: 400; }}
.edel-menu-item:hover {{ background-color: {accent}; color: {accent_text}; }}
.edel-menu-item.edel-danger {{ color: {error}; }}
/* The Appearance page's theme (M5.12b6b, drawn to the board's sizes in M5.12b6c): the theme line, the three styles, the themes row and the accent's row. Radii are the window's and the small one's tokens; the 12.5 px and 9.5 px sizes are the text size less half a point and less three and a half, as the board draws them at 13 px. */
.edel-appearance .edel-section {{ font-size: {size}px; font-weight: 600; }}
.edel-theme-title {{ font-size: {heading}px; font-weight: 700; letter-spacing: -0.4px; }}
.edel-theme-tag {{ color: {muted}; font-size: {small}px; }}
.edel-theme-changed {{ color: {secondary}; font-size: {caption}px; }}
.edel-changed-dot {{ min-width: 6px; min-height: 6px; border-radius: 999px; background-color: {accent}; }}
.edel-pictures-link {{ min-height: 24px; padding: 0 4px; background: none; box-shadow: none; border: none; color: {secondary}; font-size: {caption}px; font-weight: 400; }}
.edel-pictures-link:hover {{ background-color: {fill}; }}
.edel-pictures-arrow {{ color: {muted}; }}
.edel-theme-reset {{ min-height: 30px; padding: 0 12px; border-radius: 15px; background: none; background-image: none; box-shadow: none; border: none; color: {accent}; font-size: {caption}px; font-weight: 400; }}
.edel-theme-keep {{ min-height: 30px; padding: 0 14px; border-radius: 15px; background-color: {accent}; background-image: none; box-shadow: none; border: none; color: {accent_text}; font-size: {caption}px; font-weight: 600; }}
.edel-theme-more {{ background: none; background-image: none; box-shadow: none; border: none; padding: 0; min-width: 30px; min-height: 30px; }}
.edel-theme-more > button {{ min-width: 30px; min-height: 30px; padding: 0; border-radius: 999px; background: none; background-color: transparent; background-image: none; box-shadow: none; border: 1px solid {line}; color: {text}; }}
/* A style and a tile are toggles: the one chosen is :checked, and its frame's border is the ring (3 px out for a style, 2 px for a tile, as the mockup draws them). */
.edel-style, .edel-tile {{ padding: 0; min-width: 0; min-height: 0; background: none; background-image: none; box-shadow: none; border: none; color: {text}; }}
.edel-style:checked, .edel-tile:checked {{ background: none; background-image: none; box-shadow: none; }}
.edel-style-frame {{ padding: 3px; border: 2px solid transparent; border-radius: {rw_plus}px; }}
.edel-style:checked .edel-style-frame {{ border-color: {accent}; }}
.edel-style-picture {{ border-radius: {rw}px; box-shadow: 0 1px 2px alpha({shadow}, 0.08), 0 10px 24px alpha({shadow}, 0.14); }}
.edel-style-name {{ font-size: {size}px; font-weight: 400; }}
.edel-style:checked .edel-style-name {{ font-weight: 600; }}
.edel-tile-frame {{ padding: 2px; border: 2px solid transparent; border-radius: {rw_plus}px; }}
.edel-tile:checked .edel-tile-frame {{ border-color: {accent}; }}
.edel-tile-picture {{ border-radius: {rw}px; box-shadow: 0 4px 12px alpha({shadow}, 0.12); }}
.edel-tile-name {{ font-size: {caption}px; font-weight: 400; }}
.edel-tile:checked .edel-tile-name {{ font-weight: 600; }}
.edel-theme-built-in {{ background-color: alpha({badge_deep}, 0.38); color: rgba(255, 255, 255, 0.94); font-size: {badge}px; font-weight: 600;
  border-radius: 999px; padding: 2px 6px; box-shadow: inset 0 0 0 0.5px rgba(255, 255, 255, 0.28); }}
.edel-theme-thumb {{ border-radius: 4px; box-shadow: 0 0 0 1.5px {window}; }}
.edel-pictures-sheet {{ padding: 12px; }}
.edel-theme-new {{ padding: 0; min-width: 0; min-height: 0; background: none; background-image: none; box-shadow: none; border: none; color: {text}; }}
.edel-theme-new:focus-visible {{ outline: 2px solid alpha({accent}, 0.45); outline-offset: 2px; }}
.edel-theme-new-look {{ border: 1.5px dashed {line}; border-radius: {rw}px; }}
.edel-theme-new:hover .edel-theme-new-look {{ border-color: alpha({accent}, 0.5); }}
.edel-theme-new-plus {{ color: {accent}; font-size: {heading}px; font-weight: 300; }}
.edel-theme-new-name {{ color: {muted}; font-size: {caption}px; font-weight: 400; }}
/* The accent's row: the swatches and the two pills in the card, the chosen name at the far right. */
.edel-accent-card {{ padding: 16px; }}
.edel-accent-divider {{ min-width: 1px; min-height: 22px; background-color: alpha({text}, 0.14); }}
.edel-accent-pill {{ min-height: 28px; padding: 0 12px; border-radius: 14px; background: none; background-image: none; box-shadow: none;
  border: 1px solid {line}; color: {text}; font-size: {small}px; font-weight: 400; }}
.edel-accent-pill:hover {{ background-color: {fill}; }}
.edel-accent-pill:checked {{ border-color: {accent}; color: {accent}; }}
.edel-accent-pill:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 1px; }}
/* The Text group's font rows (M5.12b6c): 48 px tall, the drop-down 30 px tall in regular weight. */
.edel-font-row {{ min-height: 36px; }}
.edel-font, .edel-font label {{ font-weight: 400; }}
.edel-font > button {{ min-height: 30px; padding-top: 0; padding-bottom: 0; }}
/* The pictures sheet's picture menu (M5.12b6, part c2): its button shows on hover and focus over a card's corner. */
.edel-picture-more {{ opacity: 0; min-width: 24px; min-height: 24px; margin: 6px; border-radius: {rw}px; }}
.edel-picture-overlay:hover .edel-picture-more, .edel-picture-overlay:focus-within .edel-picture-more, .edel-picture-more:focus {{ opacity: 1; }}
/* The Updates and Users pages as the approved board (M5.32, docs/mockups/settings/power-updates-users.jpg): the cards in the card's
   colour (no gradient: CI reads the card token at the hero's top), the hero's round badge (green when ok, the accent for new and sync, amber for a
   restart, red for a problem) with its white icon, the accent pill, the kept-ready card, What's new, and the people's avatars. */
.edel-hero-badge {{ border-radius: 999px; }}
.edel-hero-badge.edel-tone-ok {{ background-color: {ok}; box-shadow: 0 0 22px alpha({ok}, 0.45); }}
.edel-hero-badge.edel-tone-accent {{ background-color: {accent}; box-shadow: 0 0 22px alpha({accent}, 0.4); }}
.edel-hero-badge.edel-tone-restart {{ background-color: {amber}; box-shadow: 0 0 22px alpha({amber}, 0.4); }}
.edel-hero-badge.edel-tone-problem {{ background-color: {problem}; box-shadow: 0 0 22px alpha({problem}, 0.4); }}
.edel-hero-badge image, .edel-hero-badge spinner {{ color: {accent_text}; }}
.edel-hero-button {{ min-height: 36px; padding: 0 22px; border-radius: 999px; background-image: none; background-color: {accent};
  box-shadow: none; border: none; color: {accent_text}; font-weight: 600; }}
.edel-hero-button:hover {{ background-color: alpha({accent}, 0.88); }}
.edel-hero-button:disabled {{ opacity: 0.5; }}
.edel-hero-button:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 2px; }}
.edel-kept-mark {{ border-radius: {rs}px; background-color: alpha({accent}, 0.14); color: {accent}; }}
.edel-kept-title {{ font-weight: 600; }}
.edel-kept-button {{ min-height: 28px; padding: 0 16px; border-radius: 999px; background-image: none; background-color: {fill};
  box-shadow: none; border: none; color: {text}; font-weight: 500; }}
.edel-kept-button:hover {{ background-color: {fill_strong}; }}
.edel-kept-button:disabled {{ opacity: 0.5; }}
.edel-kept-button:focus-visible {{ outline: 2px solid alpha({accent}, 0.5); outline-offset: 2px; }}
.edel-news {{ padding: 14px 16px; }}
.edel-news-title {{ font-weight: 600; }}
.edel-avatar {{ min-width: 84px; min-height: 84px; border-radius: 999px; background-image: linear-gradient(135deg, {accent}, shade({accent}, 0.8));
  color: {accent_text}; font-size: {name}px; font-weight: 650; }}
.edel-avatar-small {{ min-width: 32px; min-height: 32px; font-size: {big}px; font-weight: 650; }}
.edel-person-name {{ font-size: {hero}px; font-weight: 650; letter-spacing: -0.3px; margin-top: 6px; }}
.edel-person-line {{ color: {secondary}; font-size: {big}px; }}
.edel-row-name {{ font-weight: 600; }}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stylesheet_is_the_tokens() {
        let mut t = Tokens::built_in();
        let css = css(&t, false);
        assert!(css.contains(&format!("background-color: {};", css_colour(t.window))));
        assert!(css.contains(&format!("font-family: \"{}\";", t.font)));
        // A changed token changes the sheet: no colour is written here.
        t.accent = edel::tokens::Colour::parse("#e5484d").unwrap();
        t.radius_window = 3;
        let changed = super::css(&t, false);
        assert!(changed.contains("background-color: #e5484d;"));
        assert!(changed.contains("border-radius: 3px;"));
        assert!(!changed.contains(&Tokens::built_in().accent.hex()));
    }
}
