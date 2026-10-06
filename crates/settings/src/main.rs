//! `edel-settings` (roadmap M5.6a): Settings, the app for every setting
//! (ADR-008). Its pages are `edel::system`'s page table, the same names
//! `edel settings` prints; each change is one line of the person's
//! settings file, written by the function `edel settings set` uses, which
//! the desktop follows at once. A sidebar lists the pages; on a narrow
//! window it folds away and the pages open one at a time (ADR-004's size
//! classes, proven on our own app first).

mod about;
mod files;
mod layout;

use adw::prelude::*;

/// The app's id, which its desktop file is named after
/// (`features/settings/usr/share/applications/APP_ID.desktop`; a test
/// holds them together).
pub const APP_ID: &str = "io.github.alimardon123.edel.Settings";

/// Below this width the sidebar folds away.
const NARROW: &str = "max-width: 600sp";

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(window);
    app.run()
}

/// One page of the app: its name in the sidebar and what it shows.
struct Page {
    title: &'static str,
    build: fn() -> adw::PreferencesPage,
}

/// The pages this release shows, in `edel::system::PAGES`' order, then
/// About; a page is added here as its step lands (M5.7 to M5.10).
fn pages() -> Vec<Page> {
    let mut pages: Vec<Page> = edel::system::PAGES
        .iter()
        .filter_map(|page| match page.section {
            "layout" => Some(Page {
                title: page.title,
                build: layout::page,
            }),
            _ => None,
        })
        .collect();
    pages.push(Page {
        title: "About",
        build: about::page,
    });
    pages
}

fn window(app: &adw::Application) {
    let pages = pages();
    let content = adw::NavigationPage::builder().title(pages[0].title).build();
    let show = {
        let content = content.clone();
        move |page: &Page| {
            let built = (page.build)();
            let view = adw::ToolbarView::new();
            view.add_top_bar(&adw::HeaderBar::new());
            view.set_content(Some(&built));
            content.set_title(page.title);
            content.set_child(Some(&view));
        }
    };
    show(&pages[0]);

    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    for page in &pages {
        let label = gtk::Label::builder()
            .label(page.title)
            .xalign(0.0)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(6)
            .build();
        list.append(&label);
    }
    let split = adw::NavigationSplitView::new();
    {
        let split = split.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(page) = pages.get(row.index() as usize) {
                show(page);
            }
            split.set_show_content(true);
        });
    }
    list.select_row(list.row_at_index(0).as_ref());
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&adw::HeaderBar::new());
    sidebar_view.set_content(Some(&list));
    let sidebar = adw::NavigationPage::builder()
        .title("Settings")
        .child(&sidebar_view)
        .build();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Settings")
        .default_width(960)
        .default_height(640)
        .width_request(360)
        .height_request(300)
        .content(&split)
        .build();
    if let Ok(condition) = adw::BreakpointCondition::parse(NARROW) {
        let narrow = adw::Breakpoint::new(condition);
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(narrow);
    }
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_file_is_named_after_the_app_id() {
        let path = format!(
            "{}/../../features/settings/usr/share/applications/{APP_ID}.desktop",
            env!("CARGO_MANIFEST_DIR")
        );
        let text =
            std::fs::read_to_string(&path).expect("the settings feature ships the desktop file");
        assert!(text.contains("Exec=edel-settings"), "{text}");
    }

    #[test]
    fn the_pages_follow_the_page_table_then_about() {
        let titles: Vec<&str> = pages().iter().map(|p| p.title).collect();
        assert_eq!(titles, ["Layout", "About"]);
    }
}
