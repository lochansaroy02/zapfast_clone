//! Main-thread AppKit application menus. The menu outlives windows, just like
//! the link and tray; reopening replaces only its repaint callback. The
//! traffic lights are placed by fastframe-titlebar. The Messages theme's
//! translucent sidebar is AppKit's own sidebar material, behind the window.

use std::cell::RefCell;

use objc2_app_kit::{NSApplication, NSText};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem as Native, Submenu};

use crate::model::Action;

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

fn item(id: &str, text: &str, shortcut: Option<&str>) -> MenuItem {
    MenuItem::with_id(
        id,
        text,
        true,
        shortcut.map(|key| key.parse().expect("menu shortcut")),
    )
}

fn build_menu() -> tray_icon::menu::Result<Menu> {
    let menu = Menu::new();
    let app = Submenu::new("ZapFast", true);
    app.append_items(&[
        &item("about", "About ZapFast", None),
        &Native::separator(),
        &item("settings", "Settings…", Some("Super+Comma")),
        &Native::separator(),
        &Native::services(None),
        &Native::separator(),
        &Native::hide(Some("Hide ZapFast")),
        &Native::hide_others(None),
        &Native::show_all(None),
        &Native::separator(),
        &item("quit", "Quit ZapFast", Some("Super+KeyQ")),
    ])?;
    let file = Submenu::new("File", true);
    file.append_items(&[
        &item("new", "New Chat…", Some("Super+KeyN")),
        &Native::separator(),
        &item("close", "Close Window", Some("Super+KeyW")),
    ])?;
    let edit = Submenu::new("Edit", true);
    // Winit's view is not an NSTextView: AppKit's copy:/undo: selectors
    // cannot edit egui text. Send the same events as its keyboard shortcuts.
    edit.append_items(&[
        &item("undo", "Undo", Some("Super+KeyZ")),
        &item("redo", "Redo", Some("Super+Shift+KeyZ")),
        &Native::separator(),
        &item("cut", "Cut", Some("Super+KeyX")),
        &item("copy", "Copy", Some("Super+KeyC")),
        &item("paste", "Paste", Some("Super+KeyV")),
        &item("select-all", "Select All", Some("Super+KeyA")),
        &Native::separator(),
        &item("search", "Find…", Some("Super+KeyF")),
    ])?;
    let view = Submenu::new("View", true);
    view.append_items(&[
        &item("sidebar", "Toggle Sidebar", Some("Super+KeyB")),
        &Native::separator(),
        &item("zoom-in", "Zoom In", Some("Super+Equal")),
        &item("zoom-out", "Zoom Out", Some("Super+Minus")),
        &item("zoom-reset", "Actual Size", Some("Super+Digit0")),
        &Native::separator(),
        &Native::fullscreen(None),
    ])?;
    let window = Submenu::new("Window", true);
    window.append_items(&[
        &Native::minimize(None),
        &Native::maximize(Some("Zoom")),
        &Native::separator(),
        &item("show-window", "Show ZapFast", None),
    ])?;
    let help = Submenu::new("Help", true);
    help.append_items(&[
        &item("shortcuts", "Keyboard Shortcuts", Some("Super+Slash")),
        &item("help", "ZapFast Help", None),
    ])?;
    menu.append_items(&[&app, &file, &edit, &view, &window, &help])?;
    window.set_as_windows_menu_for_nsapp();
    help.set_as_help_menu_for_nsapp();
    Ok(menu)
}

