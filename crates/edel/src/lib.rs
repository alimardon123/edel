//! The library half of `edel`: the system file parser, which the compositor,
//! shell-ui and Settings link so every part reads `system.toml` with the
//! same code (ADR-008), the feature file reader, which tells them what a
//! machine has, and the install plan Settings' installer page shows. It
//! needs no network or signing; those sit behind the binary's `cli`
//! feature.

pub mod features;
pub mod install;
pub mod system;
