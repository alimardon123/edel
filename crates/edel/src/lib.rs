//! The library half of `edel`: the system file parser, which the compositor,
//! shell-ui and Settings link so every part reads `system.toml` with the
//! same code (ADR-008). It needs no network or signing; those sit behind
//! the binary's `cli` feature.

pub mod system;
