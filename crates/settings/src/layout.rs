//! The Layout page (M5.6a, M5.6b): one row per preset and whether windows
//! tile, each change one line of the person's settings file, which the
//! desktop follows at once. Each group says where its value comes from,
//! with Reset for the person's own choice and Copy as command for the
//! line that sets it (ADR-008).

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::files::{self, Files};
use crate::rows;

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    checks: Vec<(&'static str, gtk::CheckButton)>,
    presets: adw::PreferencesGroup,
    preset_reset: gtk::Button,
    tiling: adw::SwitchRow,
    tiling_reset: gtk::Button,
    /// Set while the page itself moves a switch or a radio, so that is
    /// not a choice.
    quiet: Cell<bool>,
}

impl Ui {
    /// Shows what the files say now: the chosen preset, whether windows
    /// tile, and where each comes from.
    fn update(&self) {
        let files = Files::here();
        let now = files.layout();
        self.quiet.set(true);
        for (name, check) in &self.checks {
            check.set_active(*name == now.preset);
        }
        self.tiling.set_active(now.tiling);
        self.quiet.set(false);
        let preset = files.source("layout.preset");
        self.presets
            .set_description(Some(&rows::describe(&preset, &files::title(&now.preset))));
        self.preset_reset.set_visible(rows::resettable(&preset));
        let tiling = files.source("layout.tiling");
        self.tiling.set_subtitle(&rows::describe(
            &tiling,
            if now.tiling { "on" } else { "off" },
        ));
        self.tiling_reset.set_visible(rows::resettable(&tiling));
    }
}

pub fn page() -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Layout")
        .icon_name("view-grid-symbolic")
        .build();
    let presets = adw::PreferencesGroup::builder()
        .title(rows::title("layout.preset"))
        .build();
    let mut checks = Vec::new();
    let mut first: Option<gtk::CheckButton> = None;
    for name in edel::presets::NAMES {
        let check = gtk::CheckButton::new();
        match &first {
            Some(first) => check.set_group(Some(first)),
            None => first = Some(check.clone()),
        }
        let row = adw::ActionRow::builder()
            .title(files::title(name))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        presets.add(&row);
        checks.push((*name, check));
    }
    let tiling = adw::SwitchRow::builder()
        .title(rows::title("layout.tiling"))
        .build();
    let windows = adw::PreferencesGroup::builder()
        .title("Windows")
        .description("Side by side, each with its title bar; Super+T switches one workspace")
        .build();
    windows.add(&tiling);
    let (preset_reset, preset_copy) = header(&presets);
    let (tiling_reset, tiling_copy) = header(&windows);
    let ui = Rc::new(Ui {
        checks,
        presets,
        preset_reset,
        tiling,
        tiling_reset,
        quiet: Cell::new(false),
    });
    ui.update();

    for (name, check) in &ui.checks {
        let (name, ui2) = (*name, ui.clone());
        check.connect_toggled(move |check| {
            if check.is_active() && !ui2.quiet.get() {
                report(Files::here().choose_preset(name));
                // A preset brings its own policy unless layout.tiling is set.
                ui2.update();
            }
        });
    }
    let ui2 = ui.clone();
    ui.tiling.connect_active_notify(move |row| {
        if !ui2.quiet.get() {
            report(Files::here().choose_tiling(row.is_active()));
            ui2.update();
        }
    });
    let ui2 = ui.clone();
    ui.preset_reset.connect_clicked(move |_| {
        report(Files::here().set("layout.preset", None));
        ui2.update();
    });
    let ui2 = ui.clone();
    ui.tiling_reset.connect_clicked(move |_| {
        report(Files::here().set("layout.tiling", None));
        ui2.update();
    });
    preset_copy.connect_clicked(|button| {
        let now = Files::here().layout();
        button
            .clipboard()
            .set_text(&rows::command("layout.preset", &now.preset));
    });
    tiling_copy.connect_clicked(|button| {
        let now = Files::here().layout();
        button
            .clipboard()
            .set_text(&rows::command("layout.tiling", &now.tiling.to_string()));
    });

    page.add(&ui.presets);
    page.add(&windows);
    page
}

/// A group's Reset and Copy as command buttons, in its header.
fn header(group: &adw::PreferencesGroup) -> (gtk::Button, gtk::Button) {
    let reset = gtk::Button::builder()
        .label("Reset")
        .tooltip_text("Take your choice out, so this machine or the release decides")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy as command")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    let buttons = gtk::Box::builder().spacing(6).build();
    buttons.append(&reset);
    buttons.append(&copy);
    group.set_header_suffix(Some(&buttons));
    (reset, copy)
}

/// Says why a change was not written; the row keeps what it shows.
fn report(result: Result<(), String>) {
    if let Err(e) = result {
        eprintln!("edel-settings: {e}");
    }
}
