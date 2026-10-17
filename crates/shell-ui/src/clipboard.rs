//! Clipboard history (M7.13): the last texts and pictures copied, shown
//! on Super+V. What is kept lives in memory only: it is gone at a reboot,
//! and the list is emptied at the lock and when `clipboard.history` turns
//! off. A selection whose offer says it is a password manager's secret
//! (`SECRET`) is never read. This module is the pure model, tested without
//! a display; `clipboard_list.rs` draws it and `clipboard_card.rs` holds
//! the surface and the ext-data-control client. The seam for the devices
//! of M7.10b is `History::items`, whose first a paired device would be sent.

use std::collections::VecDeque;
use std::sync::Arc;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// How many items are kept, newest first.
pub const MOST: usize = 10;
/// The bytes of one text kept; a longer text is not kept.
pub const MOST_TEXT: usize = 1 << 20;
/// The bytes of one picture (a PNG as copied) kept.
pub const MOST_PICTURE: usize = 16 << 20;
/// The offer's type that marks a password manager's secret: never read.
pub const SECRET: &str = "x-kde-passwordManagerHint";
/// The text types a text is offered as, the first one preferred.
pub const TEXT_TYPES: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
/// The type a picture is read and offered as.
pub const PICTURE_TYPE: &str = "image/png";
/// A picture's thumbnail is at most this high, in pixels, and never more
/// than four times as wide.
pub const THUMB: u32 = 48;

/// What the list's line for a text shows: at most this many characters.
const LINE_MOST: usize = 120;

/// One thing copied: a text, or a picture as its PNG with a thumbnail for
/// the list.
#[derive(Clone, Debug)]
pub enum Item {
    Text(String),
    Picture { png: Arc<[u8]>, thumb: Arc<Pixmap> },
}

impl PartialEq for Item {
    /// Texts are equal by their text, pictures by their PNG bytes.
    fn eq(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Text(a), Item::Text(b)) => a == b,
            (Item::Picture { png: a, .. }, Item::Picture { png: b, .. }) => a[..] == b[..],
            _ => false,
        }
    }
}

impl Item {
    /// A text as copied, `None` when it is empty or only whitespace, or
    /// longer than `MOST_TEXT` bytes.
    pub fn text(text: String) -> Option<Item> {
        if text.trim().is_empty() || text.len() > MOST_TEXT {
            return None;
        }
        Some(Item::Text(text))
    }

    /// A picture from its PNG bytes, `None` when it is over `MOST_PICTURE`
    /// bytes or does not decode. Its thumbnail is scaled down to `THUMB`
    /// high (never up), and to at most four times `THUMB` wide.
    pub fn picture(png: Vec<u8>) -> Option<Item> {
        if png.len() > MOST_PICTURE {
            return None;
        }
        let source = Pixmap::decode_png(&png).ok()?;
        let thumb = thumb_of(&source)?;
        Some(Item::Picture {
            png: Arc::from(png),
            thumb: Arc::new(thumb),
        })
    }

    /// The type the item is offered as when pasted.
    pub fn mime(&self) -> &'static str {
        match self {
            Item::Text(_) => TEXT_TYPES[0],
            Item::Picture { .. } => PICTURE_TYPE,
        }
    }

    /// The bytes offered for a paste: the text's, or the PNG.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Item::Text(text) => text.as_bytes(),
            Item::Picture { png, .. } => png,
        }
    }

    /// The line the list shows for a text: whitespace runs collapsed to one
    /// space, trimmed, at most `LINE_MOST` characters. `None` for a picture.
    pub fn line(&self) -> Option<String> {
        let Item::Text(text) = self else {
            return None;
        };
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = match collapsed.char_indices().nth(LINE_MOST) {
            Some((cut, _)) => collapsed[..cut].to_string(),
            None => collapsed,
        };
        Some(line)
    }
}

