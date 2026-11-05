//! The application menu as `com.canonical.dbusmenu`, the interface KDE
//! Plasma's Global Menu (and other desktops' global menus) read a window's
//! menu from. [`super::linux`] tells the compositor where to find it.
//!
//! The menu never changes while ZapFast runs, so it is a fixed tree: item ids
//! are their positions in a depth-first walk, the layout's revision stays 1,
//! and nothing is ever signalled as updated. A click hands the item's name to
//! the callback it was served with, [`super::push`] in the app.

// Compiled on every platform for its tests; only Linux serves it.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::HashMap;

use serde::Serialize;
use zbus::zvariant::{self, OwnedValue, Type, Value};

/// Where the menu is served on the session bus, as Qt applications do.
pub const PATH: &str = "/MenuBar";

/// One entry of the menu.
#[derive(Clone, Copy, Debug)]
pub enum Entry {
    /// A clickable item: its id in [`super::action`] or [`super::edit_event`],
    /// its label with `_` before the mnemonic, and its shortcut as dbusmenu
    /// spells keys (`Control`, `Shift`, then the key), shown beside it.
    Item {
        id: &'static str,
        label: &'static str,
        keys: &'static [&'static str],
    },
    Separator,
    Menu {
        label: &'static str,
        entries: &'static [Entry],
    },
}

const fn item(id: &'static str, label: &'static str, keys: &'static [&'static str]) -> Entry {
    Entry::Item { id, label, keys }
}

/// The menus macOS has, laid out as a Linux application's are: Settings and
/// Quit under File, About under Help. Only shortcuts ZapFast answers on
/// Linux are shown.
pub const MENU: &[Entry] = &[
    Entry::Menu {
        label: "_File",
        entries: &[
            item("new", "_New Chat…", &["Control", "n"]),
            item("settings", "_Settings…", &["Control", "comma"]),
            Entry::Separator,
            item("close", "_Close Window", &["Control", "w"]),
            item("quit", "_Quit ZapFast", &["Control", "q"]),
        ],
    },
    Entry::Menu {
        label: "_Edit",
        entries: &[
            item("undo", "_Undo", &["Control", "z"]),
            item("redo", "_Redo", &["Control", "Shift", "z"]),
            Entry::Separator,
            item("cut", "Cu_t", &["Control", "x"]),
            item("copy", "_Copy", &["Control", "c"]),
            item("paste", "_Paste", &["Control", "v"]),
            item("select-all", "Select _All", &["Control", "a"]),
            Entry::Separator,
            item("search", "_Find…", &["Control", "f"]),
        ],
    },
    Entry::Menu {
        label: "_View",
        entries: &[
            item("sidebar", "Toggle _Sidebar", &["Control", "b"]),
            Entry::Separator,
            item("zoom-in", "Zoom _In", &["Control", "plus"]),
            item("zoom-out", "Zoom _Out", &["Control", "minus"]),
            item("zoom-reset", "_Actual Size", &["Control", "0"]),
        ],
    },
    Entry::Menu {
        label: "_Help",
        entries: &[
            item("shortcuts", "_Keyboard Shortcuts", &["Control", "slash"]),
            item("help", "ZapFast _Help", &[]),
            Entry::Separator,
            item("about", "_About ZapFast", &[]),
        ],
    },
];

/// The menu flattened: node `n` has id `n`, and node 0 is the root.
#[derive(Debug)]
pub struct Tree {
    nodes: Vec<Node>,
}

#[derive(Debug)]
struct Node {
    /// The entry, or `None` for the root.
    entry: Option<Entry>,
    children: Vec<i32>,
}

impl Tree {
    pub fn new(menu: &[Entry]) -> Self {
        let mut tree = Self {
            nodes: vec![Node {
                entry: None,
                children: Vec::new(),
            }],
        };
        tree.add(0, menu);
        tree
    }

    fn add(&mut self, parent: usize, entries: &[Entry]) {
        for entry in entries {
            let id = self.nodes.len();
            self.nodes.push(Node {
                entry: Some(*entry),
                children: Vec::new(),
            });
            self.nodes[parent].children.push(id as i32);
            if let Entry::Menu { entries, .. } = entry {
                self.add(id, entries);
            }
        }
    }

