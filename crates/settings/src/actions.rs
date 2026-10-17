//! The actions a person takes in Settings, each with the page its button is
//! on and the `edel` command that does the same (roadmap M6.11). ADR-008
//! says every action is a Settings button and an `edel` command with the
//! same words, and ADR-010 says every fact has one owner: this table owns
//! those words. The pages read their `edel` arguments from here, so a
//! command's words are written once, and the tests at the end fail when an
//! action lacks a page Settings shows or a command the reference lists.

/// One action a person takes: the page its button is on and the `edel`
/// command that does the same.
pub struct Action {
    /// The page's section, as `edel::settings::PAGES` names it (`updates`), or the installer page's name.
    pub page: &'static str,
    /// `edel`'s arguments, such as `["update", "--check"]`; a file or key the person picks is added after them.
    pub args: &'static [&'static str],
}

/// The Updates page's Check for updates button.
pub const CHECK: Action = Action {
    page: "updates",
    args: &["update", "--check"],
};

/// The Updates page's Install the update button.
pub const UPDATE: Action = Action {
    page: "updates",
    args: &["update"],
};

/// The Updates page's Roll back button, which asks twice and then goes back.
pub const ROLLBACK: Action = Action {
    page: "updates",
    args: &["rollback"],
};

/// The Updates page's Restart now button.
pub const RESTART: Action = Action {
    page: "updates",
    args: &["restart"],
};

/// The System page's Save as button, which writes this machine's settings to a file.
pub const EXPORT: Action = Action {
    page: "system",
    args: &["settings", "export"],
};

/// The System page's Choose file button, which shows what a file would change;
/// the file is added after the words.
pub const DIFF: Action = Action {
    page: "system",
    args: &["settings", "diff"],
};

/// The System page's Apply button, which makes a file this machine's settings;
/// the file is added after the words. `edel settings import` applies it and
/// keeps it as the machine's own file.
pub const IMPORT: Action = Action {
    page: "system",
    args: &["settings", "import"],
};

/// Every row's Reset button: the key is added after the words, as the
/// person's line is taken out of the settings file.
pub const RESET: Action = Action {
    page: "system",
    args: &["settings", "reset"],
};

/// The Region page's Time zone and Time servers changes, and the System
/// page's developer mode switch (M7.1), which set one machine key as root;
/// the key and its value are added after the words.
pub const SET: Action = Action {
    page: "region",
    args: &["settings", "set"],
};

/// The apply that follows a machine key being set, so the Region page's
/// clock and the System page's developer mode take effect at once (as root,
/// through `doas`, like SET).
pub const APPLY: Action = Action {
    page: "region",
    args: &["settings", "apply"],
};

/// The installer page's Erase and install button, which runs `edel install`
/// with the disk and its options added after the words.
pub const INSTALL: Action = Action {
    page: "Install Edel OS",
    args: &["install"],
};

/// The Welcome's Start using Edel OS on a computer nobody has an account on
/// (M6.12), which makes its first person; the person's name, the password on
/// standard input and the choices made follow these words. `first-boot` is
/// hidden from `edel --help` and the command reference, as it works only
/// while nobody has an account, so [`ACTIONS`], which the tests hold to the
/// reference, leaves it out.
pub const FIRST_PERSON: Action = Action {
    page: "Welcome",
    args: &["settings", "first-boot"],
};

/// The System page's Reset developer mode button (M7.1), which deletes the
/// changes developer mode made under /usr when the computer next starts and
/// takes its key out of the settings file; the page asks twice first, as
/// Roll back does. Apply follows it, as it follows SET.
pub const RESET_DEVELOPER: Action = Action {
    page: "system",
    args: &["settings", "reset", "--yes", "system.developer_mode"],
};

// `edel addon list` joins this table when add-ons and their page come (M7.2):
// the roadmap's list names it, but the command does not exist yet.

