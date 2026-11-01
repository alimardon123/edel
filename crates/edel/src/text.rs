//! Text for the parts that draw their own (M8.21, T4): shell-ui and the
//! compositor's title bars, the overview and the frame rate. One engine,
//! cosmic-text over fontconfig's fonts, shapes every script and falls back
//! from the tokens' interface font to whatever font has the letters, so
//! CJK, Arabic and emoji titles show as words, not boxes. A line is shaped
//! once and drawn to a tiny-skia pixmap ([`Text::draw`]) or to any canvas
//! ([`Text::paint`]). Behind the `text` feature.

use cosmic_text::{
    Attrs, Buffer, Color, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping,
    SwashCache, Weight,
};
use tiny_skia::Pixmap;

use crate::tokens::Colour;

/// The fonts and glyph cache text is drawn with, loaded once.
pub struct Text {
    fonts: FontSystem,
    glyphs: SwashCache,
    /// The interface font's family, from the tokens.
    family: String,
}

/// How heavy a line of text is and whether its figures all have one
/// width (Inter's `tnum`), as the mockups set the clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub weight: u16,
    pub tabular: bool,
}

impl Face {
    pub const REGULAR: Face = Face {
        weight: 400,
        tabular: false,
    };
    /// Window buttons' titles.
    pub const MEDIUM: Face = Face {
        weight: 500,
        tabular: false,
    };
    /// Numbers that stand out: the clock's time, the workspaces'.
    pub const SEMIBOLD: Face = Face {
        weight: 600,
        tabular: false,
    };

    /// The same weight with tabular figures.
    pub const fn tabular(self) -> Face {
        Face {
            tabular: true,
            ..self
        }
    }
}

/// One line of text, shaped.
pub struct Line {
    buffer: Buffer,
    pub width: f32,
    /// The words shaped, cut short when [`Text::fit`] made the line.
    text: String,
}

impl Line {
    /// The words shaped, with the ellipsis when they were cut short.
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Text {
    /// Text in `family`, the tokens' interface font. When fontconfig finds
    /// no font at all, the fallback fonts are loaded from their files; with
    /// none of those either, text is drawn as nothing rather than ending
    /// the part that draws it, which may be the lock screen (M6.17), and
    /// [`Text::has_fonts`] says so for the part's own message.
    pub fn load(family: &str) -> Text {
        let mut fonts = FontSystem::new();
        if fonts.db().is_empty() {
            for path in crate::places::FALLBACK_FONTS {
                let _ = fonts.db_mut().load_font_file(path);
            }
        }
        Text {
            fonts,
            glyphs: SwashCache::new(),
            family: family.to_string(),
        }
    }

    /// Shapes in `family` from now on, as the look changed (M6.17): the
    /// fonts are the system's, loaded once, so only the name changes.
    pub fn set_family(&mut self, family: &str) {
        if self.family != family {
            self.family = family.to_string();
        }
    }

    /// `text` shaped in the interface font, falling back for other
    /// scripts, `size` pixels high.
    pub fn line(&mut self, text: &str, size: f32) -> Line {
        self.line_in(text, size, Face::REGULAR)
    }

    /// [`Text::line`] in `face`.
    pub fn line_in(&mut self, text: &str, size: f32, face: Face) -> Line {
        let metrics = Metrics::new(size, size * 1.25);
        if self.fonts.db().is_empty() {
            // Shaping with no font at all panics in cosmic-text: an empty
            // line instead, never shaped.
            return Line {
                buffer: Buffer::new_empty(metrics),
                width: 0.0,
                text: String::new(),
            };
        }
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        buffer.set_size(None, None);
        let mut attrs = Attrs::new()
            .family(Family::Name(&self.family))
            .weight(Weight(face.weight));
        if face.tabular {
            let mut features = FontFeatures::new();
            features.enable(FeatureTag::new(b"tnum"));
            attrs = attrs.font_features(features);
        }
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        Line {
            buffer,
            width,
            text: text.to_string(),
        }
    }

    /// `text` as one line `size` pixels high in at most `room` pixels:
    /// whole, or cut short with an ellipsis, or nothing if not even that
    /// fits.
    pub fn fit(&mut self, text: &str, size: f32, room: f32) -> Line {
        self.fit_in(text, size, room, Face::REGULAR)
    }

    /// [`Text::fit`] in `face`.
    pub fn fit_in(&mut self, text: &str, size: f32, room: f32, face: Face) -> Line {
        let whole = self.line_in(text, size, face);
        if whole.width <= room {
            return whole;
        }
        // The most characters that fit before the ellipsis, by halving.
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut fits, mut over) = (0, ends.len());
        let mut best = self.line_in("\u{2026}", size, face);
        while fits + 1 < over {
            let mid = (fits + over) / 2;
            let cut = format!("{}\u{2026}", text[..ends[mid]].trim_end());
            let line = self.line_in(&cut, size, face);
            if line.width <= room {
                (fits, best) = (mid, line);
            } else {
                over = mid;
            }
        }
        if best.width > room {
            return self.line_in("", size, face);
        }
        best
    }