/// `source` scaled to `THUMB` high (never up) and at most four times
/// `THUMB` wide, its aspect kept.
fn thumb_of(source: &Pixmap) -> Option<Pixmap> {
    let (w, h) = (source.width() as f32, source.height() as f32);
    let high = THUMB as f32;
    let mut scale = (high / h).min(1.0);
    let most_wide = 4.0 * high;
    if w * scale > most_wide {
        scale = most_wide / w;
    }
    let tw = (w * scale).round().max(1.0) as u32;
    let th = (h * scale).round().max(1.0) as u32;
    let mut thumb = Pixmap::new(tw, th)?;
    let smooth = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let to = Transform::from_scale(tw as f32 / w, th as f32 / h);
    thumb.draw_pixmap(0, 0, source.as_ref(), &smooth, to, None);
    Some(thumb)
}

/// What to read of an offer that lists `types`: `None` when it lists
/// `SECRET` (a password manager's secret) or nothing kept; else the first
/// of `TEXT_TYPES` it lists, else `PICTURE_TYPE` when it lists that.
pub fn wanted(types: &[String]) -> Option<&'static str> {
    if types.iter().any(|t| t == SECRET) {
        return None;
    }
    let has = |want: &str| types.iter().any(|t| t == want);
    TEXT_TYPES
        .iter()
        .copied()
        .find(|t| has(t))
        .or_else(|| has(PICTURE_TYPE).then_some(PICTURE_TYPE))
}

/// The list: the items kept, newest first, and whether keeping is on.
#[derive(Debug, Default)]
pub struct History {
    items: VecDeque<Item>,
    off: bool,
}

impl History {
    /// An empty list, keeping items when `on`.
    pub fn new(on: bool) -> History {
        History {
            items: VecDeque::new(),
            off: !on,
        }
    }

    /// Whether items are kept.
    pub fn on(&self) -> bool {
        !self.off
    }

    /// Turns keeping on or off; off empties the list.
    pub fn set_on(&mut self, on: bool) {
        self.off = !on;
        if self.off {
            self.items.clear();
        }
    }

    /// Keeps `item` as the newest. An item equal to one kept moves to the
    /// front; the oldest past `MOST` goes. Nothing happens while off.
    /// Returns whether the order of the list changed.
    pub fn add(&mut self, item: Item) -> bool {
        if self.off {
            return false;
        }
        if let Some(at) = self.items.iter().position(|kept| *kept == item) {
            self.items.remove(at);
            self.items.push_front(item);
            return at != 0;
        }
        self.items.push_front(item);
        self.items.truncate(MOST);
        true
    }

    /// Takes the item at `index` (newest first) out of the list.
    pub fn remove(&mut self, index: usize) -> Option<Item> {
        self.items.remove(index)
    }

    /// Empties the list.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The items, newest first.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }

    /// The item at `index`, newest first.
    pub fn get(&self, index: usize) -> Option<&Item> {
        self.items.get(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Item {
        Item::text(s.to_string()).unwrap()
    }

    /// A PNG of `w` by `h` pixels, drawn in one colour.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(40, 90, 200, 255));
        pixmap.encode_png().unwrap()
    }

    fn texts(history: &History) -> Vec<String> {
        history
            .items()
            .map(|item| item.line().unwrap_or_default())
            .collect()
    }

    #[test]
    fn the_newest_comes_first_and_an_equal_one_moves_to_the_front() {
        let mut history = History::new(true);
        assert!(history.add(text("one")));
        assert!(history.add(text("two")));
        assert_eq!(texts(&history), ["two", "one"]);
        assert!(history.add(text("one")), "one moves to the front");
        assert_eq!(texts(&history), ["one", "two"]);
        assert!(!history.add(text("one")), "already at the front: no change");
        assert_eq!(history.len(), 2, "no copy of an equal item");
        assert_eq!(history.get(0).and_then(Item::line).as_deref(), Some("one"));
    }

    #[test]
    fn only_the_newest_ten_are_kept() {
        let mut history = History::new(true);
        for i in 0..MOST + 3 {
            history.add(text(&format!("item {i}")));
        }
        assert_eq!(history.len(), MOST);
        assert_eq!(history.get(0).and_then(Item::line).unwrap(), "item 12");
        assert_eq!(
            history.get(MOST - 1).and_then(Item::line).unwrap(),
            "item 3"
        );
    }

    #[test]
    fn off_keeps_nothing_and_turning_it_off_empties_the_list() {
        let mut history = History::new(false);
        assert!(!history.on());
        assert!(!history.add(text("secret copy")));
        assert!(history.is_empty());
        history.set_on(true);
        history.add(text("kept"));
        history.set_on(false);
        assert!(history.is_empty(), "off empties the list");
        assert!(!history.add(text("not kept")));
    }

    #[test]
    fn remove_and_clear_take_items_out() {
        let mut history = History::new(true);
        history.add(text("a"));
        history.add(text("b"));
        history.add(text("c"));
        assert_eq!(
            history.remove(1).and_then(|i| i.line()).as_deref(),
            Some("b")
        );
        assert_eq!(texts(&history), ["c", "a"]);
        assert!(history.remove(5).is_none());
        history.clear();
        assert!(history.is_empty());
        assert!(history.get(0).is_none());
    }

    #[test]
    fn a_secret_or_nothing_we_keep_is_never_read() {
        let offer =
            |types: &[&str]| -> Vec<String> { types.iter().map(|t| t.to_string()).collect() };
        assert_eq!(
            wanted(&offer(&["text/plain;charset=utf-8", SECRET])),
            None,
            "a password manager's secret"
        );
        assert_eq!(
            wanted(&offer(&["text/plain", "text/plain;charset=utf-8"])),
            Some("text/plain;charset=utf-8"),
            "UTF-8 preferred"
        );
        assert_eq!(wanted(&offer(&["text/plain"])), Some("text/plain"));
        assert_eq!(
            wanted(&offer(&["text/html", PICTURE_TYPE])),
            Some(PICTURE_TYPE),
            "a picture when no text is offered"
        );
        assert_eq!(wanted(&offer(&["text/html"])), None, "nothing we keep");
        assert_eq!(wanted(&[]), None);
    }

    #[test]
    fn a_text_is_kept_unless_empty_or_too_long() {
        assert!(Item::text(String::new()).is_none());
        assert!(Item::text(" \n\t ".to_string()).is_none());
        assert!(Item::text("x".repeat(MOST_TEXT)).is_some());
        assert!(Item::text("x".repeat(MOST_TEXT + 1)).is_none());
    }

    #[test]
    fn a_text_line_collapses_whitespace_and_is_cut_at_120_characters() {
        assert_eq!(
            text("  first\n\n second \t third  ").line().as_deref(),
            Some("first second third")
        );
        let long = "é".repeat(LINE_MOST + 5);
        let line = text(&long).line().unwrap();
        assert_eq!(line.chars().count(), LINE_MOST);
        assert_eq!(text("a").mime(), TEXT_TYPES[0]);
        assert_eq!(text("a").bytes(), b"a");
    }

    #[test]
    fn two_pictures_are_equal_by_their_bytes() {
        let a = Item::picture(png(20, 10)).unwrap();
        let b = Item::picture(png(20, 10)).unwrap();
        let c = Item::picture(png(30, 10)).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, text("x"));
    }

    #[test]
    fn a_picture_has_a_thumbnail_48_high_and_keeps_its_aspect() {
        let item = Item::picture(png(200, 100)).unwrap();
        let Item::Picture { thumb, png } = &item else {
            panic!("a picture")
        };
        assert_eq!((thumb.width(), thumb.height()), (96, 48));
        assert!(!png.is_empty());
        assert_eq!(item.mime(), PICTURE_TYPE);
        assert_eq!(item.bytes(), &png[..]);
        assert_eq!(item.line(), None);
    }

    #[test]
    fn a_small_picture_is_not_scaled_up_and_a_wide_one_is_capped() {
        let small = Item::picture(png(20, 10)).unwrap();
        let Item::Picture { thumb, .. } = &small else {
            panic!("a picture")
        };
        assert_eq!((thumb.width(), thumb.height()), (20, 10));
        let wide = Item::picture(png(1000, 10)).unwrap();
        let Item::Picture { thumb, .. } = &wide else {
            panic!("a picture")
        };
        assert_eq!(thumb.width(), 4 * THUMB);
    }

    #[test]
    fn garbage_is_not_a_picture() {
        assert!(Item::picture(b"not a png".to_vec()).is_none());
    }
}
