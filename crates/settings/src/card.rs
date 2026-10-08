//! The building blocks of a page about the state of one thing, such as the
//! Network and Bluetooth pages (M5.8a): a card at the head with a large
//! icon, one plain headline and a line under it, so a person sees at a
//! glance whether they are connected and reads no further unless they
//! want to; signal bars; a fold for what only technical people read; and
//! the timer that keeps a page up to date only while it is on screen. Like
//! `widgets.rs` it holds no look of its own, the look is `style.rs`'s from
//! the tokens, and another toolkit would be these few functions again
//! (ADR-004).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::icon;

/// The card's icon, in logical pixels.
const ICON: i32 = 40;

/// The card at the head of a page.
pub struct Glance {
    icon: gtk::Image,
    title: gtk::Label,
    sub: gtk::Label,
    /// Where a control goes, at the card's end: Bluetooth's switch.
    pub aside: gtk::Box,
}

impl Glance {
    /// Appends a card showing the icon `icon_name` to `content`.
    pub fn new(content: &gtk::Box, icon_name: &str) -> Glance {
        let icon = icon::image(icon_name, ICON);
        icon.add_css_class("edel-glance-icon");
        icon.set_valign(gtk::Align::Center);
        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-glance-title"])
            .build();
        let sub = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-glance-sub"])
            .build();
        let words = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        words.append(&title);
        words.append(&sub);
        let aside = gtk::Box::builder().valign(gtk::Align::Center).build();
        let card = gtk::Box::builder()
            .spacing(18)
            .margin_top(18)
            .css_classes(["edel-glance"])
            .build();
        card.append(&icon);
        card.append(&words);
        card.append(&aside);
        content.append(&card);
        Glance {
            icon,
            title,
            sub,
            aside,
        }
    }

    /// Shows the headline and the line under it; the icon is in the
    /// accent while `on` and muted when it is not. A screen reader hears
    /// the headline and the line as one sentence.
    pub fn show(&self, on: bool, title: &str, sub: &str) {
        self.title.set_label(title);
        self.sub.set_label(sub);
        self.sub.set_visible(!sub.is_empty());
        if on {
            self.icon.remove_css_class("edel-off");
        } else {
            self.icon.add_css_class("edel-off");
        }
        let said = if sub.is_empty() {
            title.to_string()
        } else {
            format!("{title}. {sub}")
        };
        if let Some(card) = self.title.parent().and_then(|words| words.parent()) {
            card.update_property(&[gtk::accessible::Property::Label(&said)]);
        }
    }
}

/// A fold-out in `content` that starts closed, for what only some people
/// want to read; `child` is shown when it opens. The title is its heading.
pub fn fold(content: &gtk::Box, title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Expander {
    let expander = gtk::Expander::builder()
        .label(title)
        .child(child)
        .margin_top(26)
        .css_classes(["edel-fold"])
        .build();
    content.append(&expander);
    expander
}

/// Four bars of rising height, the first `lit` of them in the accent: the
/// signal of a Wi-Fi network. Screen readers hear `word`, not the bars.
pub fn bars(lit: u8, word: &str) -> gtk::Box {
    let bars = gtk::Box::builder()
        .spacing(2)
        .valign(gtk::Align::Center)
        .css_classes(["edel-bars"])
        .build();
    for at in 0..4u8 {
        let bar = gtk::Box::builder()
            .valign(gtk::Align::End)
            .height_request(5 + i32::from(at) * 3)
            .css_classes(["edel-bar-part"])
            .build();
        if at < lit {
            bar.add_css_class("edel-lit");
        }
        bars.append(&bar);
    }
    bars.update_property(&[gtk::accessible::Property::Label(word)]);
    bars
}

/// Calls `tick` when `page` comes on screen and then every `seconds`
/// while it stays, and never while it is hidden or closed: a page that
/// shows live state looks at it only for as long as someone sees it
/// (no service runs for what nobody uses).
pub fn while_shown(page: &gtk::Widget, seconds: u32, tick: impl Fn() + 'static) {
    let tick = Rc::new(tick);
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    {
        let (tick, timer) = (tick.clone(), timer.clone());
        page.connect_map(move |_| {
            tick();
            let tick = tick.clone();
            let id = glib::timeout_add_seconds_local(seconds, move || {
                tick();
                glib::ControlFlow::Continue
            });
            if let Some(old) = timer.borrow_mut().replace(id) {
                old.remove();
            }
        });
    }
    page.connect_unmap(move |_| {
        if let Some(id) = timer.borrow_mut().take() {
            id.remove();
        }
    });
}

/// Removes every child of `holder`.
pub fn clear(holder: &gtk::Box) {
    while let Some(child) = holder.first_child() {
        holder.remove(&child);
    }
}
