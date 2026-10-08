//! What screen readers read (M5.1d): each panel is a window in an AccessKit
//! tree whose children are its widgets, each with the role and label its
//! line in the widget table gives and its place in the panel. The Unix
//! adapter serves the tree over AT-SPI on the session's D-Bus, and only
//! once a reader has turned accessibility on (`org.a11y.Status`); until
//! then shell-ui keeps what the panel shows and builds no tree.

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
}

/// The panel itself, the tree's root.
const ROOT: NodeId = NodeId(0);

/// The whole tree of a panel `size` big (logical pixels) holding `items`.
pub fn tree(size: (f64, f64), items: &[Item]) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label(tr("Panel"));
    root.set_bounds(Rect::new(0.0, 0.0, size.0, size.1));
    let mut nodes = Vec::new();
    for item in items {
        let id = add(&mut nodes, item);
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
        // A panel takes no keyboard focus; the launcher, which does, is
        // not in the tree yet.
        focus: ROOT,
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
    for part in &item.children {
        let child = add(nodes, part);
        node.push_child(child);
    }
    nodes[id.0 as usize - 1].1 = node;
    id
}

/// What a panel shows a reader: its size and its widgets.
type Shown = ((f64, f64), Vec<Item>);

/// A panel's link to screen readers.
pub struct Reader {
    adapter: Adapter,
    latest: Arc<Mutex<Option<Shown>>>,
}

impl Reader {
    pub fn new() -> Reader {
        let latest = Arc::new(Mutex::new(None));
        let adapter = Adapter::new(Latest(Arc::clone(&latest)), Nothing, Nothing);
        Reader { adapter, latest }
    }

    /// The panel, `size` big, now shows `items`; a reader listening hears
    /// of the change, and the tree is built only for one.
    pub fn update(&mut self, size: (f64, f64), items: Vec<Item>) {
        // Kept first, so a reader turning accessibility on meanwhile gets
        // this, not the one before.
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some((size, items.clone()));
        }
        self.adapter.update_if_active(|| tree(size, &items));
    }
}

/// Gives a reader that turns accessibility on the panel's latest tree.
struct Latest(Arc<Mutex<Option<Shown>>>);

impl ActivationHandler for Latest {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let latest = self.0.lock().ok()?;
        let (size, items) = latest.as_ref()?;
        Some(tree(*size, items))
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
            },
            Item {
                role: Role::Label,
                label: "14:05".into(),
                bounds: Rect::new(1216.0, 12.0, 1280.0, 52.0),
                children: vec![],
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
        };
        let items = [
            Item {
                role: Role::Group,
                label: "Tray: Network, Volume".into(),
                bounds: Rect::new(1100.0, 12.0, 1164.0, 52.0),
                children: vec![icon("Network", 1102.0), icon("Volume", 1132.0)],
            },
            Item {
                role: Role::Label,
                label: "14:05".into(),
                bounds: Rect::new(1216.0, 12.0, 1280.0, 52.0),
                children: vec![],
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
