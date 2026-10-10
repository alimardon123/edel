//! The panel editor's model (M5.31a): the changes a person makes to the
//! panels by hand (moving, adding and removing a widget, moving a panel to
//! the other edge, adding and removing a panel), each checked as
//! `edel settings set layout.panels` checks a value, and the
//! `layout.panels` line they come to. shell-ui's editor (M5.31b) and
//! Settings (M5.31d) call it, so both write the same lines. No widget gains
//! options here: what a widget shows stays a setting of its page.

use anyhow::{Context, Result, bail};

use crate::i18n::{tr, trf};
use crate::presets::{self, Edge, Hide, Panel, Screens, Size, Style};
use crate::settings;

/// The settings key the panels are written to (M5.31b).
pub const PANELS: &str = "panels.list";

/// The settings key the apps the apps widget pins are written to, in
/// order (M5.31d): the dock's drag of an app's icon writes it.
pub const PINNED: &str = "panels.pinned";

/// The session-bus name shell-ui serves its panel editor on (M5.31d).
pub const SHELL_BUS: &str = "org.edel.Shell";
/// The object path shell-ui serves it at (M5.31d).
pub const SHELL_PATH: &str = "/org/edel/Shell";
/// The interface, whose method `EDIT_PANELS` is (M5.31d). shell-ui serves
/// them and Settings' Panels group calls them; the literals in shell-ui's
/// interface attribute are tested against these.
pub const SHELL_INTERFACE: &str = "org.edel.Shell1";
/// The method that opens the panel editor, as the drawer's Edit panels
/// row does (M5.31d).
pub const EDIT_PANELS: &str = "EditPanels";

/// The three groups of widgets a panel holds, from its start to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Start,
    Centre,
    End,
}

impl Group {
    /// The groups, in the order a panel lists them.
    pub const ALL: [Group; 3] = [Group::Start, Group::Centre, Group::End];
}

/// The widgets of one group of one panel.
fn list(panel: &Panel, group: Group) -> &Vec<String> {
    match group {
        Group::Start => &panel.start,
        Group::Centre => &panel.centre,
        Group::End => &panel.end,
    }
}

fn list_mut(panel: &mut Panel, group: Group) -> &mut Vec<String> {
    match group {
        Group::Start => &mut panel.start,
        Group::Centre => &mut panel.centre,
        Group::End => &mut panel.end,
    }
}

/// A place on the panels: a panel by its index in the list, a group on it
/// and a place in the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub panel: usize,
    pub group: Group,
    pub index: usize,
}

/// The first place `widget` lies, reading the panels from the first to the
/// last, each one's start, centre and end.
pub fn find(panels: &[Panel], widget: &str) -> Option<Spot> {
    for (panel, p) in panels.iter().enumerate() {
        for group in Group::ALL {
            if let Some(index) = list(p, group).iter().position(|w| w == widget) {
                return Some(Spot {
                    panel,
                    group,
                    index,
                });
            }
        }
    }
    None
}

/// The widget at `at`, if there is one.
fn widget_at(panels: &[Panel], at: Spot) -> Option<&str> {
    panels
        .get(at.panel)
        .and_then(|p| list(p, at.group).get(at.index))
        .map(String::as_str)
}

/// Puts `widget` at `at`: its index counted after anything removed, clamped
/// to the group's length, so an index past the end goes last.
fn put(panels: &mut [Panel], at: Spot, widget: String) -> Result<()> {
    let Some(panel) = panels.get_mut(at.panel) else {
        bail!(
            "{}",
            trf(
                "there is no panel {panel}",
                &[("panel", &at.panel.to_string())]
            )
        );
    };
    let list = list_mut(panel, at.group);
    let index = at.index.min(list.len());
    list.insert(index, widget);
    Ok(())
}

/// The panels with the widget at `from` taken out and put at `to`. Refuses
/// a `from` that names no widget and a `to` panel that does not exist, then
/// checks the result as `edel settings set` does.
pub fn move_widget(panels: &[Panel], from: Spot, to: Spot) -> Result<Vec<Panel>> {
    let Some(widget) = widget_at(panels, from).map(str::to_string) else {
        bail!("{}", tr("there is no widget at that place on the panels"));
    };
    if from.panel != to.panel {
        refuse_twice(panels, to.panel, &widget)?;
    }
    let mut out = panels.to_vec();
    list_mut(&mut out[from.panel], from.group).remove(from.index);
    put(&mut out, to, widget)?;
    presets::check_panels(&out)?;
    Ok(out)
}

