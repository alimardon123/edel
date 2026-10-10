//! `edel-settings` (roadmap M5.6): Settings, the app for every setting
//! (ADR-008). Its pages are `edel::settings`'s page table, the same names
//! `edel settings` prints; each change is one line of the person's
//! settings file, written by the function `edel settings set` uses, which
//! the desktop follows at once. A sidebar with a search field lists the
//! pages; on a narrow window it folds away and the pages open one at a
//! time (ADR-004's size classes, proven on our own app first). The window
//! has no header bar of its own: the compositor draws its title bar, as
//! for every window, with the buttons where the person put them.
//! Everything people see is drawn from the design tokens and our own
//! icons (`style.rs`, `icon.rs`), in our own look, not GNOME's.

mod about;
mod bluetooth;
mod card;
mod cmd;
mod display;
mod files;
mod icon;
mod layout;
mod network;
mod notifications;
mod panels;
mod power;
mod preview;
mod rows;
mod screens;
mod sound;
mod status;
mod style;
mod system;
mod tray;
mod updates;
mod users;
mod widgets;
mod workspaces;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use edel::i18n::{tr, trf};

use crate::style::Theme;

/// The app's id, which its desktop file is named after
/// (`features/settings/usr/share/applications/APP_ID.desktop`; a test
/// holds them together).
pub const APP_ID: &str = "io.github.alimardon123.edel.Settings";

/// Below this width the sidebar folds away.
const NARROW: &str = "max-width: 600sp";

/// The sidebar's width, in logical pixels, as the mockups draw it.
const SIDEBAR: f64 = 204.0;

/// The renderer GTK draws Settings with unless `GSK_RENDERER` says
/// otherwise: Cairo, on the processor. Measured on 2026-10-07 in CI's
/// VM, Settings held 18.6 MiB of its own with it and 107.4 MiB with GL
/// under llvmpipe, and a page of rows that changes on a click needs no GPU to keep
/// up (Efficient over a speed nobody sees; the default row in the roadmap).
const RENDERER: &str = "cairo";

fn main() -> gtk::glib::ExitCode {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: the first thing the program does, before GTK or any
        // other thread starts, so nothing reads the environment meanwhile.
        unsafe { std::env::set_var("GSK_RENDERER", RENDERER) };
    }
    // Without a session bus GTK names the window after the program, so
    // the program takes the app's id: the compositor and the panel find
    // its icon and name either way.
    gtk::glib::set_prgname(Some(APP_ID));
    if let Some((language, words)) = edel::i18n::init("settings") {
        eprintln!("edel-settings: words in {language}, {words} translated");
    }
    // `--page NAME` opens that page at the start, as a link to one page
    // would: the Displays page from a screen's own menu, CI's screendumps.
    let start = match start_page(std::env::args().skip(1), &pages()) {
        Ok(at) => at,
        Err(message) => {
            eprintln!("edel-settings: {message}");
            return gtk::glib::ExitCode::from(2);
        }
    };
    let app = adw::Application::builder().application_id(APP_ID).build();
    // Any argument is --page (others were refused above), and a page
    // opened by name gives the keyboard to its main control; a row named
    // (`--page layout.panels`) gives it to that row's control (M5.31d).
    let asked = std::env::args().len() > 1;
    let row = std::env::args()
        .skip(1)
        .find_map(|arg| rows::row_named(arg.rsplit_once('=').map_or(&arg, |(_, v)| v)));
    widgets::set_asked_row(row);
    app.connect_activate(move |app| window(app, start, asked));
    // GTK would refuse `--page`, which is read above.
    app.run_with_args(&[APP_ID])
}

/// The page `--page NAME` asks for, as its place in [`pages`], or the first
/// without it; an argument it does not know, or a page this machine
/// lacks, is refused with what to do.
fn start_page(args: impl IntoIterator<Item = String>, pages: &[Page]) -> Result<usize, String> {
    let mut args = args.into_iter();
    let mut name: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.strip_prefix("--page") {
            Some("") => {
                name = Some(args.next().ok_or_else(|| {
                    tr("--page needs the name of a page, such as --page displays").to_string()
                })?);
            }
            Some(rest) if rest.starts_with('=') => name = Some(rest[1..].to_string()),
            _ => {
                return Err(trf(
                    "unknown option \"{option}\"; the only one is --page NAME",
                    &[("option", &arg)],
                ));
            }
        }
    }
    match name {
        None => Ok(0),
        Some(name) => page_named(pages, &name).ok_or_else(|| {
            let titles: Vec<String> = pages.iter().map(|p| p.title.to_lowercase()).collect();
            trf(
                "there is no page \"{name}\" on this machine; the pages are {pages}",
                &[("name", &name), ("pages", &titles.join(", "))],
            )
        }),
    }
}

