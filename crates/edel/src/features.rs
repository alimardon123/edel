//! Feature files (ADR-008, roadmap M4.0): `features/NAME.toml`, the unit
//! Edel OS is made of. A feature names its packages, its OpenRC services by
//! runlevel, its kernel modules and mkinitfs features, and the health files
//! it writes; `features/NAME/`, if present, is copied over the root. Image
//! definitions list features (`crate::def` in the binary merges them), and
//! every image carries the files of its features in
//! `/usr/share/edel/features/`, so the parts can ask what a machine has.
//!
//! Two readers, as for the settings file, both behind the `os` feature:
//! `check` is strict, for `edel image build` and `image check`; `read` is
//! lenient, for readers on a machine, and returns what it ignored so the
//! caller can report it.
//!
//! What every part may ask, the folder and the rule for a name, is here;
//! the reader is the system's own work, behind the library's `os` feature
//! (M6.17), so the compositor and shell-ui never link it.

#[cfg(feature = "os")]
mod file;
#[cfg(feature = "os")]
pub use file::*;

/// Where every image keeps the files of its features.
pub const DIR: &str = crate::places::FEATURES_DIR;

/// a-z, 0-9 and `-`, not starting with `-`: feature and service names.
pub fn is_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
