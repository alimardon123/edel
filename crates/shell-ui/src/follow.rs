//! shell-ui follows a change of look while it runs (M6.17): the preset, the
//! colour scheme, the accent and colours, the shape, the text, the icons
//! and the language, each read again from the settings files when the
//! compositor's `settings` event says they changed. Before, the compositor
//! started shell-ui again for each, which lost the notifications, the tray
//! and anything open. Now the panels keep their surfaces and are drawn
//! again in the new look, resized in place when their height changed;
//! what hangs over them closes, as its size and place come from the look,
//! and quick settings opens again when it was open, as its Dark style tile
//! is one of the ways to change the look; the lock is drawn again.

use edel::presets;
use edel::tokens::{Scheme, Tokens};

use crate::Shell;

/// What the settings files choose that the look follows, as read now.
pub struct Chosen {
    /// The preset `layout.preset` names, else Classic's name.
    pub preset: String,
    pub scheme: Scheme,
}

/// Why shell-ui follows, in the words the compositor said when it started
/// shell-ui again for the same changes, first the one that matters most:
/// the preset, the scheme, the tokens (the text, the icons, else the
/// colours and shape), the language. None when nothing it shows changed.
pub fn why(
    old: &Chosen,
    new: &Chosen,
    old_tokens: &Tokens,
    new_tokens: &Tokens,
    language: Option<&Option<String>>,
) -> Option<String> {
    // A name this release lacks is Classic, so it differs from Classic only
    // in name and changes nothing (M5.4a).
    if old.preset != new.preset
        && presets::named(Some(&old.preset)).0 != presets::named(Some(&new.preset)).0
    {
        return Some(format!("the preset is now {}", new.preset));
    }
    if old.scheme != new.scheme {
        return Some(format!("the colour scheme is now {}", new.scheme.name()));
    }
    if old_tokens != new_tokens {
        let text = old_tokens.font != new_tokens.font
            || old_tokens.panel_text_size != new_tokens.panel_text_size;
        return Some(if text {
            "the text changed".to_string()
        } else if old_tokens.panel_icon != new_tokens.panel_icon {
            "the icons changed".to_string()
        } else {
            "the colours changed".to_string()
        });
    }
    language.map(|language| {
        let name = language.as_deref().unwrap_or("English");
        format!("the language is now {name}")
    })
}

impl Shell {
    /// The settings files changed: the look follows what they choose now,
    /// before the rest of `settings_changed` reads the panels and the
    /// portal's settings with it.
    pub fn follow_look(&mut self) {
        let new = crate::chosen();
        let language = edel::i18n::follow("shell-ui");
        if let Some((name, words)) = &language {
            let name = name.as_deref().unwrap_or("English");
            eprintln!("edel-shell-ui: words in {name}, {words} translated");
        }
        // The accent, colours, shape, text, icons and contrast keys all lay
        // over the tokens, so they are read again whatever changed; the
        // files changed, which is seldom.
        let (tokens, notes) = edel::tokens::on_this_machine(new.scheme);
        if tokens != self.tokens {
            for note in notes {
                eprintln!("edel-shell-ui: {}: {note}", edel::tokens::PATH);
            }
        }
        let tiles = if new.preset != self.chosen.preset {
            let (preset, note) = presets::named(Some(&new.preset));
            if let Some(note) = note {
                eprintln!("edel-shell-ui: {note}");
            }
            preset.quick.tiles
        } else {
            self.quick_tiles.clone()
        };
        let Some(why) = why(
            &self.chosen,
            &new,
            &self.tokens,
            &tokens,
            language.as_ref().map(|(name, _)| name),
        ) else {
            // A name that changes nothing on screen is still the one now.
            self.quick_tiles = tiles;
            self.chosen = new;
            return;
        };
        eprintln!("edel-shell-ui: follows the settings: {why}");
        let quick_open = self.quick.is_some();
        // What is drawn in the old look closes: the popups, and the panel
        // editor and the lock looks, which write nothing when closed so.
        self.close_popups();
        self.close_editor(false);
        self.close_lock_looks();
        if new.scheme != self.chosen.scheme {
            crate::terminal::follow(new.scheme == Scheme::Dark);
        }
        if tokens != self.tokens {
            let old = std::mem::replace(&mut self.tokens, tokens);
            if old.font != self.tokens.font || old.panel_text_size != self.tokens.panel_text_size {
                eprintln!(
                    "edel-shell-ui: text {} at {} px",
                    self.tokens.font, self.tokens.panel_text_size
                );
            }
            if old.panel_icon != self.tokens.panel_icon {
                eprintln!("edel-shell-ui: icons at {} px", self.tokens.panel_icon);
            }
            self.text.set_family(&self.tokens.font);
            self.place_panels(&old);
        }
        self.quick_tiles = tiles;
        self.chosen = new;
        for panel in &mut self.panels {
            // Drawn and laid out again in the new look; its places are
            // logged again, as at start, for the tests that click them.
            panel.drawn = None;
            panel.places.clear();
        }
        self.draw_all();
        self.lock_relook();
        if self.status_wanted {
            // The status area's words, read again in the language now.
            self.request_status(false);
        }
        if quick_open {
            self.open_quick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen(preset: &str, scheme: Scheme) -> Chosen {
        Chosen {
            preset: preset.to_string(),
            scheme,
        }
    }

    #[test]
    fn the_reason_names_what_matters_most_and_nothing_when_nothing_changed() {
        let tokens = Tokens::built_in();
        let classic = chosen("classic", Scheme::Light);
        assert_eq!(why(&classic, &classic, &tokens, &tokens, None), None);
        // A preset this release lacks is Classic: no change.
        let unknown = chosen("tablet", Scheme::Light);
        assert_eq!(why(&classic, &unknown, &tokens, &tokens, None), None);
        let mac = chosen("mac-like", Scheme::Dark);
        assert_eq!(
            why(&classic, &mac, &tokens, &tokens, None).as_deref(),
            Some("the preset is now mac-like")
        );
        let dark = chosen("classic", Scheme::Dark);
        assert_eq!(
            why(&classic, &dark, &tokens, &tokens, None).as_deref(),
            Some("the colour scheme is now dark")
        );
        let mut red = tokens.clone();
        red.accent.r = 1.0;
        assert_eq!(
            why(&classic, &classic, &tokens, &red, None).as_deref(),
            Some("the colours changed")
        );
        let mut bigger = tokens.clone();
        bigger.panel_text_size += 2;
        assert_eq!(
            why(&classic, &classic, &tokens, &bigger, None).as_deref(),
            Some("the text changed")
        );
        let mut icons = tokens.clone();
        icons.panel_icon += 4;
        assert_eq!(
            why(&classic, &classic, &tokens, &icons, None).as_deref(),
            Some("the icons changed")
        );
        let xx = Some("xx".to_string());
        assert_eq!(
            why(&classic, &classic, &tokens, &tokens, Some(&xx)).as_deref(),
            Some("the language is now xx")
        );
        assert_eq!(
            why(&classic, &classic, &tokens, &tokens, Some(&None)).as_deref(),
            Some("the language is now English")
        );
    }
}
