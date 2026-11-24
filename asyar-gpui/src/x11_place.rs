//! Centers the launcher on the monitor under the pointer on X11.
//!
//! shortcut: GPUI's X11 backend sets no position flag in WM_NORMAL_HINTS,
//! so the window manager places the window itself and ignores the requested
//! origin. Remove this module once GPUI sets PPosition upstream.

use std::sync::OnceLock;

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

/// A monitor rectangle in X11 root coordinates (device pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn inset(self, by: i32) -> Self {
        Self {
            x: self.x + by,
            y: self.y + by,
            width: (self.width - 2 * by).max(1),
            height: (self.height - 2 * by).max(1),
        }
    }
}

/// Whether a compositing manager runs, i.e. window transparency will show.
/// Always true off X11 (Wayland compositors composite by definition).
pub fn compositing() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return true;
    }
    let Some((conn, screen)) = connection() else {
        return false;
    };
    let name = format!("_NET_WM_CM_S{screen}");
    conn.intern_atom(true, name.as_bytes())
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .filter(|atom| atom.atom != 0)
        .and_then(|atom| conn.get_selection_owner(atom.atom).ok())
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|owner| owner.owner != 0)
}

/// Horizontally centered, a fifth of the way down: launchers sit above the
/// visual middle so results grow downwards in view.
fn target(monitors: &[Rect], pointer: (i32, i32), window: (i32, i32)) -> Option<(i32, i32)> {
    let (px, py) = pointer;
    let monitor = monitors
        .iter()
        .find(|m| px >= m.x && px < m.x + m.width && py >= m.y && py < m.y + m.height)
        .or(monitors.first())?;
    Some((
        monitor.x + (monitor.width - window.0) / 2,
        monitor.y + (monitor.height - window.1) / 5,
    ))
}

/// Returns where the window now sits, or `None` off X11 or on error.
pub fn center_on_pointer_monitor(window: &Window) -> Option<Rect> {
    try_center(window)
        .inspect_err(|error| eprintln!("[asyar-gpui] could not position the window: {error:#}"))
        .ok()
        .flatten()
}

pub fn connection() -> Option<&'static (RustConnection, usize)> {
    static CONN: OnceLock<Option<(RustConnection, usize)>> = OnceLock::new();
    CONN.get_or_init(|| x11rb::connect(None).ok()).as_ref()
}

/// The window's X11 id; `None` on Wayland.
fn xid(window: &Window) -> Option<u32> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        RawWindowHandle::Xlib(handle) => Some(handle.window as u32),
        _ => None,
    }
}

/// Maps or unmaps the window: GPUI has no show/hide, and re-creating the
/// window costs ~100 ms per summon. Returns false off X11.
pub fn set_mapped(window: &Window, mapped: bool) -> bool {
    let (Some(xid), Some((conn, _))) = (xid(window), connection()) else {
        return false;
    };
    let _ = if mapped {
        conn.map_window(xid)
    } else {
        conn.unmap_window(xid)
    };
    let _ = conn.flush();
    true
}

fn try_center(window: &Window) -> anyhow::Result<Option<Rect>> {
    // Wayland: layer-shell anchors the window instead.
    let Some(xid) = xid(window) else {
        return Ok(None);
    };
    let Some((conn, screen)) = connection() else {
        anyhow::bail!("no X11 connection");
    };
    let root = conn.setup().roots[*screen].root;
    let pointer = conn.query_pointer(root)?.reply()?;
    let geometry = conn.get_geometry(xid)?.reply()?;
    let monitors: Vec<Rect> = conn
        .randr_get_monitors(root, true)?
        .reply()?
        .monitors
        .iter()
        .map(|m| Rect {
            x: m.x.into(),
            y: m.y.into(),
            width: m.width.into(),
            height: m.height.into(),
        })
        .collect();
    let (width, height) = (geometry.width.into(), geometry.height.into());
    let Some((x, y)) = target(
        &monitors,
        (pointer.root_x.into(), pointer.root_y.into()),
        (width, height),
    ) else {
        return Ok(None);
    };

    // Keep GPUI's size hints and add a user-specified position the WM honours.
    let mut hints = WmSizeHints::get_normal_hints(conn, xid)?
        .reply()?
        .unwrap_or_default();
    hints.position = Some((WmSizeHintsSpecification::UserSpecified, x, y));
    hints.set_normal_hints(conn, xid)?;
    conn.configure_window(xid, &ConfigureWindowAux::new().x(x).y(y))?;
    conn.flush()?;
    Ok(Some(Rect {
        x,
        y,
        width,
        height,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Rect = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    const RIGHT: Rect = Rect {
        x: 2560,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn centers_on_the_monitor_under_the_pointer() {
        assert_eq!(
            target(&[LEFT, RIGHT], (3000, 500), (1360, 880)),
            Some((2560 + 280, 40))
        );
        assert_eq!(
            target(&[LEFT, RIGHT], (10, 10), (1360, 880)),
            Some((600, 112))
        );
    }

    #[test]
    fn falls_back_to_the_first_monitor_and_handles_none() {
        assert_eq!(
            target(&[LEFT, RIGHT], (-5, -5), (1360, 880)),
            Some((600, 112))
        );
        assert_eq!(target(&[], (0, 0), (10, 10)), None);
    }
}
