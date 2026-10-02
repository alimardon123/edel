//! GRUB's environment block on the EFI system partition (ADR-006).
//!
//! The block is exactly 1024 bytes: a header line, `name=value` lines and
//! `#` padding. GRUB reads and writes it with `load_env` and `save_env`, and
//! `edel update` rewrites it in place. The variables use RAUC's names:
//! `ORDER`, the slots in the order to try them, and for each slot
//! `<slot>_OK` (it holds a complete system) and `<slot>_TRY` (boot attempts
//! not confirmed yet).

use std::fmt;

use anyhow::{Result, bail};

/// GRUB's environment block is always exactly this many bytes.
pub const SIZE: usize = 1024;
const HEADER: &str = "# GRUB Environment Block\n";

/// One of the two root slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    /// The slot on GPT partition `number`: 2 is A, 3 is B (see `boot.rs`).
    pub fn from_partition(number: u32) -> Option<Slot> {
        match number {
            2 => Some(Slot::A),
            3 => Some(Slot::B),
            _ => None,
        }
    }

    /// The GPT partition number of the slot.
    pub fn partition(self) -> u32 {
        match self {
            Slot::A => 2,
            Slot::B => 3,
        }
    }

    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Slot::A => "A",
            Slot::B => "B",
        })
    }
}

/// The variables of an environment block, in file order. Variables edel
/// does not know are kept, so a rewrite never drops what a newer loader
/// added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    vars: Vec<(String, String)>,
}

impl Env {
    /// The block of a new disk: slot A boots, slot B is empty.
    pub fn initial() -> Env {
        let mut env = Env { vars: Vec::new() };
        env.set("ORDER", "A B");
        env.set_slot(Slot::A, true, 0);
        env.set_slot(Slot::B, false, 0);
        env
    }

    pub fn parse(text: &str) -> Result<Env> {
        let mut env = Env { vars: Vec::new() };
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                bail!("the GRUB environment block has a line without '=': {line:?}");
            };
            env.set(name, value);
        }
        Ok(env)
    }

    /// The block as GRUB writes it, padded with `#` to exactly [`SIZE`].
    pub fn render(&self) -> Result<String> {
        let mut text = String::from(HEADER);
        for (name, value) in &self.vars {
            text += &format!("{name}={value}\n");
        }
        let Some(padding) = SIZE.checked_sub(text.len()) else {
            bail!("GRUB variables take {} bytes, more than {SIZE}", text.len());
        };
        text += &"#".repeat(padding);
        Ok(text)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: &str) {
        match self.vars.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value.to_string(),
            None => self.vars.push((name.to_string(), value.to_string())),
        }
    }

    /// The slots in the order GRUB tries them; unknown names are skipped.
    pub fn order(&self) -> Vec<Slot> {
        self.get("ORDER")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|s| match s {
                "A" => Some(Slot::A),
                "B" => Some(Slot::B),
                _ => None,
            })
            .collect()
    }

    /// Puts `first` at the front of `ORDER`, the other slot after it.
    pub fn set_order(&mut self, first: Slot) {
        self.set("ORDER", &format!("{first} {}", first.other()));
    }

    pub fn ok(&self, slot: Slot) -> bool {
        self.get(&format!("{slot}_OK")) == Some("1")
    }

    pub fn tries(&self, slot: Slot) -> u32 {
        self.get(&format!("{slot}_TRY"))
            .and_then(|t| t.parse().ok())
            .unwrap_or(0)
    }

    pub fn set_slot(&mut self, slot: Slot, ok: bool, tries: u32) {
        self.set(&format!("{slot}_OK"), if ok { "1" } else { "0" });
        self.set(&format!("{slot}_TRY"), &tries.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_block_is_exactly_one_block() {
        let env = Env::initial().render().unwrap();
        assert_eq!(env.len(), SIZE);
        assert!(env.starts_with("# GRUB Environment Block\nORDER=A B\nA_OK=1\nA_TRY=0\n"));
        assert!(env.contains("B_OK=0\nB_TRY=0\n#"));
        assert!(env.ends_with('#'));
    }

    #[test]
    fn round_trips_and_keeps_unknown_variables() {
        let text =
            "# GRUB Environment Block\nORDER=B A\nA_OK=1\nA_TRY=2\nB_OK=0\nB_TRY=3\nNEWER=x\n###";
        let env = Env::parse(text).unwrap();
        assert_eq!(env.order(), [Slot::B, Slot::A]);
        assert!(env.ok(Slot::A) && !env.ok(Slot::B));
        assert_eq!((env.tries(Slot::A), env.tries(Slot::B)), (2, 3));
        let rendered = env.render().unwrap();
        assert_eq!(rendered.len(), SIZE);
        assert!(rendered.contains("NEWER=x\n"));
        assert_eq!(Env::parse(&rendered).unwrap(), env);
    }

    #[test]
    fn refuses_to_overflow() {
        let mut env = Env::initial();
        env.set("ORDER", &"x".repeat(SIZE));
        assert!(env.render().is_err());
    }

    #[test]
    fn refuses_a_garbled_line() {
        assert!(Env::parse("# GRUB Environment Block\nORDER\n").is_err());
    }
}
