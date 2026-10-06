//! Keyboard layouts (roadmap M5.21): `region.keyboard`, one layout or
//! several, each with a variant if wanted, written as xkb names them:
//! `"de"`, `"us,ru"`, `"de(nodeadkeys),us"`. The settings check refuses a
//! layout or variant xkeyboard-config lacks when its list is on the
//! machine (the desktop image has it); without the list only the spelling
//! is checked. The compositor hands the layouts to xkb, and Super+Space
//! (`next_keyboard_layout`) goes to the next one.

use std::collections::BTreeSet;

/// xkeyboard-config's list of layouts and variants, the one the settings
/// check reads.
pub const RULES: &str = "/usr/share/X11/xkb/rules/base.lst";

/// xkb holds at most four layouts at once.
pub const MOST_LAYOUTS: usize = 4;

/// One layout, as xkb names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub variant: Option<String>,
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.variant {
            Some(variant) => write!(f, "{}({variant})", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

/// The layouts `value` names, in order, or what is wrong with its
/// spelling.
pub fn parse(value: &str) -> Result<Vec<Layout>, String> {
    let mut layouts = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        let (name, variant) = match part.split_once('(') {
            Some((name, rest)) => {
                let variant = rest.strip_suffix(')').ok_or_else(|| {
                    format!(
                        "{part:?} opens a variant it does not close; write it as \"de(nodeadkeys)\""
                    )
                })?;
                (name, Some(variant))
            }
            None => (part, None),
        };
        if !is_xkb_name(name) {
            return Err(if part.is_empty() {
                "a layout is missing; write layouts as \"us\" or \"us,ru\"".to_string()
            } else {
                format!(
                    "{name:?} is not a layout name; layouts are short names such as \"us\", \"de\" or \"ru\""
                )
            });
        }
        if let Some(variant) = variant.filter(|v| !is_xkb_name(v)) {
            return Err(format!(
                "{variant:?} is not a variant name; variants are names such as \"nodeadkeys\""
            ));
        }
        layouts.push(Layout {
            name: name.to_string(),
            variant: variant.map(str::to_string),
        });
    }
    if layouts.len() > MOST_LAYOUTS {
        return Err(format!(
            "{} layouts given; a keyboard holds at most {MOST_LAYOUTS}",
            layouts.len()
        ));
    }
    Ok(layouts)
}

/// Lower case letters, digits, `_` and `-`, as xkb names are.
fn is_xkb_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The layouts and the variants (`layout(variant)`) xkeyboard-config's
/// list `rules` names.
pub fn known(rules: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut layouts, mut variants) = (BTreeSet::new(), BTreeSet::new());
    let mut section = "";
    for line in rules.lines() {
        if let Some(name) = line.strip_prefix("! ") {
            section = name.trim();
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(name) = words.next() else {
            continue;
        };
        match section {
            "layout" => {
                layouts.insert(name.to_string());
            }
            // "  nodeadkeys      de: German (no dead keys)"
            "variant" => {
                if let Some(layout) = words.next().and_then(|w| w.strip_suffix(':')) {
                    variants.insert(format!("{layout}({name})"));
                }
            }
            _ => {}
        }
    }
    (layouts, variants)
}

/// `value` written the one way (no spaces), checked against `rules`
/// when the machine has xkeyboard-config's list, else what is wrong.
pub fn normalize(value: &str, rules: Option<&str>) -> Result<String, String> {
    let layouts = parse(value)?;
    if let Some(rules) = rules {
        let (names, variants) = known(rules);
        for layout in &layouts {
            if !names.contains(&layout.name) {
                return Err(format!(
                    "{:?} is not a keyboard layout this machine knows; layouts are short names such as \"us\", \"de\", \"fr\" or \"ru\" (the list is {RULES})",
                    layout.name
                ));
            }
            if layout.variant.is_some() && !variants.contains(&layout.to_string()) {
                return Err(format!(
                    "{layout} is not a variant of the {} layout this machine knows (the list is {RULES})",
                    layout.name
                ));
            }
        }
    }
    Ok(layouts
        .iter()
        .map(Layout::to_string)
        .collect::<Vec<_>>()
        .join(","))
}

/// xkb's layout and variant lists for `layouts`: `("us,de", ",nodeadkeys")`.
pub fn xkb_names(layouts: &[Layout]) -> (String, String) {
    let names: Vec<&str> = layouts.iter().map(|l| l.name.as_str()).collect();
    let variants: Vec<&str> = layouts
        .iter()
        .map(|l| l.variant.as_deref().unwrap_or(""))
        .collect();
    let variant = if variants.iter().all(|v| v.is_empty()) {
        String::new()
    } else {
        variants.join(",")
    };
    (names.join(","), variant)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES_SAMPLE: &str = "\
! model
  pc105           Generic 105-key PC

! layout
  us              English (US)
  de              German
  ru              Russian

! variant
  chr             us: Cherokee
  nodeadkeys      de: German (no dead keys)

! option
  grp             Switching to another layout
";

    #[test]
    fn reads_one_layout_several_and_variants() {
        assert_eq!(normalize("de", None).unwrap(), "de");
        assert_eq!(normalize(" us , ru ", None).unwrap(), "us,ru");
        let layouts = parse("de(nodeadkeys),us").unwrap();
        assert_eq!(layouts[0].variant.as_deref(), Some("nodeadkeys"));
        assert_eq!(
            xkb_names(&layouts),
            ("de,us".to_string(), "nodeadkeys,".to_string())
        );
        assert_eq!(
            xkb_names(&parse("us,ru").unwrap()),
            ("us,ru".into(), String::new())
        );
    }

    #[test]
    fn refuses_what_xkb_could_not_read_and_says_how_to_write_it() {
        assert!(parse("").unwrap_err().contains("missing"));
        assert!(parse("us,,ru").unwrap_err().contains("missing"));
        assert!(parse("German").unwrap_err().contains("not a layout name"));
        assert!(
            parse("de(nodeadkeys")
                .unwrap_err()
                .contains("does not close")
        );
        assert!(parse("us,de,fr,ru,ua").unwrap_err().contains("at most 4"));
    }

    #[test]
    fn checks_names_against_xkeyboard_configs_list() {
        let (layouts, variants) = known(RULES_SAMPLE);
        assert!(layouts.contains("de") && !layouts.contains("pc105"));
        assert!(variants.contains("de(nodeadkeys)") && !variants.contains("grp"));
        assert_eq!(
            normalize("de(nodeadkeys),us", Some(RULES_SAMPLE)).unwrap(),
            "de(nodeadkeys),us"
        );
        let err = normalize("xx", Some(RULES_SAMPLE)).unwrap_err();
        assert!(err.contains("\"xx\" is not a keyboard layout"), "{err}");
        let err = normalize("us(nodeadkeys)", Some(RULES_SAMPLE)).unwrap_err();
        assert!(err.contains("not a variant of the us layout"), "{err}");
    }
}