/// The panels with `widget` put at `to`. Refuses a widget the panel there
/// already shows, then checks the result as `edel settings set` does, which
/// refuses a name that is not a widget name.
pub fn add_widget(panels: &[Panel], widget: &str, to: Spot) -> Result<Vec<Panel>> {
    refuse_twice(panels, to.panel, widget)?;
    let mut out = panels.to_vec();
    put(&mut out, to, widget.to_string())?;
    presets::check_panels(&out)?;
    Ok(out)
}

/// Refuses `widget` on panel `panel` when it shows it already: a panel
/// shows each widget once.
fn refuse_twice(panels: &[Panel], panel: usize, widget: &str) -> Result<()> {
    if let Some(panel) = panels.get(panel) {
        if panel.widgets().any(|w| w == widget) {
            bail!(
                "{}",
                trf(
                    "the {edge} panel already shows {widget}",
                    &[("edge", panel.edge.name()), ("widget", widget)]
                )
            );
        }
    }
    Ok(())
}

/// The panels with the widget at `at` taken out.
pub fn remove_widget(panels: &[Panel], at: Spot) -> Result<Vec<Panel>> {
    if widget_at(panels, at).is_none() {
        bail!("{}", tr("there is no widget at that place on the panels"));
    }
    let mut out = panels.to_vec();
    list_mut(&mut out[at.panel], at.group).remove(at.index);
    Ok(out)
}

/// The panel `panel` moved to `edge`. When another panel is there, the two
/// swap edges.
pub fn move_panel(panels: &[Panel], panel: usize, edge: Edge) -> Result<Vec<Panel>> {
    let Some(old) = panels.get(panel).map(|p| p.edge) else {
        bail!(
            "{}",
            trf(
                "there is no panel {panel}",
                &[("panel", &panel.to_string())]
            )
        );
    };
    let mut out = panels.to_vec();
    for (i, other) in out.iter_mut().enumerate() {
        if i != panel && other.edge == edge {
            other.edge = old;
        }
    }
    out[panel].edge = edge;
    presets::check_panels(&out)?;
    Ok(out)
}

/// The panels with an empty bar added along `edge`. Refuses an edge a panel
/// already takes.
pub fn add_panel(panels: &[Panel], edge: Edge) -> Result<Vec<Panel>> {
    if panels.iter().any(|p| p.edge == edge) {
        bail!(
            "{}",
            trf(
                "there is already a panel along the {edge} edge",
                &[("edge", edge.name())]
            )
        );
    }
    let mut out = panels.to_vec();
    out.push(Panel {
        edge,
        style: Style::Bar,
        hide: Hide::Never,
        size: Size::Medium,
        floating: false,
        screens: Screens::Main,
        start: Vec::new(),
        centre: Vec::new(),
        end: Vec::new(),
    });
    presets::check_panels(&out)?;
    Ok(out)
}

/// The panels with the panel at `panel` taken away. The last panel stays: a
/// desktop needs one to reach its menu and its windows.
pub fn remove_panel(panels: &[Panel], panel: usize) -> Result<Vec<Panel>> {
    if panel >= panels.len() {
        bail!(
            "{}",
            trf(
                "there is no panel {panel}",
                &[("panel", &panel.to_string())]
            )
        );
    }
    if panels.len() == 1 {
        bail!(
            "{}",
            trf(
                "the last panel stays: a desktop needs one to reach its menu and its windows",
                &[]
            )
        );
    }
    let mut out = panels.to_vec();
    out.remove(panel);
    Ok(out)
}

/// The panels with panel `panel` changed by `change`, checked by
/// `presets::check_panels`. Refuses a panel index that does not exist.
fn edit_panel(
    panels: &[Panel],
    panel: usize,
    change: impl FnOnce(&mut Panel),
) -> Result<Vec<Panel>> {
    if panel >= panels.len() {
        bail!(
            "{}",
            trf(
                "there is no panel {panel}",
                &[("panel", &panel.to_string())]
            )
        );
    }
    let mut out = panels.to_vec();
    change(&mut out[panel]);
    presets::check_panels(&out)?;
    Ok(out)
}

