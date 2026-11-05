//! The global menu on Linux: the application menu served over D-Bus
//! ([`super::dbus`]) and tied to the window, so a desktop that shows menus
//! outside their windows (KDE Plasma's Global Menu widget, and Plasma's
//! window title bar button) shows ZapFast's File, Edit, View and Help there.
//!
//! The menu is served once per process, on a thread of its own, and
//! registered with each new window when it first draws: on Wayland through
//! KDE's `org_kde_kwin_appmenu` protocol, on X11 through the
//! `_KDE_NET_WM_APPMENU_*` window properties. A desktop without a global menu
//! has neither and ignores both; nothing else changes.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::{Once, OnceLock};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

/// The menu's unique name on the session bus once it is served, or `None`
/// when there is no session bus to serve it on.
static ADDRESS: OnceLock<Option<String>> = OnceLock::new();
static STARTED: Once = Once::new();

thread_local! {
    /// The window the menu was last offered to, by its surface or window id,
    /// and what keeps that offer alive. Never dropped: a Wayland offer's
    /// objects belong to the window's display, which may be gone by then.
    static BOUND: RefCell<Option<(usize, ManuallyDrop<Option<Registration>>)>> =
        const { RefCell::new(None) };
}

/// Serves the menu on a thread of its own, once.
fn start() {
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("app-menu".into())
            .spawn(|| {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        log::warn!("global menu unavailable: {error}");
                        let _ = ADDRESS.set(None);
                        return;
                    }
                };
                runtime.block_on(async {
                    match super::dbus::serve(super::push).await {
                        Ok(connection) => {
                            let _ =
                                ADDRESS.set(connection.unique_name().map(|name| name.to_string()));
                            // Serves until the process ends.
                            let _connection = connection;
                            std::future::pending::<()>().await;
                        }
                        Err(error) => {
                            log::info!("global menu unavailable: {error}");
                            let _ = ADDRESS.set(None);
                        }
                    }
                });
            });
        if let Err(error) = spawned {
            log::warn!("global menu unavailable: {error}");
            let _ = ADDRESS.set(None);
        }
    });
}

/// A new window: the menu is offered to it when it first draws, even should
/// its surface reuse the last one's address.
pub fn new_window() {
    BOUND.with_borrow_mut(|bound| {
        // ManuallyDrop: the old offer is left, not dropped (see BOUND).
        bound.take();
    });
}

/// Offers the menu to the window `frame` draws, once per window. Cheap to
/// call every frame; it waits until the menu is served.
pub fn bind(frame: &eframe::Frame) {
    start();
    let Some(Some(address)) = ADDRESS.get() else {
        return;
    };
    let (Ok(window), Ok(display)) = (frame.window_handle(), frame.display_handle()) else {
        return;
    };
    let key = match window.as_raw() {
        RawWindowHandle::Wayland(handle) => handle.surface.as_ptr() as usize,
        RawWindowHandle::Xlib(handle) => handle.window as usize,
        RawWindowHandle::Xcb(handle) => handle.window.get() as usize,
        _ => return,
    };
    if BOUND.with_borrow(|bound| bound.as_ref().is_some_and(|(bound, _)| *bound == key)) {
        return;
    }
    let registration = match (window.as_raw(), display.as_raw()) {
        (RawWindowHandle::Wayland(window), RawDisplayHandle::Wayland(display)) => {
            // SAFETY: both pointers come from the live window this frame
            // draws, and its display outlives the window.
            unsafe { wayland(display.display, window.surface, address) }.map(Registration::Wayland)
        }
        (RawWindowHandle::Xlib(window), _) => {
            x11(window.window as u32, address).then_some(Registration::X11)
        }
        (RawWindowHandle::Xcb(window), _) => {
            x11(window.window.get(), address).then_some(Registration::X11)
        }
        _ => None,
    };
    if registration.is_some() {
        log::info!("global menu offered to the window");
    }
    // Remembered even when the desktop has no global menu, so it is not
    // asked again every frame.
    BOUND.with_borrow_mut(|bound| *bound = Some((key, ManuallyDrop::new(registration))));
}

/// What keeps an offer alive.
enum Registration {
    Wayland(Wayland),
    /// X11 keeps the properties on the window itself.
    X11,
}

/// Dispatches the few events our Wayland objects could get: none.
struct Menus;

struct Wayland {
    _connection: wayland_client::Connection,
    _globals: wayland_client::globals::GlobalList,
    _queue: wayland_client::EventQueue<Menus>,
    _menu: wayland_protocols_plasma::appmenu::client::org_kde_kwin_appmenu::OrgKdeKwinAppmenu,
}

impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for Menus
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

wayland_client::delegate_noop!(Menus: wayland_protocols_plasma::appmenu::client::org_kde_kwin_appmenu_manager::OrgKdeKwinAppmenuManager);
wayland_client::delegate_noop!(Menus: wayland_protocols_plasma::appmenu::client::org_kde_kwin_appmenu::OrgKdeKwinAppmenu);

/// Tells KWin where the window's menu is, through winit's own connection to
/// the compositor. `None` where the compositor has no appmenu protocol.
///
/// # Safety
///
/// `display` and `surface` must be the live `wl_display` and `wl_surface`
/// of the window being drawn.
unsafe fn wayland(
    display: NonNull<c_void>,
    surface: NonNull<c_void>,
    address: &str,
) -> Option<Wayland> {
    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::protocol::wl_surface::WlSurface;
    use wayland_client::{Connection, Proxy};
    use wayland_protocols_plasma::appmenu::client::org_kde_kwin_appmenu_manager::OrgKdeKwinAppmenuManager;

    // SAFETY: the caller hands us winit's live display.
    let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    // SAFETY: the caller hands us the window's live surface.
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }.ok()?;
    let surface = WlSurface::from_id(&connection, id).ok()?;
    let (globals, queue) =
        wayland_client::globals::registry_queue_init::<Menus>(&connection).ok()?;
    let handle = queue.handle();
    let manager: OrgKdeKwinAppmenuManager = globals.bind(&handle, 1..=2, ()).ok()?;
    let menu = manager.create(&surface, &handle, ());
    menu.set_address(address.to_owned(), super::dbus::PATH.to_owned());
    connection.flush().ok()?;
    Some(Wayland {
        _connection: connection,
        _globals: globals,
        _queue: queue,
        _menu: menu,
    })
}

/// Sets the window properties KWin reads the menu's address from on X11.
fn x11(window: u32, address: &str) -> bool {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    let Ok((connection, _)) = x11rb::connect(None) else {
        return false;
    };
    let set = |name: &[u8], value: &str| -> Result<(), Box<dyn std::error::Error>> {
        let atom = connection.intern_atom(false, name)?.reply()?.atom;
        connection.change_property8(
            PropMode::REPLACE,
            window,
            atom,
            AtomEnum::STRING,
            value.as_bytes(),
        )?;
        Ok(())
    };
    let result = set(b"_KDE_NET_WM_APPMENU_SERVICE_NAME", address)
        .and_then(|()| set(b"_KDE_NET_WM_APPMENU_OBJECT_PATH", super::dbus::PATH))
        .and_then(|()| Ok(connection.flush()?));
    if let Err(error) = &result {
        log::info!("global menu not offered on X11: {error}");
    }
    result.is_ok()
}