/// Every action of the table, in the order the pages show them. Only the
/// tests read the list: they check each action in it.
#[cfg(test)]
pub const ACTIONS: &[&Action] = &[
    &CHECK,
    &UPDATE,
    &ROLLBACK,
    &RESTART,
    &EXPORT,
    &DIFF,
    &IMPORT,
    &RESET,
    &RESET_DEVELOPER,
    &SET,
    &APPLY,
    &INSTALL,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The installer page's name, as `main.rs` titles it.
    const INSTALLER: &str = "Install Edel OS";

    /// The command reference, written by edel's own test from its command table.
    const REFERENCE: &str = include_str!("../../../docs/guide/commands.md");

    /// The command an action runs: `edel` and its words, with options and
    /// keys left out, as the reference writes a key as `KEY...` (M7.1).
    fn command(action: &Action) -> String {
        std::iter::once("edel")
            .chain(
                action
                    .args
                    .iter()
                    .copied()
                    .filter(|word| !word.starts_with('-') && !word.contains('.')),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The heading level and text of a Markdown heading line, if it is one.
    fn heading(line: &str) -> Option<(usize, &str)> {
        let text = line.trim_start_matches('#');
        let level = line.len() - text.len();
        (level > 0 && text.starts_with(' ')).then(|| (level, text.trim_start()))
    }

    /// Whether `line` is the heading of `command`'s section, such as
    /// ``## `edel update [RELEASE] [OPTIONS]` `` for `edel update`.
    fn heads(line: &str, command: &str) -> bool {
        heading(line).is_some_and(|(_, text)| {
            text.strip_prefix('`')
                .and_then(|text| text.strip_prefix(command))
                .is_some_and(|rest| rest.starts_with([' ', '`']))
        })
    }

    /// The reference's section of `command`: its heading down to the next
    /// heading of the same level or higher.
    fn section(reference: &str, command: &str) -> Option<String> {
        let lines: Vec<&str> = reference.lines().collect();
        let start = lines.iter().position(|line| heads(line, command))?;
        let level = heading(lines[start]).map_or(0, |(level, _)| level);
        let end = lines[start + 1..]
            .iter()
            .position(|line| heading(line).is_some_and(|(next, _)| next <= level))
            .map_or(lines.len(), |at| start + 1 + at);
        Some(lines[start..end].join("\n"))
    }

    #[test]
    fn every_action_names_a_page_settings_shows() {
        for action in ACTIONS {
            let on_settings = edel::settings::PAGES
                .iter()
                .any(|page| page.section == action.page);
            let installer = action.page == INSTALLER;
            assert!(
                on_settings || installer,
                "the action `{}` names the page {:?}, which Settings does not show",
                command(action),
                action.page
            );
        }
        assert!(
            include_str!("main.rs").contains(&format!("title: tr(\"{INSTALLER}\")")),
            "the installer page's title in main.rs is the name the table holds"
        );
    }

    #[test]
    fn every_action_is_a_command_edel_has() {
        for action in ACTIONS {
            let command = command(action);
            let top = command.split(' ').nth(1).unwrap_or_default();
            assert!(
                REFERENCE.contains(&format!("| `edel {top}` |")),
                "the command reference has no `edel {top}` row for `{command}`"
            );
            let section = section(REFERENCE, &command)
                .unwrap_or_else(|| panic!("the command reference has no heading for `{command}`"));
            for option in action.args.iter().filter(|word| word.starts_with('-')) {
                assert!(
                    section.contains(&format!("`{option}`")),
                    "`{command}`'s section in the command reference has no `{option}`"
                );
            }
        }
    }

    #[test]
    fn the_roadmap_s_actions_each_have_a_button() {
        let words: Vec<String> = ACTIONS.iter().map(|action| action.args.join(" ")).collect();
        for wanted in [
            "update",
            "update --check",
            "rollback",
            "settings export",
            "settings diff",
            "settings import",
            "settings reset",
            "settings apply",
            "install",
        ] {
            assert!(
                words.iter().any(|word| word == wanted),
                "the roadmap's action `edel {wanted}` has no button in the table"
            );
        }
    }

    #[test]
    fn no_action_is_listed_twice() {
        let mut seen: Vec<String> = Vec::new();
        for action in ACTIONS {
            let words = action.args.join(" ");
            assert!(!seen.contains(&words), "`edel {words}` is listed twice");
            seen.push(words);
        }
    }
}