    fn node(&self, id: i32) -> Option<&Node> {
        usize::try_from(id).ok().and_then(|id| self.nodes.get(id))
    }

    /// The id a click on node `id` stands for, if it is an item.
    pub fn item_id(&self, id: i32) -> Option<&'static str> {
        match self.node(id)?.entry? {
            Entry::Item { id, .. } => Some(id),
            _ => None,
        }
    }

    /// Node `id`'s properties, only those in `names` unless it is empty.
    fn properties(&self, id: i32, names: &[String]) -> Option<HashMap<String, OwnedValue>> {
        let node = self.node(id)?;
        let mut properties: Vec<(&str, OwnedValue)> = Vec::new();
        match node.entry {
            None => properties.push(("children-display", owned("submenu"))),
            Some(Entry::Menu { label, .. }) => {
                properties.push(("label", owned(label)));
                properties.push(("children-display", owned("submenu")));
            }
            Some(Entry::Item { label, keys, .. }) => {
                properties.push(("label", owned(label)));
                if !keys.is_empty() {
                    let chord: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
                    properties.push(("shortcut", owned(vec![chord])));
                }
            }
            Some(Entry::Separator) => properties.push(("type", owned("separator"))),
        }
        Some(
            properties
                .into_iter()
                .filter(|(name, _)| names.is_empty() || names.iter().any(|wanted| wanted == name))
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    /// Node `id` with its children `depth` levels down, all of them when
    /// `depth` is negative.
    pub fn layout(&self, id: i32, depth: i32, names: &[String]) -> Option<Layout> {
        let node = self.node(id)?;
        let children = if depth == 0 {
            Vec::new()
        } else {
            node.children
                .iter()
                .filter_map(|child| self.layout(*child, depth - 1, names))
                .map(Value::from)
                .collect()
        };
        Some(Layout {
            id,
            properties: self.properties(id, names)?,
            children,
        })
    }
}

fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    value
        .into()
        .try_to_owned()
        .expect("menu properties hold no file descriptors")
}

/// One node and its children, as dbusmenu's `(ia{sv}av)`.
#[derive(Debug, Serialize, Type)]
pub struct Layout {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<Value<'static>>,
}

impl From<Layout> for Value<'static> {
    fn from(layout: Layout) -> Self {
        Value::from(
            zvariant::StructureBuilder::new()
                .add_field(layout.id)
                .add_field(layout.properties)
                .add_field(layout.children)
                .build()
                .expect("a layout has three fields"),
        )
    }
}

/// The served menu: the tree and what a click does.
pub struct DbusMenu {
    tree: Tree,
    clicked: fn(&str),
}

impl DbusMenu {
    pub fn new(clicked: fn(&str)) -> Self {
        Self {
            tree: Tree::new(MENU),
            clicked,
        }
    }

    /// A menu event: a click on an item runs it; opening, closing and
    /// hovering need nothing. Unknown ids are reported.
    fn handle(&self, id: i32, event: &str) -> bool {
        if self.tree.node(id).is_none() {
            return false;
        }
        if event == "clicked"
            && let Some(item) = self.tree.item_id(id)
        {
            (self.clicked)(item);
        }
        true
    }
}

