//! The desktop session in `edel status` (roadmap M5.11, M5.25a): what the
//! session is doing, read from the state file the compositor writes, so the
//! command line sees what Settings will show.

use std::fs;

/// Where the compositor keeps what it shows (M4.3).
pub const STATE: &str = edel::places::STATE_FILE;

/// The session's effect tier, `lite`, `balanced` or `full`, while a
/// desktop session runs.
pub fn tier_now() -> Option<String> {
    tier_of(&fs::read_to_string(STATE).ok()?)
}

/// The `tier` key of a state file.
fn tier_of(text: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table.get("tier")?.as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tier_comes_from_the_state_file() {
        let state =
            "format = 1\npolicy = \"floating\"\ntier = \"lite\"\n\n[[windows]]\ntitle = \"foot\"\n";
        assert_eq!(tier_of(state).as_deref(), Some("lite"));
        assert_eq!(tier_of("format = 1\n"), None);
        assert_eq!(tier_of("not toml ["), None);
    }
}
