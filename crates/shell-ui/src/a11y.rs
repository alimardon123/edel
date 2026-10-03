//! What screen readers read (M5.1d): each panel is a window in an AccessKit
//! tree whose children are its widgets, each with the role and label its
//! line in the widget table gives and its place in the panel. The Unix
//! adapter serves the tree over AT-SPI on the session's D-Bus, and only
//! once a reader has turned accessibility on (`org.a11y.Status`); until
//! then it holds the latest tree and sends nothing.

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
}

/// The panel itself, the tree's root.
const ROOT: NodeId = NodeId(0);

/// The whole tree of a panel `size` big (logical pixels) holding `items`.
pub fn tree(size: (f64, f64), items: &[Item]) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label("Panel");
    root.set_bounds(Rect::new(0.0, 0.0, size.0, size.1));
    let mut nodes = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let id = NodeId(i as u64 + 1);
        let mut node = Node::new(item.role);
        // A label's text is its value; anything else is named by its label.
        if item.role == Role::Label {
            node.set_value(item.label.as_str());
        } else {
            node.set_label(item.label.as_str());
        }
        node.set_bounds(item.bounds);
        root.push_child(id);
        nodes.push((id, node));
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
        // A panel takes no keyboard focus yet (the launcher does, M5.3).
        focus: ROOT,
    }
}

/// A panel's link to screen readers.
pub struct Reader {
    adapter: Adapter,
    latest: Arc<Mutex<Option<TreeUpdate>>>,
}

impl Reader {
    pub fn new() -> Reader {
        let latest = Arc::new(Mutex::new(None));
        let adapter = Adapter::new(Latest(Arc::clone(&latest)), Nothing, Nothing);
        Reader { adapter, latest }
    }

    /// The panel now shows `tree`; a reader listening hears of the change.
    pub fn update(&mut self, tree: TreeUpdate) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(tree.clone());
        }
        self.adapter.update_if_active(|| tree);
    }
}

/// Gives a reader that turns accessibility on the panel's latest tree.
struct Latest(Arc<Mutex<Option<TreeUpdate>>>);

impl ActivationHandler for Latest {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok()?.clone()
    }
}

/// The panel's widgets take no actions yet; the menu button opens the
/// launcher with M5.3.
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
            },
            Item {
                role: Role::Label,
                label: "14:05".into(),
                bounds: Rect::new(1216.0, 12.0, 1280.0, 52.0),
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
}