/// The page `name` means: its section or its title, whatever the case, or
/// the start of exactly one of them (`display` is Displays).
fn page_named(pages: &[Page], name: &str) -> Option<usize> {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    let names = |page: &Page| -> [String; 2] {
        [
            page.section.unwrap_or_default().to_lowercase(),
            page.title.to_lowercase(),
        ]
    };
    // A row's key or title names the page it is on (M5.31d).
    let exact = pages
        .iter()
        .position(|p| names(p).contains(&name))
        .or_else(|| {
            let (section, _) = rows::row_named(&name)?.split_once('.')?;
            pages.iter().position(|p| p.section == Some(section))
        });
    let mut starts = pages.iter().enumerate().filter(|(_, p)| {
        names(p)
            .iter()
            .any(|n| !name.is_empty() && n.starts_with(&name))
    });
    exact.or_else(|| {
        let first = starts.next()?;
        starts.next().is_none().then_some(first.0)
    })
}

/// One page of the app: its name and icon in the sidebar, its section
/// of the settings file, what it shows, and the feature it talks to,
/// without which it is hidden (M5.6b).
struct Page {
    title: &'static str,
    icon: &'static str,
    section: Option<&'static str>,
    build: fn(&Rc<Theme>) -> gtk::Widget,
    needs: Option<&'static str>,
}

/// The pages this release has, in `edel::settings::PAGES`' order, then
/// About; a page is added here as its step lands (M5.7 to M5.10; Displays, M5.7a).
fn all_pages() -> Vec<Page> {
    let mut pages: Vec<Page> = edel::settings::PAGES
        .iter()
        .filter_map(|page| match page.section {
            // The layout is the desktop's: no desktop, no Layout page.
            "layout" => Some(Page {
                title: tr(page.title),
                icon: "page-layout",
                section: Some(page.section),
                build: layout::page,
                needs: Some("shell"),
            }),
            // The screens are the compositor's too.
            "displays" => Some(Page {
                title: tr(page.title),
                icon: "page-displays",
                section: Some(page.section),
                build: display::page,
                needs: Some("shell"),
            }),
            // Sound talks to PipeWire through wpctl; no sound feature, no page.
            "sound" => Some(Page {
                title: tr(page.title),
                icon: "page-sound",
                section: Some(page.section),
                build: |_| sound::page(),
                needs: Some("sound"),
            }),
            // Users reads the accounts every machine has (M5.8b).
            "users" => Some(Page {
                title: tr(page.title),
                icon: "page-users",
                section: Some(page.section),
                build: |_| users::page(),
                needs: None,
            }),
            // Network talks to NetworkManager through nmcli, Bluetooth to
            // BlueZ through bluetoothctl; no such feature, no page (M5.8a).
            "network" => Some(Page {
                title: tr(page.title),
                icon: "page-network",
                section: Some(page.section),
                build: |_| network::page(),
                needs: Some("network"),
            }),
            "bluetooth" => Some(Page {
                title: tr(page.title),
                icon: "page-bluetooth",
                section: Some(page.section),
                build: |_| bluetooth::page(),
                needs: Some("bluetooth"),
            }),
            // Power talks to UPower through upower (M5.8b).
            "power" => Some(Page {
                title: tr(page.title),
                icon: "page-power",
                section: Some(page.section),
                build: |_| power::page(),
                needs: Some("power"),
            }),
            // Notifications are shell-ui's, so the desktop's (M5.9b).
            "notifications" => Some(Page {
                title: tr(page.title),
                icon: "bell",
                section: Some(page.section),
                build: |_| notifications::page(),
                needs: Some("shell"),
            }),
            // Updates and System run `edel`, which every image has.
            "updates" => Some(Page {
                title: tr(page.title),
                icon: "page-updates",
                section: Some(page.section),
                build: |_| updates::page(),
                needs: None,
            }),
            "system" => Some(Page {
                title: tr(page.title),
                icon: "page-system",
                section: Some(page.section),
                build: |_| system::page(),
                needs: None,
            }),
            _ => None,
        })
        .collect();
    pages.push(Page {
        title: tr("About"),
        icon: "page-about",
        section: None,
        build: |_| about::page(),
        needs: None,
    });
    pages
}

