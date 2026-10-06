//! Tab completion for `edel` (roadmap M5.26, ADR-008's same names
//! decision): a bash script written from `edel`'s own command table and
//! the settings keys, so it completes exactly what the tool takes.
//! `edel image build` writes it into every bootable image; the
//! `completion` feature brings bash, which reads it.
//!
//! It completes commands and their options, a settings key one part at a
//! time (`lay` becomes `layout.`, then the page's keys), and after `=` the
//! values a key takes. It reads the line itself (`COMP_LINE`), so it needs
//! no helper library.

use edel::system::{self, Kind};

/// Where bash-completion looks for a command's completion.
pub const PATH: &str = "/usr/share/bash-completion/completions/edel";

/// The command paths of `cmd`, each with the words that may follow it:
/// its visible subcommands and long options.
fn paths(prefix: &str, cmd: &clap::Command, out: &mut Vec<(String, Vec<String>)>) {
    let mut words: Vec<String> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| c.get_name().to_string())
        .collect();
    words.extend(
        cmd.get_arguments()
            .filter(|a| !a.is_hide_set())
            .filter_map(|a| a.get_long().map(|l| format!("--{l}"))),
    );
    words.push("--help".into());
    out.push((prefix.to_string(), words));
    for sub in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
        let path = if prefix.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{prefix} {}", sub.get_name())
        };
        paths(&path, sub, out);
    }
}

/// Every key a person can type in full: the key table's, with each
/// shortcut action named; keys under a person's or a screen's name are
/// left to the section's prefix.
fn keys() -> Vec<String> {
    let mut keys = Vec::new();
    for key in system::KEYS {
        if key.path == "shortcuts.*" {
            keys.extend(
                edel::shortcuts::ACTIONS
                    .iter()
                    .map(|a| format!("shortcuts.{}", a.name)),
            );
        } else if !key.path.contains('*') {
            keys.push(key.path.to_string());
        }
    }
    keys
}

/// The values `kind` takes, when there are few enough to list.
fn values(kind: Kind) -> Option<Vec<String>> {
    match kind {
        Kind::Flag => Some(vec!["true".into(), "false".into()]),
        Kind::OneOf(allowed) => Some(allowed.iter().map(|v| v.to_string()).collect()),
        Kind::WholeOf(allowed) => Some(allowed.iter().map(|v| v.to_string()).collect()),
        _ => None,
    }
}

