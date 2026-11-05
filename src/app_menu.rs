//! The application menu shared by the platforms that show one outside the
//! window: macOS's menu bar (`crate::macos`) and the global menu of a Linux
//! desktop such as KDE Plasma (`linux`, served over D-Bus by `dbus`).
//!
//! Both hand a clicked item's id to [`push`], from whatever thread the menu
//! runs on. Editing items (Copy, Paste, Undo…) become egui input events that
//! [`MenuInput`] adds before the next pass, so they reach the focused text
//! field; the rest become [`Action`]s that the app takes with [`drain`].

use std::sync::{Arc, LazyLock, Mutex};

use crate::model::{Action, Dialog, Page};

#[cfg(any(target_os = "linux", test))]
pub mod dbus;
#[cfg(target_os = "linux")]
pub mod linux;

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EDIT_EVENTS: LazyLock<Arc<Mutex<Vec<egui::Event>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Menu edits must reach the input before egui processes focus and selection.
struct MenuInput(Arc<Mutex<Vec<egui::Event>>>);

impl egui::plugin::Plugin for MenuInput {
    fn debug_name(&self) -> &'static str {
        "zapfast-app-menu"
    }

    fn input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .extend(self.0.lock().unwrap_or_else(|p| p.into_inner()).drain(..));
    }
}

/// Connects a new window's context: its input takes the menu's edits, and a
/// click repaints it.
pub fn attach(ctx: &egui::Context) {
    ctx.add_plugin(MenuInput(Arc::clone(&EDIT_EVENTS)));
    *REPAINT.lock().unwrap_or_else(|p| p.into_inner()) = Some(ctx.clone());
    #[cfg(target_os = "linux")]
    linux::new_window();
}

/// A menu item was clicked: queues its edit or its action and wakes the
/// window. Safe from any thread.
pub fn push(id: &str) {
    if let Some(edit) = edit_event(id) {
        EDIT_EVENTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(edit);
    } else {
        EVENTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(id.to_owned());
    }
    if let Some(ctx) = &*REPAINT.lock().unwrap_or_else(|p| p.into_inner()) {
        ctx.request_repaint();
    }
}

/// The input event an editing item stands for, as its shortcut would send.
pub(crate) fn edit_event(id: &str) -> Option<egui::Event> {
    match id {
        "copy" => return Some(egui::Event::Copy),
        "cut" => return Some(egui::Event::Cut),
        "paste" => {
            return Some(egui::Event::Paste(
                arboard::Clipboard::new()
                    .ok()
                    .and_then(|mut clipboard| clipboard.get_text().ok())
                    .unwrap_or_default(),
            ));
        }
        _ => {}
    }
    let (key, shift) = match id {
        "undo" => (egui::Key::Z, false),
        "redo" => (egui::Key::Z, true),
        "select-all" => (egui::Key::A, false),
        _ => return None,
    };
    Some(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            shift,
            ..Default::default()
        },
    })
}

/// The action an item stands for, through the app's own paths.
pub(crate) fn action(id: &str) -> Option<Action> {
    Some(match id {
        "about" => Action::ShowDialog(Dialog::About),
        "settings" => Action::Open(Page::Settings),
        "new" => Action::ShowDialog(Dialog::NewChat),
        "close" => Action::CloseWindow,
        "quit" => Action::Quit,
        "search" => Action::FocusSearch,
        "sidebar" => Action::ToggleSidebar,
        "zoom-in" => Action::ZoomBy(0.1),
        "zoom-out" => Action::ZoomBy(-0.1),
        "zoom-reset" => Action::ResetZoom,
        "shortcuts" => Action::ShowDialog(Dialog::Shortcuts),
        "help" => Action::OpenUrl("https://zapfast.rocks/using-zapfast/".into()),
        "show-window" => Action::ShowWindow,
        _ => return None,
    })
}

/// The actions clicked since the last call. A hidden window drops queued
/// edits, and shows itself again for an item that opens something in it.
pub fn drain(hidden: bool) -> Vec<Action> {
    if hidden {
        EDIT_EVENTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
    let events = std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|p| p.into_inner()));
    let mut actions = Vec::new();
    for id in events {
        if let Some(action) = action(&id) {
            if hidden
                && matches!(
                    action,
                    Action::ShowDialog(_) | Action::Open(_) | Action::FocusSearch
                )
            {
                actions.push(Action::ShowWindow);
            }
            actions.push(action);
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_edits_reach_the_focused_text_field_before_the_pass() {
        let ctx = egui::Context::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        ctx.add_plugin(MenuInput(Arc::clone(&events)));
        let id = egui::Id::new("menu-edit-fixture");
        let mut text = String::from("draft");
        let mut frame = |event: Option<egui::Event>| {
            events.lock().unwrap().extend(event);
            ctx.memory_mut(|memory| memory.request_focus(id));
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(id));
            });
            output.textures_delta.clear();
            output
        };
        let _ = frame(None);
        let _ = frame(edit_event("select-all"));
        let output = frame(edit_event("cut"));
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(value) if value == "draft")
        }));
        let _ = frame(Some(egui::Event::Paste("replacement".into())));
        let _ = frame(edit_event("select-all"));
        let output = frame(edit_event("copy"));
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(value) if value == "replacement")
        }));
        assert_eq!(text, "replacement");
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn menu_uses_the_apps_close_quit_and_edit_paths() {
        assert!(matches!(action("close"), Some(Action::CloseWindow)));
        assert!(matches!(action("quit"), Some(Action::Quit)));
        assert!(matches!(action("show-window"), Some(Action::ShowWindow)));
        assert!(matches!(edit_event("copy"), Some(egui::Event::Copy)));
        assert!(
            matches!(edit_event("redo"), Some(egui::Event::Key { key: egui::Key::Z, modifiers, .. }) if modifiers.command && modifiers.shift)
        );
    }
}
