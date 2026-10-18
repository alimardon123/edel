//! The docs site's page of the machines that sent `edel report` (roadmap
//! M8.1): `docs/hardware.md`, written from `docs/hardware/*.toml` by a cargo
//! test, as `docs/guide/commands.md` is written from the command table. Each
//! report is read leniently with the types `report.rs` writes, so a report
//! from another release still makes its row; a file that is not a report
//! fails the test, naming it. It is test-only: `#![cfg(test)]` keeps it out
//! of the binary, and writing the attribute here rather than as `#[cfg(test)]`
//! in `main.rs` keeps `i18n`'s scan of that file whole.

#![cfg(test)]

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::report::{BasicAnswer, FrameTimes, Measured};

/// A report as this table reads it: only what the table shows, and nothing
/// a report of another release may lack.
#[derive(Deserialize)]
struct Sent {
    #[serde(default)]
    model: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    measured: Measured,
    #[serde(default)]
    frame_times: Vec<FrameTimes>,
    #[serde(default)]
    basics: Vec<BasicAnswer>,
}

/// The answer a `works` value gives: `Some(true)` for "yes", `Some(false)`
/// for "no", and None for anything else, which counts as not answered.
fn answer(works: &str) -> Option<bool> {
    match works.trim().to_ascii_lowercase().as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// A table cell's text with its pipes escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The page for `reports`, given as (file name, text) pairs sorted by file
/// name: one row per report, then the basics that do not work (M8.1).
pub(crate) fn page(reports: &[(String, String)]) -> Result<String> {
    let mut rows = String::new();
    let mut broken = String::new();
    for (file, text) in reports {
        let sent: Sent = toml::from_str(text)
            .with_context(|| format!("docs/hardware/{file} is not a report"))?;
        let model = sent.model.trim();
        let label = if model.is_empty() {
            file.strip_suffix(".toml").unwrap_or(file)
        } else {
            model
        };
        let start = sent
            .measured
            .boot_seconds
            .map_or("not measured".to_string(), |s| format!("{s:.2} s"));
        let memory = sent
            .measured
            .memory_at_boot_mib
            .map_or("not measured".to_string(), |mib| format!("{mib} MiB"));
        let render = sent
            .frame_times
            .iter()
            .map(|f| f.render_p99_ms)
            .reduce(f64::max)
            .map_or("not measured".to_string(), |ms| format!("{ms:.2} ms"));
        let answered = sent
            .basics
            .iter()
            .filter(|b| answer(&b.works).is_some())
            .count();
        let yes = sent
            .basics
            .iter()
            .filter(|b| answer(&b.works) == Some(true))
            .count();
        let basics = if answered == 0 {
            "none answered".to_string()
        } else {
            format!("{yes} of {answered} answered")
        };
        let machine = format!("{label} ([report](hardware/{file}))");
        rows.push_str(&format!(
            "| {} | {} | {start} | {memory} | {render} | {basics} |\n",
            cell(&machine),
            cell(&sent.version),
        ));
        let no: Vec<&str> = sent
            .basics
            .iter()
            .filter(|b| answer(&b.works) == Some(false))
            .map(|b| b.name.as_str())
            .collect();
        if !no.is_empty() {
            broken.push_str(&format!("- {label}: {}\n", no.join(", ")));
        }
    }
    let mut out = String::from(
        "<!-- Written by a cargo test from docs/hardware/*.toml, crates/edel/src/hardware_table.rs; never edit it by hand: add a report and run EDEL_WRITE_DOCS=1 cargo test -p edel hardware_table. -->\n\n\
         # Hardware reports\n\n\
         Each machine below sent `edel report` (roadmap M8.1): the stick writes it to its EFI system partition as `EFI/edel/report.toml` at every start, and on an installed machine `edel report > report.toml` prints it. Copy it off the stick before setting the answers: the stick writes it again at every start. Before attaching it to an issue, set each `[[basics]]` row's `works` to \"yes\" or \"no\". The report holds no personal files.\n\n\
         | Machine | Release | Start | Memory at start | Frame render p99 | Basics that work |\n\
         |---|---|---|---|---|---|\n",
    );
    out.push_str(&rows);
    if !broken.is_empty() {
        out.push_str("\n## Basics that do not work\n\n");
        out.push_str(&broken);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// The docs page is the reports in `docs/hardware/`: written by
    /// `EDEL_WRITE_DOCS=1 cargo test -p edel hardware_table`, and failing
    /// while the file on disk differs.
    #[test]
    fn the_hardware_page_is_the_reports() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/hardware");
        let mut files: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect();
        files.sort();
        let reports: Vec<(String, String)> = files
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    fs::read_to_string(format!("{dir}/{name}")).unwrap(),
                )
            })
            .collect();
        let text = page(&reports).unwrap();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/hardware.md");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            fs::write(path, &text).unwrap();
        }
        let shipped = fs::read_to_string(path).unwrap_or_default();
        assert!(
            shipped == text,
            "docs/hardware.md differs from docs/hardware/*.toml; run EDEL_WRITE_DOCS=1 cargo test -p edel hardware_table"
        );
    }

    #[test]
    fn a_report_without_measurements_says_so() {
        let text = page(&[("bare.toml".into(), "format = 1\n".into())]).unwrap();
        assert!(
            text.ends_with(
                "| bare ([report](hardware/bare.toml)) |  | not measured | not measured | not measured | none answered |\n"
            ),
            "{text}"
        );
        assert!(!text.contains("## Basics that do not work"), "{text}");
    }

    #[test]
    fn a_row_that_says_no_gets_a_section() {
        let report = r#"
model = "Edel Test"
version = "2026.10.1"

[measured]
boot_seconds = 6.78
memory_at_boot_mib = 60

[[frame_times]]
person = "ci"
frames = 300
render_p50_ms = 1.2
render_p99_ms = 3.4
idle_frames = 0

[[basics]]
feature = "sound"
name = "Audio"
test = "sound"
works = "yes"

[[basics]]
feature = "power"
name = "Battery"
needs = "hardware"
works = "not answered"

[[basics]]
feature = "capture"
name = "Camera"
needs = "hardware"
works = "no"
"#;
        let text = page(&[("edel-test.toml".into(), report.into())]).unwrap();
        assert!(
            text.contains(
                "| Edel Test ([report](hardware/edel-test.toml)) | 2026.10.1 | 6.78 s | 60 MiB | 3.40 ms | 1 of 2 answered |\n"
            ),
            "{text}"
        );
        assert!(
            text.ends_with("\n## Basics that do not work\n\n- Edel Test: Camera\n"),
            "{text}"
        );
    }

    #[test]
    fn a_pipe_in_a_cell_is_escaped() {
        let text = page(&[("pipe.toml".into(), "model = \"A|B\"\n".into())]).unwrap();
        assert!(
            text.contains("| A\\|B ([report](hardware/pipe.toml)) |"),
            "{text}"
        );
    }

    #[test]
    fn a_text_that_is_not_a_report_is_an_error_naming_the_file() {
        let err = page(&[("broken.toml".into(), "format = [".into())]).unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.starts_with("docs/hardware/broken.toml is not a report: "),
            "{text}"
        );
    }
}