    /// Whether any font loaded; with none, every line is empty.
    pub fn has_fonts(&self) -> bool {
        !self.fonts.db().is_empty()
    }

    /// How far above the baseline (positive) and below it (negative) the
    /// interface font reaches at `size` pixels in `face`, as the font
    /// itself says; with no font, `size` split as most fonts split it.
    pub fn extent(&mut self, size: f32, face: Face) -> (f32, f32) {
        let query = cosmic_text::fontdb::Query {
            families: &[Family::Name(&self.family), Family::SansSerif],
            weight: Weight(face.weight),
            ..Default::default()
        };
        let found = self.fonts.db().query(&query);
        let metrics = found
            .and_then(|id| self.fonts.get_font(id, Weight(face.weight)))
            .map(|font| font.as_swash().metrics(&[]));
        match metrics {
            Some(m) if m.units_per_em > 0 => {
                let per = size / f32::from(m.units_per_em);
                (m.ascent * per, -m.descent.abs() * per)
            }
            _ => (size * 0.8, -size * 0.2),
        }
    }

    /// Where `line`'s baseline lies below its top, in pixels.
    pub fn baseline(line: &Line) -> f32 {
        line.buffer
            .layout_runs()
            .next()
            .map_or(0.0, |run| run.line_y)
    }

    /// Hands each pixel `line` covers with its top left at `x`, `y`
    /// (rounded to pixels) to `cover`, with how much of it the letters
    /// cover, 0 to 1, so any canvas can blend it in its own way.
    pub fn paint(&mut self, line: &mut Line, x: f32, y: f32, mut cover: impl FnMut(i32, i32, f32)) {
        let (x, y) = (x.round() as i32, y.round() as i32);
        let ink = Color::rgba(255, 255, 255, 255);
        line.buffer.draw(
            &mut self.fonts,
            &mut self.glyphs,
            ink,
            |gx, gy, gw, gh, color| {
                let alpha = color.a() as f32 / 255.0;
                if alpha == 0.0 {
                    return;
                }
                for py in (y + gy)..(y + gy + gh as i32) {
                    for px in (x + gx)..(x + gx + gw as i32) {
                        cover(px, py, alpha);
                    }
                }
            },
        );
    }

    /// Draws `line` with its top left at `x`, `y`, rounded to pixels.
    pub fn draw(&mut self, pixmap: &mut Pixmap, line: &mut Line, x: f32, y: f32, colour: Colour) {
        let ink = [colour.r, colour.g, colour.b, 1.0].map(|c| (c * 255.0) as u8 as f32);
        let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
        let pixels = pixmap.data_mut();
        self.paint(line, x, y, |px, py, alpha| {
            if px < 0 || py < 0 || px >= pw || py >= ph {
                return;
            }
            let i = ((py * pw + px) * 4) as usize;
            // Premultiplied source over premultiplied destination.
            for (k, channel) in ink.into_iter().enumerate() {
                let src = channel * alpha;
                let dst = pixels[i + k] as f32;
                pixels[i + k] = (src + dst * (1.0 - alpha)).round() as u8;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_font_at_all_text_is_drawn_as_nothing() {
        // Neither fontconfig's fonts nor the fallback files (M6.17):
        // cosmic-text panics shaping with none, which once ended shell-ui
        // and left a lock screen nothing could unlock.
        let empty = cosmic_text::fontdb::Database::new();
        let mut text = Text {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), empty),
            glyphs: SwashCache::new(),
            family: "Inter".into(),
        };
        let mut line = text.line("Wrong password. Type it again.", 14.0);
        assert_eq!(line.width, 0.0);
        assert_eq!(text.fit("Settings", 14.0, 40.0).width, 0.0);
        let mut pixmap = Pixmap::new(64, 16).unwrap();
        let ink = Colour {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        text.draw(&mut pixmap, &mut line, 0.0, 0.0, ink);
        assert!(pixmap.data().iter().all(|b| *b == 0));
    }
    #[test]
    fn a_line_keeps_its_words_and_a_fitted_one_its_ellipsis() {
        let mut text = Text::load("Inter");
        if !text.has_fonts() {
            return;
        }
        let line = text.line("Files", 13.0);
        assert_eq!(line.text(), "Files");
        let fitted = text.fit(
            "a title far too long for the little room it gets",
            13.0,
            100.0,
        );
        assert!(fitted.text().ends_with('\u{2026}'), "{}", fitted.text());
        assert!(fitted.width <= 100.0);
        let (ascent, descent) = text.extent(13.0, Face::REGULAR);
        assert!(ascent > 0.0 && descent < 0.0 && ascent - descent < 26.0);
    }
}
