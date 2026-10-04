//! The library half of `edel`: the system file parser, which the compositor,
//! shell-ui and Settings link so every part reads `system.toml` with the
//! same code (ADR-008), the feature file reader, which tells them what a
//! machine has, the install plan Settings' installer page shows, the
//! keyboard shortcuts every part agrees on (M5.13), the design tokens the
//! compositor and shell-ui draw with (M5.1b) and the layout presets
//! (M5.1c). It needs no network or signing; those sit behind the binary's
//! `cli` feature.

pub mod features;
#[cfg(feature = "icons")]
pub mod icons;
pub mod install;
pub mod presets;
pub mod shortcuts;
pub mod system;
pub mod tokens;