/// The panels with panel `panel` as a bar or a dock (M5.31c). A dock floats
/// already, so it stops floating; a bar does not hide, so it stops hiding.
pub fn set_style(panels: &[Panel], panel: usize, style: Style) -> Result<Vec<Panel>> {
    edit_panel(panels, panel, |p| {
        p.style = style;
        match style {
            Style::Dock => p.floating = false,
            Style::Bar => p.hide = Hide::Never,
        }
    })
}

/// The panels with panel `panel` at `size` (M5.31c).
pub fn set_size(panels: &[Panel], panel: usize, size: Size) -> Result<Vec<Panel>> {
    edit_panel(panels, panel, |p| p.size = size)
}

/// The panels with panel `panel` hiding as `hide` says. Refuses it on a bar,
/// as the check does, since only a dock hides.
pub fn set_hide(panels: &[Panel], panel: usize, hide: Hide) -> Result<Vec<Panel>> {
    edit_panel(panels, panel, |p| p.hide = hide)
}

/// The panels with panel `panel` floating or not (M5.31c). Refuses it on a
/// dock, as the check does, since a dock floats already.
pub fn set_floating(panels: &[Panel], panel: usize, floating: bool) -> Result<Vec<Panel>> {
    edit_panel(panels, panel, |p| p.floating = floating)
}

/// The panels with panel `panel` on the screens `screens` says (M5.31c).
pub fn set_screens(panels: &[Panel], panel: usize, screens: Screens) -> Result<Vec<Panel>> {
    edit_panel(panels, panel, |p| p.screens = screens)
}

/// The panels that apply, the rule shell-ui's `from_system_files` follows:
/// `panels.list`, the person's file over the machine's, else the preset
/// `layout.preset` names (the person's over the machine's). A file that
/// does not read, or is not in this release's format, counts as absent.
pub fn applying(machine: Option<&str>, person: Option<&str>) -> Vec<Panel> {
    let file = |text: Option<&str>| {
        text.and_then(|t| settings::read(t).ok())
            .map(|read| read.file)
            .unwrap_or_default()
    };
    let (machine, person) = (file(machine), file(person));
    let panels = person.panels.list.or(machine.panels.list);
    if let Some(panels) = panels {
        return panels;
    }
    let name = person.layout.preset.or(machine.layout.preset);
    presets::named(name.as_deref()).0.panels
}

/// The `layout.panels` value of `panels` as one TOML line: an inline array
/// of inline tables, exactly the line `edel settings set layout.panels=VALUE`
/// writes. `set` writes each table's keys alphabetically whatever order it
/// is given, so this does too (the empty and default fields are left out by
/// `Panel`'s serde attributes).
pub fn value(panels: &[Panel]) -> Result<String> {
    let items = panels
        .iter()
        .map(toml::Value::try_from)
        .collect::<Result<Vec<_>, _>>()
        .context("cannot write the panels")?;
    Ok(toml::Value::Array(items).to_string())
}

/// The `layout.panels` line to write for `panels`, or `None` when they are
/// what applies without the person's own `layout.panels`: a default is
/// never written (ADR-008). The person's file has its `layout.panels` taken
/// out for the comparison, and keeps its preset.
pub fn to_write(
    panels: &[Panel],
    machine: Option<&str>,
    person: Option<&str>,
) -> Result<Option<String>> {
    let without =
        person.map(|text| settings::unset(text, PANELS).unwrap_or_else(|_| text.to_string()));
    if applying(machine, without.as_deref()) == panels {
        return Ok(None);
    }
    value(panels).map(Some)
}

/// The apps pinned to the apps widget that apply (M5.31d): the person's
/// `panels.pinned` over the machine's, else the list the preset named by
/// `layout.preset` pins (the person's over the machine's, as `applying`
/// finds it). A file that does not read, or is not in this release's
/// format, counts as absent.
pub fn pins_applying(machine: Option<&str>, person: Option<&str>) -> Vec<String> {
    let read = |text: Option<&str>| {
        text.and_then(|t| settings::read(t).ok())
            .map(|read| read.file)
            .unwrap_or_default()
    };
    let (machine, person) = (read(machine), read(person));
    if let Some(pins) = person.panels.pinned.clone().or(machine.panels.pinned.clone()) {
        return pins;
    }
    let name = person.layout.preset.or(machine.layout.preset);
    presets::named(name.as_deref()).0.apps.pinned
}