/// The completion script for `cmd`, `edel`'s command table.
pub fn bash(cmd: &clap::Command) -> String {
    let mut out = String::from(
        "# bash completion for edel (roadmap M5.26): written by edel image build\n\
         # from edel's own command table and the settings keys; never edit it.\n\
         _edel() {\n\
         \tlocal line=\"${COMP_LINE:0:$COMP_POINT}\" cur path word\n\
         \tcur=${line##* }\n\
         \tpath=''\n\
         \tfor word in ${line% *}; do\n\
         \t\tcase \"$word\" in edel | -* | *=*) ;; *) path=\"${path:+$path }$word\" ;; esac\n\
         \tdone\n\
         \t[ \"$line\" = \"${line% *}\" ] && path=''\n\
         \tlocal words=''\n\
         \tcase \"$path\" in\n",
    );
    let mut all = Vec::new();
    paths("", cmd, &mut all);
    let settings_keys = keys();
    for (path, words) in &all {
        // A key or page goes after these; set's keys take a value.
        let takes_key = matches!(
            path.as_str(),
            "settings get" | "settings set" | "settings reset"
        );
        let pattern = if takes_key {
            // Several keys may follow: settings set a=1 b=2.
            format!("\"{path}\" | \"{path} \"*")
        } else {
            format!("\"{path}\"")
        };
        out.push_str(&format!("\t{pattern})\n"));
        if takes_key {
            out.push_str(&format!(
                "\t\t_edel_key \"$cur\" {}\n\t\treturn\n\t\t;;\n",
                if path == "settings set" { "=" } else { "\"\"" }
            ));
        } else {
            out.push_str(&format!("\t\twords='{}'\n\t\t;;\n", words.join(" ")));
        }
    }
    out.push_str(
        "\tesac\n\
         \tCOMPREPLY=($(compgen -W \"$words\" -- \"$cur\"))\n\
         }\n\n\
         # _edel_key CUR SUFFIX: a key one part at a time, then after = its values.\n\
         _edel_key() {\n\
         \tlocal cur=$1 key words\n\
         \tcase \"$cur\" in\n\
         \t*=*)\n\
         \t\tkey=${cur%%=*}\n\
         \t\tcase \"$key\" in\n",
    );
    for key in system::KEYS {
        if key.path.contains('*') {
            continue;
        }
        if let Some(values) = values(key.kind) {
            out.push_str(&format!(
                "\t\t{}) words='{}' ;;\n",
                key.path,
                values.join(" ")
            ));
        }
    }
    let sections: Vec<String> = system::PAGES
        .iter()
        .map(|p| format!("{}.", p.section))
        .collect();
    out.push_str(&format!(
        "\t\t*) words='' ;;\n\
         \t\tesac\n\
         \t\t# bash breaks words at =, so only the value is replaced.\n\
         \t\tCOMPREPLY=($(compgen -W \"$words\" -- \"${{cur#*=}}\"))\n\
         \t\treturn\n\
         \t\t;;\n\
         \t*.*) words='{}' ;;\n\
         \t*) words='{}' ;;\n\
         \tesac\n\
         \tCOMPREPLY=($(compgen -W \"$words\" -- \"$cur\"))\n\
         \t# A section ends in . and a key to set in =: the person types on.\n\
         \tcase \"${{COMPREPLY[0]}}\" in *. | *=) compopt -o nospace 2>/dev/null ;; esac\n\
         \t[ \"$2\" = = ] && [ \"${{cur%%.*}}\" != \"$cur\" ] &&\n\
         \t\tCOMPREPLY=($(compgen -W \"$words\" -S = -- \"$cur\")) && compopt -o nospace 2>/dev/null\n\
         }}\n\
         complete -F _edel edel\n",
        settings_keys.join(" "),
        sections.join(" ")
    ));
    out
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// Runs the script in bash for `line` and returns what it offers;
    /// `None` without bash.
    fn complete(line: &str) -> Option<Vec<String>> {
        let script = bash(&crate::cli_command());
        let out = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "{script}\nCOMP_LINE={line:?}; COMP_POINT=${{#COMP_LINE}}; _edel; printf '%s\\n' \"${{COMPREPLY[@]}}\""
            ))
            .output()
            .ok()?;
        Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
        )
    }

    #[test]
    fn completes_commands_keys_and_values() {
        let Some(found) = complete("edel se") else {
            return;
        };
        assert_eq!(found, ["settings"]);
        assert_eq!(complete("edel settings set lay").unwrap(), ["layout."]);
        assert!(
            complete("edel settings set layout.pr")
                .unwrap()
                .contains(&"layout.preset=".to_string())
        );
        assert_eq!(
            complete("edel settings set layout.preset=hi").unwrap(),
            ["hive"]
        );
        assert!(
            complete("edel settings get net")
                .unwrap()
                .contains(&"network.".to_string())
        );
        assert!(
            complete("edel settings re")
                .unwrap()
                .contains(&"reset".to_string())
        );
        assert!(
            complete("edel settings set shortcuts.close")
                .unwrap()
                .contains(&"shortcuts.close_window=".to_string())
        );
        assert!(
            complete("edel update --ch")
                .unwrap()
                .contains(&"--check".to_string())
        );
        // Hidden commands stay hidden.
        assert!(!complete("edel b").unwrap().contains(&"boot".to_string()));
    }
}
