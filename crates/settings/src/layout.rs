//! The Layout page (M5.6a): one row per preset and whether windows tile,
//! each change one line of the person's settings file, which the desktop
//! follows at once.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::files::{self, Files};

pub fn page() -> adw::PreferencesPage {
    let now = Files::here().layout();
    let page = adw::PreferencesPage::builder()
        .title("Layout")
        .icon_name("view-grid-symbolic")
        .build();
    let presets = adw::PreferencesGroup::builder()
        .title("Preset")
        .description("Where the panels sit and how windows are placed")
        .build();
    let tiling = adw::SwitchRow::builder()
        .title("Tile windows")
        .subtitle("Side by side, each with its title bar; Super+T switches one workspace")
        .active(now.tiling)
        .build();
    // Set while the page itself moves the switch, so that is not a choice.
    let quiet = Rc::new(Cell::new(false));
    let mut first: Option<gtk::CheckButton> = None;
    for name in edel::presets::NAMES {
        let check = gtk::CheckButton::new();
        match &first {
            Some(first) => check.set_group(Some(first)),
            None => first = Some(check.clone()),
        }
        check.set_active(*name == now.preset);
        let row = adw::ActionRow::builder()
            .title(files::title(name))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        let (tiling, quiet) = (tiling.clone(), quiet.clone());
        check.connect_toggled(move |check| {
            if !check.is_active() {
                return;
            }
            let files = Files::here();
            report(files.choose_preset(name));
            // A preset brings its own policy unless layout.tiling is set.
            quiet.set(true);
            tiling.set_active(files.layout().tiling);
            quiet.set(false);
        });
        presets.add(&row);
    }
    tiling.connect_active_notify(move |row| {
        if !quiet.get() {
            report(Files::here().choose_tiling(row.is_active()));
        }
    });
    let windows = adw::PreferencesGroup::builder().title("Windows").build();
    windows.add(&tiling);
    page.add(&presets);
    page.add(&windows);
    page
}

/// Says why a change was not written; the row keeps what it shows.
fn report(result: Result<(), String>) {
    if let Err(e) = result {
        eprintln!("edel-settings: {e}");
    }
}