/// `list` with `app` taken out where it is and put at `index`, counted
/// after that removal and clamped to the list's end (M5.31d). An app not
/// in the list is put in.
pub fn pin_at(list: &[String], app: &str, index: usize) -> Vec<String> {
    let mut out = unpin(list, app);
    out.insert(index.min(out.len()), app.to_string());
    out
}

/// `list` without `app` (M5.31d).
pub fn unpin(list: &[String], app: &str) -> Vec<String> {
    list.iter().filter(|a| a.as_str() != app).cloned().collect()
}

/// `list` as the TOML array `edel settings set apps.pinned=VALUE` takes
/// and writes, such as `["settings", "terminal"]`.
pub fn pins_value(list: &[String]) -> String {
    let items = list
        .iter()
        .map(|app| toml::Value::String(app.clone()))
        .collect();
    toml::Value::Array(items).to_string()
}

/// The `apps.pinned` value to write for `list`, or `None` when `list` is
/// what applies without the person's own line: a default is never written
/// (ADR-008). The person's `apps.pinned` is taken out for the comparison.
pub fn pins_to_write(
    list: &[String],
    machine: Option<&str>,
    person: Option<&str>,
) -> Option<String> {
    let without =
        person.map(|text| settings::unset(text, PINNED).unwrap_or_else(|_| text.to_string()));
    if pins_applying(machine, without.as_deref()) == list {
        return None;
    }
    Some(pins_value(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classic() -> Vec<Panel> {
        presets::named(Some("classic")).0.panels
    }

    #[test]
    fn the_clock_moves_from_the_end_to_the_start() {
        let panels = classic();
        let clock = find(&panels, "clock").unwrap();
        assert_eq!(
            clock,
            Spot {
                panel: 0,
                group: Group::End,
                index: 6
            }
        );
        let to = Spot {
            panel: 0,
            group: Group::Start,
            index: 0,
        };
        let moved = move_widget(&panels, clock, to).unwrap();
        assert_eq!(moved[0].start, ["clock", "menu", "separator", "windows"]);
        assert!(!moved[0].end.iter().any(|w| w == "clock"));
        let line = value(&moved).unwrap();
        assert_eq!(
            line,
            r#"[{ edge = "bottom", end = ["workspaces", "layout", "separator", "tray", "keyboard", "status"], start = ["clock", "menu", "separator", "windows"] }]"#
        );
        // The line is what `edel settings set` takes and writes, and it
        // writes the same line from the struct's own field order too.
        let file = settings::set("format = 1\n", PANELS, &line).unwrap();
        assert_eq!(file, format!("format = 1\n\n[layout]\npanels = {line}\n"));
        let in_struct_order = r#"[{ edge = "bottom", start = ["clock", "menu", "separator", "windows"], end = ["workspaces", "layout", "separator", "tray", "keyboard", "status"] }]"#;
        assert_eq!(
            settings::set("format = 1\n", PANELS, in_struct_order).unwrap(),
            file
        );
    }

    #[test]
    fn a_move_along_one_group_keeps_the_others_order() {
        let panels = classic();
        let menu = find(&panels, "menu").unwrap();
        let moved = move_widget(
            &panels,
            menu,
            Spot {
                panel: 0,
                group: Group::Start,
                index: 2,
            },
        )
        .unwrap();
        assert_eq!(moved[0].start, ["separator", "windows", "menu"]);
        assert_eq!(moved[0].end, panels[0].end, "the end is untouched");
    }

    #[test]
    fn a_move_to_an_index_past_the_end_goes_last() {
        let panels = classic();
        let menu = find(&panels, "menu").unwrap();
        let moved = move_widget(
            &panels,
            menu,
            Spot {
                panel: 0,
                group: Group::Start,
                index: 99,
            },
        )
        .unwrap();
        assert_eq!(moved[0].start, ["separator", "windows", "menu"]);
    }

    #[test]
    fn a_move_from_a_place_with_no_widget_or_to_a_missing_panel_is_refused() {
        let panels = classic();
        let nothing = Spot {
            panel: 0,
            group: Group::Centre,
            index: 0,
        };
        let to = Spot {
            panel: 0,
            group: Group::Start,
            index: 0,
        };
        let e = move_widget(&panels, nothing, to).unwrap_err();
        assert!(
            e.to_string()
                .contains("there is no widget at that place on the panels"),
            "{e}"
        );
        let clock = find(&panels, "clock").unwrap();
        let e = move_widget(
            &panels,
            clock,
            Spot {
                panel: 3,
                group: Group::Start,
                index: 0,
            },
        )
        .unwrap_err();
        assert!(e.to_string().contains("there is no panel 3"), "{e}");
    }

    #[test]
    fn add_widget_refuses_a_widget_the_panel_already_shows() {
        let panels = classic();
        let to = Spot {
            panel: 0,
            group: Group::Start,
            index: 0,
        };
        let e = add_widget(&panels, "clock", to).unwrap_err();
        assert!(
            e.to_string()
                .contains("the bottom panel already shows clock"),
            "{e}"
        );
    }

    #[test]
    fn a_move_onto_another_panel_that_shows_the_widget_is_refused() {
        let panels = add_panel(&classic(), Edge::Top).unwrap();
        let top = panels.iter().position(|p| p.edge == Edge::Top).unwrap();
        let bottom = 1 - top;
        let panels = add_widget(
            &panels,
            "clock",
            Spot {
                panel: top,
                group: Group::End,
                index: 0,
            },
        )
        .unwrap();
        let from = find(&panels[bottom..=bottom], "clock").unwrap();
        let from = Spot {
            panel: bottom,
            ..from
        };
        let to = Spot {
            panel: top,
            group: Group::Start,
            index: 0,
        };
        let e = move_widget(&panels, from, to).unwrap_err();
        assert!(
            e.to_string().contains("the top panel already shows clock"),
            "{e}"
        );
    }

    #[test]
    fn add_widget_refuses_a_name_that_is_not_a_widget_name_as_settings_does() {
        let panels = classic();
        let to = Spot {
            panel: 0,
            group: Group::Start,
            index: 0,
        };
        let here = add_widget(&panels, "Clock!", to).unwrap_err().to_string();
        let file = settings::set(
            "format = 1\n",
            "layout.panels",
            r#"[{ edge = "top", end = ["Clock!"] }]"#,
        )
        .unwrap_err()
        .to_string();
        assert!(here.contains("\"Clock!\" is not a widget name"), "{here}");
        assert!(file.contains("\"Clock!\" is not a widget name"), "{file}");
    }

    #[test]
    fn move_panel_to_a_taken_edge_swaps_the_two() {
        let panels = add_panel(&classic(), Edge::Top).unwrap();
        let swapped = move_panel(&panels, 1, Edge::Bottom).unwrap();
        assert_eq!(swapped[0].edge, Edge::Top);
        assert_eq!(swapped[1].edge, Edge::Bottom);
        assert_eq!(
            swapped[0].end, panels[0].end,
            "the widgets stay with the bar"
        );
    }

    #[test]
    fn add_panel_on_a_taken_edge_is_refused() {
        let e = add_panel(&classic(), Edge::Bottom).unwrap_err();
        assert!(
            e.to_string()
                .contains("there is already a panel along the bottom edge"),
            "{e}"
        );
    }

    #[test]
    fn remove_panel_refuses_the_last_one() {
        let e = remove_panel(&classic(), 0).unwrap_err();
        assert!(e.to_string().contains("the last panel stays"), "{e}");
        let two = add_panel(&classic(), Edge::Top).unwrap();
        assert_eq!(remove_panel(&two, 1).unwrap().len(), 1);
    }

    #[test]
    fn to_write_is_none_for_what_applies_and_some_after_a_move() {
        let panels = classic();
        assert_eq!(to_write(&panels, None, None).unwrap(), None);
        let clock = find(&panels, "clock").unwrap();
        let moved = move_widget(
            &panels,
            clock,
            Spot {
                panel: 0,
                group: Group::Start,
                index: 0,
            },
        )
        .unwrap();
        assert_eq!(
            to_write(&moved, None, None).unwrap(),
            Some(value(&moved).unwrap())
        );
    }

    #[test]
    fn to_write_is_none_when_the_machine_file_already_says_these_panels() {
        let machine = "format = 1\n[layout]\npanels = [{ edge = \"bottom\", end = [\"clock\"] }]\n";
        let panels = applying(Some(machine), None);
        assert_eq!(to_write(&panels, Some(machine), None).unwrap(), None);
        // The person's own layout.panels is taken out of the comparison,
        // so a file that sets the same panels itself still writes none.
        let person = "format = 1\n[layout]\npanels = [{ edge = \"bottom\", end = [\"clock\"] }]\n";
        assert_eq!(
            to_write(&panels, Some(machine), Some(person)).unwrap(),
            None
        );
    }

    #[test]
    fn value_writes_the_size_floating_and_screens_in_alphabetical_order() {
        let mut panels = classic();
        panels[0].size = Size::Large;
        panels[0].floating = true;
        panels[0].screens = Screens::Every;
        let line = value(&panels).unwrap();
        assert_eq!(
            line,
            r#"[{ edge = "bottom", end = ["workspaces", "layout", "separator", "tray", "keyboard", "status", "clock"], floating = true, screens = "every", size = "large", start = ["menu", "separator", "windows"] }]"#
        );
        let file = settings::set("format = 1\n", PANELS, &line).unwrap();
        let read = settings::read(&file).unwrap();
        assert_eq!(read.file.panels.list.unwrap(), panels);
        // The defaults are left out, as they are in a preset's file.
        let plain = value(&classic()).unwrap();
        assert!(
            !plain.contains("size") && !plain.contains("floating"),
            "{plain}"
        );
    }

    #[test]
    fn applying_takes_the_persons_panels_over_the_machines() {
        let machine = "format = 1\n[layout]\npanels = [{ edge = \"bottom\", end = [\"clock\"] }]\n";
        let person = "format = 1\n[layout]\npanels = [{ edge = \"top\", end = [\"clock\"] }]\n";
        let panels = applying(Some(machine), Some(person));
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].edge, Edge::Top);
        let hive = "format = 1\n[layout]\npreset = \"hive\"\n";
        assert_eq!(
            applying(None, Some(hive)),
            presets::named(Some("hive")).0.panels
        );
    }

    #[test]
    fn set_style_makes_a_dock_and_the_line_says_so() {
        let docked = set_style(&classic(), 0, Style::Dock).unwrap();
        assert_eq!(docked[0].style, Style::Dock);
        let line = value(&docked).unwrap();
        assert!(line.contains(r#"style = "dock""#), "{line}");
        let file = settings::set("format = 1\n", PANELS, &line).unwrap();
        assert_eq!(
            settings::read(&file).unwrap().file.panels.list.unwrap(),
            docked
        );
    }

    #[test]
    fn a_dock_stops_floating_and_a_bar_stops_hiding() {
        let floated = set_floating(&classic(), 0, true).unwrap();
        let docked = set_style(&floated, 0, Style::Dock).unwrap();
        assert!(!docked[0].floating, "a dock floats already");
        let hiding = set_hide(&docked, 0, Hide::Covered).unwrap();
        let bar = set_style(&hiding, 0, Style::Bar).unwrap();
        assert_eq!(bar[0].hide, Hide::Never, "only a dock hides");
        assert_eq!(bar[0].style, Style::Bar);
    }

    #[test]
    fn set_size_screens_hide_and_floating_change_their_own_field() {
        let panels = classic();
        assert_eq!(
            set_size(&panels, 0, Size::Large).unwrap()[0].size,
            Size::Large
        );
        assert_eq!(
            set_screens(&panels, 0, Screens::Every).unwrap()[0].screens,
            Screens::Every
        );
        assert!(set_floating(&panels, 0, true).unwrap()[0].floating);
        let docked = set_style(&panels, 0, Style::Dock).unwrap();
        assert_eq!(
            set_hide(&docked, 0, Hide::Covered).unwrap()[0].hide,
            Hide::Covered
        );
    }

    #[test]
    fn a_bar_refuses_hiding_and_a_dock_refuses_floating_as_the_check_says() {
        let e = set_hide(&classic(), 0, Hide::Covered).unwrap_err();
        assert!(
            e.to_string()
                .contains("only a dock hides; the bottom panel is a bar"),
            "{e}"
        );
        let docked = set_style(&classic(), 0, Style::Dock).unwrap();
        let e = set_floating(&docked, 0, true).unwrap_err();
        assert!(
            e.to_string()
                .contains("a dock floats already; take floating out of the bottom panel"),
            "{e}"
        );
    }

    #[test]
    fn a_change_to_a_panel_that_does_not_exist_is_refused() {
        let panels = classic();
        let e = set_size(&panels, 3, Size::Large).unwrap_err();
        assert!(e.to_string().contains("there is no panel 3"), "{e}");
        let e = set_style(&panels, 3, Style::Dock).unwrap_err();
        assert!(e.to_string().contains("there is no panel 3"), "{e}");
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_pins_are_the_persons_then_the_machines_then_the_presets() {
        let mac = presets::named(Some("mac-like")).0.apps.pinned;
        assert_eq!(
            pins_applying(None, None),
            Vec::<String>::new(),
            "Classic pins none"
        );
        assert_eq!(
            pins_applying(None, Some("format = 1\n[layout]\npreset = \"mac-like\"\n")),
            mac
        );
        let machine = "format = 1\n[apps]\npinned = [\"foot\"]\n";
        assert_eq!(pins_applying(Some(machine), None), ["foot"]);
        let person = "format = 1\n[apps]\npinned = [\"mail\", \"terminal\"]\n";
        assert_eq!(
            pins_applying(Some(machine), Some(person)),
            ["mail", "terminal"]
        );
        // The person's empty list pins none, over the machine's.
        let none = "format = 1\n[apps]\npinned = []\n";
        assert!(pins_applying(Some(machine), Some(none)).is_empty());
        // The machine's preset, when neither file sets the list.
        let hive = "format = 1\n[layout]\npreset = \"hive\"\n";
        assert!(pins_applying(Some(hive), None).is_empty());
    }

    #[test]
    fn an_app_moves_to_an_index_counted_after_it_is_taken_out() {
        let list = strings(&["files", "browser", "terminal"]);
        assert_eq!(
            pin_at(&list, "terminal", 0),
            ["terminal", "files", "browser"]
        );
        assert_eq!(
            pin_at(&list, "files", 2),
            ["browser", "terminal", "files"],
            "removed first, so index 2 is last"
        );
        assert_eq!(
            pin_at(&list, "files", 99),
            ["browser", "terminal", "files"],
            "clamped"
        );
        // An app not in the list is put in, at the index.
        assert_eq!(
            pin_at(&list, "foot", 1),
            ["files", "foot", "browser", "terminal"]
        );
    }

    #[test]
    fn an_app_is_unpinned_and_the_others_keep_their_order() {
        let list = strings(&["files", "browser", "terminal"]);
        assert_eq!(unpin(&list, "browser"), ["files", "terminal"]);
        assert_eq!(unpin(&list, "foot"), list, "not pinned: nothing changes");
    }

    #[test]
    fn the_value_is_the_array_settings_set_writes() {
        let list = strings(&["settings", "terminal"]);
        let value = pins_value(&list);
        assert_eq!(value, r#"["settings", "terminal"]"#);
        let file = settings::set("format = 1\n", PINNED, &value).unwrap();
        assert_eq!(
            file,
            "format = 1\n\n[apps]\npinned = [\"settings\", \"terminal\"]\n"
        );
        assert_eq!(pins_value(&[]), "[]");
    }

    #[test]
    fn a_default_is_never_written_and_a_persons_own_line_is_taken_out() {
        let list = strings(&["settings", "terminal"]);
        assert_eq!(
            pins_to_write(&list, None, None),
            Some(r#"["settings", "terminal"]"#.to_string())
        );
        // What applies without the person's line is written as no line.
        let machine = "format = 1\n[apps]\npinned = [\"settings\", \"terminal\"]\n";
        assert_eq!(pins_to_write(&list, Some(machine), None), None);
        // A person's line equal to the preset's is taken out, not kept.
        let mac = presets::named(Some("mac-like")).0.apps.pinned;
        let person = "format = 1\n[layout]\npreset = \"mac-like\"\n[apps]\npinned = [\"files\", \"browser\", \"mail\", \"music\", \"editor\", \"terminal\", \"settings\"]\n";
        assert_eq!(mac.len(), 7);
        assert_eq!(pins_to_write(&mac, None, Some(person)), None);
        // An empty list is a choice, written.
        assert_eq!(
            pins_to_write(&[], Some(machine), None),
            Some("[]".to_string())
        );
    }
}
