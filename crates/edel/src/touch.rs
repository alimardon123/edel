//! Touch mode (roadmap M9.6, `docs/layout-guide.md`): the desktop-wide
//! word that a person is using fingers rather than a pointer. shell-ui says
//! it to apps through the settings portal under [`NAMESPACE`] and [`KEY`],
//! true while the machine's tablet-mode switch is on, and Settings grows
//! its rows to [`TARGET`] while it is.
//!
//! Seam (ADR-008, M9.3): the form factor, `[layout] device_type`, decides it
//! once M9.3 makes Phone and Tablet; the portal key stays the same.

/// The settings portal's namespace for Edel OS's own keys.
pub const NAMESPACE: &str = "io.github.alimardon123.edel";

/// The portal key that is true in touch mode, else false.
pub const KEY: &str = "touch-mode";

/// The side of the smallest target a finger can hit, logical pixels, its
/// padding included (`docs/layout-guide.md`, "Touch targets").
pub const TARGET: u32 = 44;

/// The width below which a screen or window is Compact, logical pixels
/// (`docs/layout-guide.md`, "Size classes"): laid out for a phone.
pub const COMPACT_BELOW: u32 = 600;