pub fn attach(ctx: &egui::Context) {
    crate::app_menu::attach(ctx);
    // Layout tests use headless contexts on test threads, without an NSApp.
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // The tray's menu shares muda's one handler; its ids are its own.
        if fastframe_tray::claim_menu_event(&event.id.0) {
            return;
        }
        if native_edit(&event.id.0) {
            return;
        }
        crate::app_menu::push(&event.id.0);
    }));
    MENU.with_borrow_mut(|menu| {
        if menu.is_none() {
            match build_menu() {
                Ok(created) => *menu = Some(created),
                Err(error) => log::warn!("could not create the application menu: {error}"),
            }
        }
        if let Some(menu) = menu {
            menu.init_for_nsapp();
        }
    });
    // Demo windows have no tray to activate the app. This also brings a newly
    // recreated window forward after a menu command while running headless.
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// A native file dialog runs a modal loop, so its text fields must receive
/// editing commands immediately instead of leaving them queued for egui.
fn native_edit(id: &str) -> bool {
    use objc2::sel;
    let selector = match id {
        "copy" => sel!(copy:),
        "cut" => sel!(cut:),
        "paste" => sel!(paste:),
        "undo" => sel!(undo:),
        "redo" => sel!(redo:),
        "select-all" => sel!(selectAll:),
        _ => return false,
    };
    let Some(main) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(main);
    let Some(responder) = app.keyWindow().and_then(|window| window.firstResponder()) else {
        return false;
    };
    // Only native text editors (including a file dialog's field editor) can
    // handle these selectors. GL views and AccessKit's dynamically subclassed
    // WinitView must use egui, regardless of their Objective-C class name.
    if responder.downcast_ref::<NSText>().is_none() {
        return false;
    }
    // SAFETY: standard AppKit editing selectors; nil target walks the native
    // responder chain, and these actions accept a nil sender.
    unsafe { app.sendAction_to_from(selector, None, None) }
}

/// The actions the menu's items asked for since the last call.
pub fn drain(hidden: bool) -> Vec<Action> {
    crate::app_menu::drain(hidden)
}

#[cfg(test)]
mod tests {
    #[test]
    fn menu_shortcuts_parse() {
        for shortcut in [
            "Super+KeyN",
            "Super+Comma",
            "Super+Equal",
            "Super+Digit0",
            "Super+Shift+KeyZ",
        ] {
            shortcut
                .parse::<tray_icon::menu::accelerator::Accelerator>()
                .unwrap();
        }
    }
}

thread_local! {
    /// The vibrancy behind the current window, while the theme asks for it,
    /// and whether it is drawn dark.
    static VIBRANCY: RefCell<Option<(objc2::rc::Retained<objc2_app_kit::NSVisualEffectView>, bool)>> =
        const { RefCell::new(None) };
}

/// Puts AppKit's sidebar material behind the window's content while `on`,
/// light or dark as `palette` is, or takes it away. The window is
/// transparent; whatever egui leaves translucent, the sidebar in the
/// Messages theme, shows the blurred desktop as Messages does. Cheap to call
/// every frame: it changes the view only when the palette does or the
/// window was made anew.
pub fn vibrancy(frame: &eframe::Frame, palette: &crate::theme::Palette, on: bool) {
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: eframe supplies a live NSView, and this runs on its main thread.
    let content = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    // SAFETY: as above; the content view's parent is the window's frame view.
    let Some(frame_view) = (unsafe { content.superview() }) else {
        return;
    };
    VIBRANCY.with_borrow_mut(|slot| {
        // A closed window took its view with it.
        let current = slot.as_ref().is_some_and(|(view, _)| {
            // SAFETY: the view is ours and alive; read on the main thread.
            unsafe { view.superview() }.is_some_and(|parent| parent == frame_view)
        });
        if !current {
            *slot = None;
        }
        match (on, slot.is_some()) {
            (true, false) => {
                let view = NSVisualEffectView::initWithFrame(mtm.alloc(), frame_view.bounds());
                view.setMaterial(NSVisualEffectMaterial::Sidebar);
                view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                view.setState(NSVisualEffectState::FollowsWindowActiveState);
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                frame_view.addSubview_positioned_relativeTo(
                    &view,
                    NSWindowOrderingMode::Below,
                    Some(content),
                );
                // Flipped below to the palette's appearance.
                *slot = Some((view, !palette.dark));
            }
            (false, true) => {
                if let Some((view, _)) = slot.take() {
                    view.removeFromSuperview();
                }
            }
            _ => {}
        }
        // The material follows the palette rather than the system, which a
        // cached choice can lag behind for a frame.
        if let Some((view, dark)) = slot.as_mut()
            && *dark != palette.dark
        {
            // SAFETY: AppKit's constants, read on the main thread.
            let name = unsafe {
                if palette.dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            view.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
            *dark = palette.dark;
        }
    });
}
