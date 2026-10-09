//! Edel OS's Settings app (M5.6a): one window, a sidebar of pages beside
//! the page shown, collapsing into one pane at a time when the window is
//! narrower than 600 sp (the Compact size class, ADR-004). Each row reads
//! and writes the system file through `edel::system`, the same code the
//! `edel system set` command runs, so a change made here is one line of
//! the person's `~/.config/edel/system.toml` and takes effect as it does
//! from the command line.

mod about;
mod layout;
mod system;

use adw::prelude::*;
use gtk::glib;

/// The app's id: its desktop file's name and its D-Bus name.
const APP_ID: &str = "os.edel.Settings";

/// The pages, in the sidebar's order: a name, a title and the page.
fn pages() -> Vec<(&'static str, &'static str, gtk::Widget)> {
    vec![
        ("layout", "Layout", layout::page().upcast()),
        ("about", "About", about::page().upcast()),
    ]
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(window);
    app.run()
}

fn window(app: &adw::Application) {
    let stack = gtk::Stack::new();
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    let mut titles = Vec::new();
    for (name, title, page) in pages() {
        stack.add_titled(&page, Some(name), title);
        let label = gtk::Label::builder().label(title).xalign(0.0).build();
        list.append(&label);
        titles.push((name, title));
    }
    let sidebar = adw::ToolbarView::new();
    sidebar.add_top_bar(&adw::HeaderBar::new());
    sidebar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build(),
    ));
    let content = adw::ToolbarView::new();
    content.add_top_bar(&adw::HeaderBar::new());
    content.set_content(Some(&stack));
    let (_, first) = titles[0];
    let content_page = adw::NavigationPage::new(&content, first);
    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&adw::NavigationPage::new(&sidebar, "Settings")));
    split.set_content(Some(&content_page));
    {
        let (split, content_page) = (split.clone(), content_page.clone());
        list.connect_row_activated(move |_, row| {
            if let Some((name, title)) = usize::try_from(row.index())
                .ok()
                .and_then(|i| titles.get(i))
            {
                stack.set_visible_child_name(name);
                content_page.set_title(title);
                split.set_show_content(true);
            }
        });
    }
    list.select_row(list.row_at_index(0).as_ref());
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Settings")
        .default_width(960)
        .default_height(640)
        .width_request(360)
        .height_request(320)
        .content(&split)
        .build();
    // Compact (ADR-004): one pane at a time, the sidebar first.
    if let Ok(condition) = adw::BreakpointCondition::parse("max-width: 600sp") {
        let breakpoint = adw::Breakpoint::new(condition);
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(breakpoint);
    }
    window.present();
}