/// The pages this machine shows: those whose feature it has, as the
/// feature files in `features` say (`edel::features::DIR` on a machine).
fn pages_in(features: &std::path::Path) -> Vec<Page> {
    all_pages()
        .into_iter()
        .filter(|page| {
            page.needs
                .is_none_or(|f| features.join(format!("{f}.toml")).exists())
        })
        .collect()
}

fn pages() -> Vec<Page> {
    pages_in(&edel::places::found_shared(edel::features::DIR))
}

/// Whether search text `query` finds `page`: its title or one of its rows'
/// titles holds it, whatever the case.
fn finds(page: &Page, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    let holds = |text: &str| text.to_lowercase().contains(&query);
    query.is_empty()
        || holds(page.title)
        || page
            .section
            .is_some_and(|s| rows::on_page(s).any(|row| holds(tr(row.title))))
}

fn window(app: &adw::Application, start: usize, asked: bool) {
    let theme = Theme::new();
    let pages = Rc::new(pages());
    // Each page is built when first shown and kept.
    let built: Rc<RefCell<Vec<Option<gtk::Widget>>>> =
        Rc::new(RefCell::new(vec![None; pages.len()]));

    let split = adw::NavigationSplitView::builder()
        .min_sidebar_width(SIDEBAR)
        .max_sidebar_width(SIDEBAR)
        .build();
    let content = adw::NavigationPage::builder()
        .title(pages[start.min(pages.len() - 1)].title)
        .build();
    let back = gtk::Button::builder()
        .halign(gtk::Align::Start)
        .css_classes(["edel-back"])
        .visible(false)
        .build();
    let back_label = gtk::Box::builder().spacing(4).build();
    back_label.append(&icon::image("back", 14));
    back_label.append(&gtk::Label::new(Some(tr("Settings"))));
    back.set_child(Some(&back_label));
    back.update_property(&[gtk::accessible::Property::Label(tr("Back to the pages"))]);
    split
        .bind_property("collapsed", &back, "visible")
        .sync_create()
        .build();
    {
        let split = split.clone();
        back.connect_clicked(move |_| split.set_show_content(false));
    }
    let shown = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    shown.append(&back);
    shown.append(&holder);
    content.set_child(Some(&shown));
    let show: Rc<dyn Fn(usize)> = {
        let (pages, built, theme, content) =
            (pages.clone(), built.clone(), theme.clone(), content.clone());
        Rc::new(move |at: usize| {
            let Some(page) = pages.get(at) else { return };
            let widget = built.borrow_mut()[at]
                .get_or_insert_with(|| (page.build)(&theme))
                .clone();
            while let Some(child) = holder.first_child() {
                holder.remove(&child);
            }
            holder.append(&widget);
            content.set_title(page.title);
        })
    };
    show(start);
    if asked {
        if let Some(page) = built.borrow()[start].as_ref() {
            page.add_css_class(widgets::ASKED);
        }
    }

    let list = gtk::ListBox::builder()
        .css_classes(["edel-pages"])
        .vexpand(true)
        .build();
    for page in pages.iter() {
        let row = gtk::Box::builder().spacing(10).build();
        row.append(&icon::image(page.icon, 16));
        row.append(&gtk::Label::builder().label(page.title).xalign(0.0).build());
        list.append(&row);
    }
    {
        let (split, show) = (split.clone(), show.clone());
        list.connect_row_activated(move |_, row| {
            show(row.index().max(0) as usize);
            split.set_show_content(true);
        });
    }
    list.select_row(list.row_at_index(start as i32).as_ref());

    let search = gtk::Entry::builder()
        .placeholder_text(tr("Search"))
        .primary_icon_paintable(&icon::Icon::new("search", 14))
        .css_classes(["edel-search"])
        .build();
    search.update_property(&[gtk::accessible::Property::Label(tr("Search the settings"))]);
    {
        let (pages, search2) = (pages.clone(), search.clone());
        list.set_filter_func(move |row| {
            pages
                .get(row.index().max(0) as usize)
                .is_some_and(|page| finds(page, &search2.text()))
        });
    }
    {
        let list = list.clone();
        search.connect_changed(move |_| list.invalidate_filter());
    }
    {
        // Return opens the first page search found.
        let list = list.clone();
        search.connect_activate(move |_| {
            let mut at = 0;
            while let Some(row) = list.row_at_index(at) {
                if row.is_child_visible() {
                    list.select_row(Some(&row));
                    row.activate();
                    break;
                }
                at += 1;
            }
        });
    }

    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["edel-sidebar"])
        .build();
    sidebar_box.append(&search);
    sidebar_box.append(&list);
    let sidebar = adw::NavigationPage::builder()
        .title(tr("Settings"))
        .child(&sidebar_box)
        .build();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));
    // Asked for a page, a narrow window opens on it, not on the list.
    split.set_show_content(start > 0);

    let bin = adw::BreakpointBin::builder()
        .width_request(360)
        .height_request(300)
        .child(&split)
        .build();
    if let Ok(condition) = adw::BreakpointCondition::parse(NARROW) {
        let narrow = adw::Breakpoint::new(condition);
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        bin.add_breakpoint(narrow);
    }
    // A plain GTK window, without libadwaita's own bar, so the compositor
    // draws ours (KDE's server decoration protocol, M5.6a).
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(tr("Settings"))
        .default_width(960)
        .default_height(640)
        .css_classes(["edel"])
        .child(&bin)
        .build();
    // `win.show-page` opens a page by name, as About's Check for updates
    // button does: the sidebar follows, and a narrow window shows the page.
    let go = gio::SimpleAction::new("show-page", Some(gtk::glib::VariantTy::STRING));
    {
        let (pages, list, split) = (pages.clone(), list.clone(), split.clone());
        go.connect_activate(move |_, name| {
            let Some(at) = name
                .and_then(|n| n.str())
                .and_then(|n| page_named(&pages, n))
            else {
                return;
            };
            show(at);
            list.select_row(list.row_at_index(at as i32).as_ref());
            split.set_show_content(true);
        });
    }
    window.add_action(&go);
    // Ctrl+F goes to the search field, as in every app with one.
    let keys = gtk::ShortcutController::new();
    let find = search.clone();
    keys.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>f"),
        Some(gtk::CallbackAction::new(move |_, _| {
            find.grab_focus();
            gtk::glib::Propagation::Stop
        })),
    ));
    window.add_controller(keys);
    window.present();
    // The pages' list takes the keyboard first, so the search field opens
    // quiet; it is a click away, or Ctrl+F.
    if let Some(row) = list.row_at_index(start as i32) {
        row.grab_focus();
    }
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
        let titles: Vec<&str> = all_pages().iter().map(|p| p.title).collect();
        assert_eq!(
            titles,
            [
                "Layout",
                "Displays",
                "Sound",
                "Network",
                "Bluetooth",
                "Power",
                "Notifications",
                "Users",
                "Updates",
                "System",
                "About"
            ]
        );
    }

    #[test]
    fn every_page_has_one_of_our_icons() {
        for page in all_pages() {
            assert!(
                edel::icons::mask(page.icon, 16).is_some(),
                "the {} page's icon {} is not in design/icons",
                page.title,
                page.icon
            );
        }
    }

    #[test]
    fn search_finds_a_page_by_its_title_or_its_rows() {
        let pages = all_pages();
        let titles = |query: &str| -> Vec<&str> {
            pages
                .iter()
                .filter(|p| finds(p, query))
                .map(|p| p.title)
                .collect()
        };
        assert_eq!(
            titles(""),
            [
                "Layout",
                "Displays",
                "Sound",
                "Network",
                "Bluetooth",
                "Power",
                "Notifications",
                "Users",
                "Updates",
                "System",
                "About"
            ]
        );
        assert_eq!(titles("abo"), ["About"]);
        assert_eq!(titles("sou"), ["Sound"]);
        assert_eq!(titles("blue"), ["Bluetooth"]);
        assert_eq!(titles("net"), ["Network"]);
        assert_eq!(titles("pow"), ["Power"]);
        assert_eq!(
            titles("disturb"),
            ["Notifications"],
            "the Do not disturb row"
        );
        assert_eq!(titles("notif"), ["Notifications"]);
        assert_eq!(titles("user"), ["Users"]);
        assert_eq!(titles("chan"), ["Updates"], "the Channel row");
        assert_eq!(titles("TITLE BARS"), ["Layout"]);
        assert_eq!(titles("resolution"), ["Displays"]);
        assert_eq!(titles("scale"), ["Displays"]);
        assert!(titles("nothing like this").is_empty());
    }

    #[test]
    fn a_page_is_asked_for_by_name() {
        let pages = all_pages();
        let at = |name: &str| page_named(&pages, name);
        assert_eq!(at("layout"), Some(0));
        assert_eq!(at("displays"), Some(1));
        assert_eq!(at("Displays"), Some(1));
        assert_eq!(at("display"), Some(1), "the start of one name");
        assert_eq!(at("sound"), Some(2));
        assert_eq!(at("Sound"), Some(2));
        assert_eq!(at("users"), Some(7));
        assert_eq!(at("network"), Some(3));
        assert_eq!(at("bluetooth"), Some(4));
        assert_eq!(at("blue"), Some(4));
        assert_eq!(at("power"), Some(5));
        assert_eq!(at("notifications"), Some(6));
        assert_eq!(at("notif"), Some(6));
        assert_eq!(at("updates"), Some(8));
        assert_eq!(at("system"), Some(9));
        assert_eq!(at("sy"), Some(9));
        assert_eq!(at("about"), Some(10));
        assert_eq!(at("abo"), Some(10));
        assert_eq!(at(""), None);
        assert_eq!(at("l"), Some(0), "one start only");
    }

    #[test]
    fn a_row_names_its_page_and_asks_for_its_control() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let pages = all_pages();
        assert_eq!(page_named(&pages, "layout.panels"), Some(0));
        assert_eq!(page_named(&pages, "Panels"), Some(0));
        assert_eq!(page_named(&pages, "scale"), Some(1), "a row of Displays");
        assert_eq!(start_page(args(&["--page", "panels"]), &pages), Ok(0));
        assert_eq!(start_page(args(&["--page=layout.panels"]), &pages), Ok(0));
        assert_eq!(rows::row_named("layout.panels"), Some("layout.panels"));
        assert_eq!(rows::row_named("--page"), None);
    }

    #[test]
    fn the_page_option_is_read_or_refused_with_what_to_do() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let pages = all_pages();
        let start = |list: &[&str]| start_page(args(list), &pages);
        assert_eq!(start(&[]), Ok(0));
        assert_eq!(start(&["--page", "about"]), Ok(10));
        assert_eq!(start(&["--page=about"]), Ok(10));
        assert_eq!(start(&["--page", "updates"]), Ok(8));
        assert_eq!(start(&["--page", "sound"]), Ok(2));
        assert_eq!(start(&["--page", "users"]), Ok(7));
        assert_eq!(start(&["--page", "notifications"]), Ok(6));
        assert_eq!(start(&["--page", "network"]), Ok(3));
        assert_eq!(start(&["--page", "bluetooth"]), Ok(4));
        assert_eq!(start(&["--page", "power"]), Ok(5));
        let refused = start(&["--page"]).unwrap_err();
        assert!(
            refused.contains("--page needs the name of a page"),
            "{refused}"
        );
        let refused = start(&["--pge", "x"]).unwrap_err();
        assert_eq!(
            refused,
            "unknown option \"--pge\"; the only one is --page NAME"
        );
        let refused = start(&["--page", "zzz"]).unwrap_err();
        assert!(
            refused.starts_with("there is no page \"zzz\" on this machine; the pages are "),
            "{refused}"
        );
    }

    #[test]
    fn a_page_whose_feature_is_missing_is_hidden() {
        let dir = std::env::temp_dir().join(format!("edel-settings-pages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let titles = |dir: &std::path::Path| -> Vec<&str> {
            pages_in(dir).iter().map(|p| p.title).collect()
        };
        // Users, Updates and System need no feature: every machine has
        // accounts and `edel`.
        assert_eq!(titles(&dir), ["Users", "Updates", "System", "About"]);
        std::fs::write(dir.join("shell.toml"), "format = 1\n").unwrap();
        assert_eq!(
            titles(&dir),
            [
                "Layout",
                "Displays",
                "Notifications",
                "Users",
                "Updates",
                "System",
                "About"
            ]
        );
        std::fs::write(dir.join("sound.toml"), "format = 1\n").unwrap();
        assert_eq!(
            titles(&dir),
            [
                "Layout",
                "Displays",
                "Sound",
                "Notifications",
                "Users",
                "Updates",
                "System",
                "About"
            ]
        );
        // Network, Bluetooth and Power each need a feature of their own.
        std::fs::write(dir.join("network.toml"), "format = 1\n").unwrap();
        assert!(titles(&dir).contains(&"Network"));
        std::fs::write(dir.join("bluetooth.toml"), "format = 1\n").unwrap();
        assert!(titles(&dir).contains(&"Bluetooth"));
        assert!(!titles(&dir).contains(&"Power"));
        std::fs::write(dir.join("power.toml"), "format = 1\n").unwrap();
        assert!(titles(&dir).contains(&"Power"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_feature_a_page_needs_has_a_file_under_features() {
        let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        for page in all_pages() {
            if let Some(feature) = page.needs {
                assert!(
                    features.join(format!("{feature}.toml")).exists(),
                    "the {} page needs a feature {feature} that has no file",
                    page.title
                );
            }
        }
    }
}
