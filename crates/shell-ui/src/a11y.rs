//! What screen readers read (M5.1d): each panel is a window in an AccessKit
//! tree whose children are its widgets, each with the role and label its
//! line in the widget table gives and its place in the panel; so is the
//! quick settings card while it is open (M5.9a), its parts with their
//! state and the one holding the keyboard focused. The Unix adapter serves
//! the tree over AT-SPI on the session's D-Bus, and only once a reader has
//! turned accessibility on (`org.a11y.Status`); until then shell-ui keeps
//! what the panel shows and builds no tree.

use edel::i18n::tr;
use std::sync::{Arc, Mutex};

use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Rect, Role,
    TreeId, TreeInfo, TreeUpdate,
};
use accesskit_unix::Adapter;

/// One node of a panel's tree: a widget, what a reader calls it and where
/// it is in the panel, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub role: Role,
    pub label: String,
    pub bounds: Rect,
    /// The parts of it read one by one, such as the tray's icons (M5.2e).
    pub children: Vec<Item>,
    /// Whether it is switched on, for a switch (M5.9a).
    pub toggled: Option<bool>,
    /// Where a slider stands, its least and its greatest (M5.9a).
    pub value: Option<(f64, f64, f64)>,
}

/// The panel itself, the tree's root.
const ROOT: NodeId = NodeId(0);

/// The whole tree of a panel `size` big (logical pixels) holding `items`.
#[cfg(test)]
pub fn tree(size: (f64, f64), items: &[Item]) -> TreeUpdate {
    named(tr("Panel"), size, items, None)
}

/// The tree of a window called `label`, `size` big, holding `items`, the
/// `focus`th of them holding the keyboard when one does: the panel's, and
/// the quick settings card's.
pub fn named(label: &str, size: (f64, f64), items: &[Item], focus: Option<usize>) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label(label);
    root.set_bounds(Rect::new(0.0, 0.0, size.0, size.1));
    let mut nodes = Vec::new();
    let mut focused = ROOT;
    for (i, item) in items.iter().enumerate() {
        let id = add(&mut nodes, item);
        if focus == Some(i) {
            focused = id;
        }
        root.push_child(id);
    }
    nodes.insert(0, (ROOT, root));
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo {
            root: ROOT,
            toolkit_name: Some("edel-shell-ui".into()),
            toolkit_version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        tree_id: TreeId::ROOT,
        // A panel takes no keyboard focus; a card does.
        focus: focused,
    }
}

/// Adds `item` and its parts to `nodes`, numbered in order from one, and
/// gives the item's id.
fn add(nodes: &mut Vec<(NodeId, Node)>, item: &Item) -> NodeId {
    let id = NodeId(nodes.len() as u64 + 1);
    // Held in its place first, so the numbers follow the order.
    nodes.push((id, Node::new(item.role)));
    let mut node = Node::new(item.role);
    // A label's text is its value; anything else is named by its label.
    if item.role == Role::Label {
        node.set_value(item.label.as_str());
    } else {
        node.set_label(item.label.as_str());
    }
    node.set_bounds(item.bounds);
    if let Some(on) = item.toggled {
        node.set_toggled(accesskit::Toggled::from(on));
    }
    if let Some((now, least, most)) = item.value {
        node.set_numeric_value(now);
        node.set_min_numeric_value(least);
        node.set_max_numeric_value(most);
    }
    for part in &item.children {
        let child = add(nodes, part);
        node.push_child(child);
    }
    nodes[id.0 as usize - 1].1 = node;
    id
}

/// What a window shows a reader: its size, its parts and which holds the
/// keyboard.
type Shown = ((f64, f64), Vec<Item>, Option<usize>);

/// A window's link to screen readers: a panel's, or the card's.
pub struct Reader {
    adapter: Adapter,
    latest: Arc<Mutex<Option<Shown>>>,
    label: &'static str,
}

impl Reader {
    /// A panel's reader.
    pub fn panel() -> Reader {
        Reader::new(tr("Panel"))
    }

    /// A reader for a window called `label`.
    pub fn new(label: &'static str) -> Reader {
        let latest = Arc::new(Mutex::new(None));
        let adapter = Adapter::new(Latest(Arc::clone(&latest), label), Nothing, Nothing);
        Reader {
            adapter,
            latest,
            label,
        }
    }

