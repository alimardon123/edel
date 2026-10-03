//! The library half of `edel`: the system file parser, which the compositor,
//! shell-ui and Settings link so every part reads `system.toml` with the
//! same code (ADR-008), the feature file reader, which tells them what a
//! machine has, the install plan Settings' installer page shows and the
//! keyboard shortcuts every part agrees on (M5.13). It
//! needs no network or signing; those sit behind the binary's `cli`
//! feature.

pub mod features;
pub mod install;
pub mod shortcuts;
pub mod system;