fn not_found() -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs("no such menu item".into())
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        self.tree
            .layout(parent_id, recursion_depth, &property_names)
            .map(|layout| (1, layout))
            .ok_or_else(not_found)
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let ids = if ids.is_empty() {
            (0..self.tree.nodes.len() as i32).collect()
        } else {
            ids
        };
        ids.into_iter()
            .filter_map(|id| {
                self.tree
                    .properties(id, &property_names)
                    .map(|properties| (id, properties))
            })
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        self.tree
            .properties(id, std::slice::from_ref(&name))
            .ok_or_else(not_found)?
            .remove(&name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("no such property".into()))
    }

    fn event(
        &self,
        id: i32,
        event_id: String,
        _data: OwnedValue,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        if self.handle(id, &event_id) {
            Ok(())
        } else {
            Err(not_found())
        }
    }

    fn event_group(
        &self,
        events: Vec<(i32, String, OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        let count = events.len();
        let missing: Vec<i32> = events
            .into_iter()
            .filter(|(id, event, _, _)| !self.handle(*id, event))
            .map(|(id, ..)| id)
            .collect();
        if count > 0 && missing.len() == count {
            Err(not_found())
        } else {
            Ok(missing)
        }
    }

    /// The menu never changes, so it never needs an update before showing.
    fn about_to_show(&self, id: i32) -> zbus::fdo::Result<bool> {
        self.tree.node(id).map(|_| false).ok_or_else(not_found)
    }

    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        let missing = ids
            .into_iter()
            .filter(|id| self.tree.node(*id).is_none())
            .collect();
        (Vec::new(), missing)
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".into()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "normal".into()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Serves the menu on a new session-bus connection, which must stay alive
/// as long as the menu is shown. Its unique name is the menu's address.
pub async fn serve(clicked: fn(&str)) -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .serve_at(PATH, DbusMenu::new(clicked))?
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CLICKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn record(id: &str) {
        CLICKED.lock().unwrap().push(id.to_owned());
    }

    fn label(layout: &Layout) -> String {
        String::try_from(layout.properties["label"].try_clone().unwrap()).unwrap()
    }

    fn child(layout: &Layout, index: usize) -> Layout {
        let mut fields = zvariant::Structure::try_from(layout.children[index].try_clone().unwrap())
            .unwrap()
            .into_fields();
        Layout {
            id: fields.remove(0).downcast().unwrap(),
            properties: HashMap::<String, OwnedValue>::try_from(fields.remove(0)).unwrap(),
            children: fields.remove(0).downcast().unwrap(),
        }
    }

    #[test]
    fn the_layout_has_the_four_menus_and_their_items() {
        let tree = Tree::new(MENU);
        let root = tree.layout(0, -1, &[]).unwrap();
        assert_eq!(root.children.len(), 4);
        let file = child(&root, 0);
        assert_eq!(label(&file), "_File");
        let new = child(&file, 0);
        assert_eq!(label(&new), "_New Chat…");
        assert_eq!(tree.item_id(new.id), Some("new"));
        // A depth of one stops at the menus.
        let shallow = tree.layout(0, 1, &[]).unwrap();
        assert!(child(&shallow, 0).children.is_empty());
        // Only the properties asked for.
        let named = tree.layout(new.id, 0, &["label".into()]).unwrap();
        assert_eq!(named.properties.len(), 1);
        assert!(tree.layout(9999, -1, &[]).is_none());
    }

    #[test]
    fn every_item_does_something_in_the_app() {
        fn check(entries: &[Entry]) {
            for entry in entries {
                match entry {
                    Entry::Item { id, .. } => assert!(
                        super::super::action(id).is_some()
                            || super::super::edit_event(id).is_some(),
                        "{id} does nothing"
                    ),
                    Entry::Menu { entries, .. } => check(entries),
                    Entry::Separator => {}
                }
            }
        }
        check(MENU);
    }

    #[test]
    fn a_click_runs_its_item_and_other_events_do_nothing() {
        let menu = DbusMenu::new(record);
        let quit = (0..menu.tree.nodes.len() as i32)
            .find(|id| menu.tree.item_id(*id) == Some("quit"))
            .unwrap();
        assert!(menu.handle(quit, "hovered"));
        assert!(menu.handle(quit, "clicked"));
        // A menu, not an item: nothing to run.
        assert!(menu.handle(1, "clicked"));
        assert!(!menu.handle(-1, "clicked"));
        assert_eq!(*CLICKED.lock().unwrap(), ["quit"]);
    }

    #[test]
    fn the_shortcut_is_a_list_of_chords() {
        let tree = Tree::new(MENU);
        let quit = (0..tree.nodes.len() as i32)
            .find(|id| tree.item_id(*id) == Some("quit"))
            .unwrap();
        let shortcut = tree.properties(quit, &[]).unwrap()["shortcut"]
            .try_clone()
            .unwrap();
        assert_eq!(
            Vec::<Vec<String>>::try_from(shortcut).unwrap(),
            [["Control", "q"]]
        );
    }

    #[test]
    fn types_have_dbusmenus_signatures() {
        assert_eq!(<Vec<Vec<String>> as Type>::SIGNATURE.to_string(), "aas");
        assert_eq!(<Layout as Type>::SIGNATURE.to_string(), "(ia{sv}av)");
    }
}
