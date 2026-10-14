//! edel-testheader (CI only, roadmap M6.16): one GTK 4 window titled
//! headerbar with a header bar of its own, as GTK and libadwaita apps draw
//! theirs. It prints one line each time its header bar is laid out anew,
//! for desktop-test's own-bars case: `headerbar layout LAYOUT close X+Y+WxH
//! icon PATH`, the decoration layout GTK took from the settings portal, the
//! close button's place in logical pixels from the window's corner, and the
//! file GTK draws the close glyph from. Only CI's images ship it.

use std::cell::RefCell;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.edel.TestHeader")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(window);
    app.run_with_args::<&str>(&[])
}

/// The window: a header bar of its own, one label, and a report of the
/// header bar's layout each time GTK takes a new decoration layout from the
/// settings portal.
fn window(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("headerbar")
        .default_width(480)
        .default_height(200)
        .build();
    window.set_titlebar(Some(&gtk::HeaderBar::new()));
    window.set_child(Some(&gtk::Label::new(Some("header bar"))));
    window.present();
    report(&window);
    let weak = window.downgrade();
    if let Some(settings) = gtk::Settings::default() {
        settings.connect_gtk_decoration_layout_notify(move |_| {
            if let Some(window) = weak.upgrade() {
                report(&window);
            }
        });
    }
}

/// Prints the header bar's line once two frames in a row compute the same
/// text, so GTK's laying out has finished. Waits while the window is not
/// mapped.
fn report(window: &gtk::ApplicationWindow) {
    let last = RefCell::new(String::new());
    window.add_tick_callback(move |window, _| {
        if !window.is_mapped() {
            return gtk::glib::ControlFlow::Continue;
        }
        let text = line(window);
        if last.replace(text.clone()) != text {
            return gtk::glib::ControlFlow::Continue;
        }
        println!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        gtk::glib::ControlFlow::Break
    });
}

/// The line CI reads: the decoration layout, the close button's place and
/// the file of the close glyph, each `none` when there is none.
fn line(window: &gtk::ApplicationWindow) -> String {
    let layout = gtk::Settings::default()
        .and_then(|settings| settings.gtk_decoration_layout())
        .map_or_else(|| "none".to_string(), |layout| layout.to_string());
    let close = find_close(window.upcast_ref())
        .and_then(|button| button.compute_bounds(window))
        .map_or_else(
            || "none".to_string(),
            |r| {
                format!(
                    "{}+{}+{}x{}",
                    r.x().round() as i32,
                    r.y().round() as i32,
                    r.width().round() as i32,
                    r.height().round() as i32
                )
            },
        );
    let display = gtk::prelude::WidgetExt::display(window);
    let icon = gtk::IconTheme::for_display(&display)
        .lookup_icon(
            "window-close-symbolic",
            &[],
            16,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        )
        .file()
        .and_then(|file| file.path())
        .map_or_else(|| "none".to_string(), |path| path.display().to_string());
    format!("headerbar layout {layout} close {close} icon {icon}")
}

/// The first widget under `widget`, depth first, that has the CSS class
/// `close`: the header bar's close button.
fn find_close(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.has_css_class("close") {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_close(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
