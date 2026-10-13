//! What every settings file key means, its default and who decides it
//! (M6.11, ADR-008): one line per key of [`crate::settings::KEYS`], the
//! one owner of the help `edel settings get` prints, the suggestion
//! `set` and `check` give, and the two generated docs,
//! `docs/settings.md`'s key list and `docs/defaults.md`. A test fails
//! when a key has no line here or a line names no key.

/// Who decides a key's value while the settings file does not set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecidedBy {
    /// The release's own value, the same on every machine.
    Release,
    /// The layout preset in use (`layout.preset`), so it differs by preset.
    Preset,
    /// A design token in `design/tokens.toml`.
    Token,
    /// The image the machine was made from (a desktop, a server).
    Image,
    /// The hardware: the screen, the keyboard, the disk.
    Hardware,
}

impl DecidedBy {
    /// The word `edel settings get` and the docs show for it.
    pub fn word(self) -> &'static str {
        match self {
            DecidedBy::Release => "release",
            DecidedBy::Preset => "preset",
            DecidedBy::Token => "token",
            DecidedBy::Image => "image",
            DecidedBy::Hardware => "hardware",
        }
    }
}

/// Whose a key is (Seam, ADR-011): a person's follows them to another of
/// their devices (M7.10f), a device's stays with the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Belongs {
    Person,
    Device,
}

/// One key's line.
#[derive(Clone, Copy, Debug)]
pub struct Help {
    /// The key's path as [`crate::settings::KEYS`] writes it, `*` and all.
    pub key: &'static str,
    /// One sentence for people, marked for translation.
    pub help: &'static str,
    /// The value that applies while the file does not set the key, as the
    /// file would write it, or a short phrase when it depends ("the
    /// preset's", "every screen's own").
    pub default: &'static str,
    /// Who decides that default.
    pub from: DecidedBy,
    pub belongs: Belongs,
    /// The release that stopped using the key, if one did: it is still
    /// read, reported as no longer used and refused by `set`.
    pub retired: Option<&'static str>,
}

/// Every key's line, in the key table's order.
pub const HELP: &[Help] = &[];

/// The line for `key`, a concrete key (`users.ali.admin`) or a table path
/// (`users.*.admin`).
pub fn find(key: &str) -> Option<&'static Help> {
    HELP.iter().find(|line| fits(line.key, key))
}

/// Whether `key` is `path`, a `*` standing for any one part.
fn fits(path: &str, key: &str) -> bool {
    let (mut a, mut b) = (path.split('.'), key.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(p), Some(k)) if p == "*" || p == k => {}
            _ => return false,
        }
    }
}