    /// The panel, `size` big, now shows `items`; a reader listening hears
    /// of the change, and the tree is built only for one.
    pub fn update(&mut self, size: (f64, f64), items: Vec<Item>) {
        self.update_focused(size, items, None);
    }

    /// The same, with the `focus`th item holding the keyboard.
    pub fn update_focused(&mut self, size: (f64, f64), items: Vec<Item>, focus: Option<usize>) {
        // Kept first, so a reader turning accessibility on meanwhile gets
        // this, not the one before.
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some((size, items.clone(), focus));
        }
        let label = self.label;
        self.adapter
            .update_if_active(|| named(label, size, &items, focus));
    }
}

/// Gives a reader that turns accessibility on the window's latest tree.
struct Latest(Arc<Mutex<Option<Shown>>>, &'static str);

impl ActivationHandler for Latest {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let latest = self.0.lock().ok()?;
        let (size, items, focus) = latest.as_ref()?;
        Some(named(self.1, *size, items, *focus))
    }
}

/// The panel's widgets take no actions from a reader yet; the keyboard
/// reaches the launcher (Super) and the layout (Super+T).
struct Nothing;

impl ActionHandler for Nothing {
    fn do_action(&mut self, _: ActionRequest) {}
}

impl DeactivationHandler for Nothing {
    fn deactivate_accessibility(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_is_a_window_holding_its_widgets_in_order() {
        let items = [
            Item {
                role: Role::Button,
                label: "Menu".into(),
                bounds: Rect::new(0.0, 12.0, 40.0, 52.0),
                children: vec![],
                toggled: None,
                value: None,
            },
            Item {
                role: Role::Label,
                label: "14:05".into(),
                bounds: Rect::new(1216.0, 12.0, 1280.0, 52.0),
                children: vec![],
                toggled: None,
                value: None,
            },
        ];
        let update = tree((1280.0, 52.0), &items);
        assert_eq!(update.tree.as_ref().unwrap().root, ROOT);
        assert_eq!(update.focus, ROOT);
        let (id, root) = &update.nodes[0];
        assert_eq!(*id, ROOT);
        assert_eq!(root.role(), Role::Window);
        assert_eq!(root.label(), Some("Panel"));
        assert_eq!(root.children(), [NodeId(1), NodeId(2)]);
        let (_, menu) = &update.nodes[1];
        assert_eq!((menu.role(), menu.label()), (Role::Button, Some("Menu")));
        let (_, clock) = &update.nodes[2];
        assert_eq!((clock.role(), clock.value()), (Role::Label, Some("14:05")));
        assert_eq!(clock.bounds(), Some(items[1].bounds));
    }

    #[test]
    fn a_widgets_parts_are_its_children() {
        let icon = |label: &str, x: f64| Item {
            role: Role::Button,
            label: label.into(),
            bounds: Rect::new(x, 12.0, x + 30.0, 52.0),
            children: vec![],
            toggled: None,
            value: None,
        };
        let items = [
            Item {
                role: Role::Group,
                label: "Tray: Network, Volume".into(),
                bounds: Rect::new(1100.0, 12.0, 1164.0, 52.0),
                children: vec![icon("Network", 1102.0), icon("Volume", 1132.0)],
                toggled: None,
                value: None,
            },
            Item {
                role: Role::Label,
                label: "14:05".into(),
                bounds: Rect::new(1216.0, 12.0, 1280.0, 52.0),
                children: vec![],
                toggled: None,
                value: None,
            },
        ];
        let update = tree((1280.0, 52.0), &items);
        let node = |i: usize| &update.nodes[i];
        assert_eq!(update.nodes.len(), 5);
        // The panel holds the group and the clock; the group its icons.
        assert_eq!(node(0).1.children(), [NodeId(1), NodeId(4)]);
        assert_eq!(node(1).1.children(), [NodeId(2), NodeId(3)]);
        assert_eq!((node(2).0, node(2).1.label()), (NodeId(2), Some("Network")));
        assert_eq!((node(3).0, node(3).1.label()), (NodeId(3), Some("Volume")));
        assert_eq!(node(4).1.value(), Some("14:05"));
    }
}
