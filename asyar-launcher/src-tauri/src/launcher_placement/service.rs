//! Applies a [`LauncherPlacement`] to the real window, and turns a drop back
//! into one.
//!
//! ## Coordinate space
//!
//! [`super::resolve`] works in a **monitor-relative, top-left origin, y-down**
//! space: `(0, 0)` is the monitor's top-left corner. Tauri's monitor APIs are
//! already top-left y-down, so the conversion is an origin offset.

use super::classify;
use super::resolve;
use super::resolve::{resolve_origin, Rect, TravelSpace};
use super::store;
use super::types::{LauncherAnchor, LauncherMonitorChoice, LauncherPlacement};
use super::LAUNCHER_MAX_HEIGHT;
use crate::error::AppError;
use crate::{AppState, SPOTLIGHT_LABEL};
use tauri::{AppHandle, Manager, Runtime};

fn reserve_height_for_scale(scale: f64) -> f64 {
    LAUNCHER_MAX_HEIGHT * scale
}

fn reserve_height<R: Runtime>(app: &AppHandle<R>) -> f64 {
    reserve_height_for_scale(app.state::<AppState>().ui_scale())
}

/// A monitor and its usable area, both monitor-relative and y-down, as
/// [`resolve_origin`] wants them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frames {
    pub monitor: Rect,
    pub work: Rect,
}

/// COMPAT(remove in 0.2.0): migrates a pre-#860 anchor at the only ingress
/// that knows the selected monitor, then persists it away from the reveal's
/// latency-sensitive path.
fn migrate_on_reveal<R: Runtime + 'static>(
    app: &AppHandle<R>,
    placement: &LauncherPlacement,
    space: TravelSpace,
) -> LauncherAnchor {
    let anchor = classify::migrate_anchor(placement.anchor, space);
    if anchor != placement.anchor {
        let app = app.clone();
        let migrated = LauncherPlacement {
            anchor,
            ..*placement
        };
        std::mem::drop(tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) = store::save(&app, migrated) {
                log::warn!("[launcher-placement] could not persist the migrated anchor: {error}");
            }
        }));
    }
    anchor
}

/// Positions the launcher for a reveal. Fail-soft by contract: the caller is
/// mid-reveal and a placement problem must never stop the window appearing, so
/// every failure path leaves the window where it was.
pub fn apply<R: Runtime + 'static>(app: &AppHandle<R>) -> Result<(), AppError> {
    let placement = store::load(app);
    apply_placement(app, &placement)
}

fn apply_placement<R: Runtime + 'static>(
    app: &AppHandle<R>,
    placement: &LauncherPlacement,
) -> Result<(), AppError> {
    use tauri::{LogicalPosition, Position};

    let window = app
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;

    let monitor = pick_monitor(app, &window, placement.monitor)
        .ok_or_else(|| AppError::NotFound("target monitor".to_string()))?;
    let (origin, frames) = monitor_frames(&monitor);

    let scale = monitor.scale_factor();
    let window_width = window
        .outer_size()
        .map(|s| s.to_logical::<f64>(scale).width)
        .unwrap_or_else(|_| 750.0 * app.state::<AppState>().ui_scale());

    let reserve_height = reserve_height(app);
    let space = TravelSpace::new(frames.monitor, window_width, reserve_height);
    let anchor = migrate_on_reveal(app, placement, space);
    let (dx, dy) = resolve_origin(
        &anchor,
        frames.monitor,
        frames.work,
        window_width,
        reserve_height,
    );

    window
        .set_position(Position::Logical(LogicalPosition {
            x: origin.0 + dx,
            y: origin.1 + dy,
        }))
        .map_err(|e| AppError::Platform(format!("set_position: {e}")))?;
    Ok(())
}

/// Records where a drag left the launcher, as fractions of the display it was
/// dropped on. Registered as the launcher's [`crate::window_drag`] drop
/// handler, so it runs once per drag rather than once per frame.
pub fn persist_dragged<R: Runtime>(app: &AppHandle<R>) {
    if let Ok(mut slot) = last_snap_state().lock() {
        *slot = None;
    }
    let _ = crate::snap_guides::service::hide(app);

    match dragged_anchor(app) {
        Some(anchor) => {
            let placement = LauncherPlacement {
                anchor,
                ..store::load(app)
            };
            if let Err(e) = store::save(app, placement) {
                log::warn!("[launcher-placement] could not persist dragged position: {e}");
            }
        }
        None => {
            log::warn!("[launcher-placement] drag ended but the window position was unreadable")
        }
    }
}

fn dragged_anchor<R: Runtime>(app: &AppHandle<R>) -> Option<LauncherAnchor> {
    let window = app.get_webview_window(SPOTLIGHT_LABEL)?;
    let scale = window.scale_factor().ok()?;
    let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.outer_size().ok()?.to_logical::<f64>(scale);

    let center = (pos.x + size.width / 2.0, pos.y + size.height / 2.0);
    let monitor = window
        .monitor_from_point(center.0, center.1)
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())?;

    let (origin, frames) = monitor_frames(&monitor);
    let offsets = (pos.x - origin.0, pos.y - origin.1);
    let reserve_height = reserve_height(app);
    let space = TravelSpace::new(frames.monitor, size.width, reserve_height);
    Some(classify::classify_drop(offsets, space, frames.work))
}

/// How far (logical px) a drag must be from the snap target before it
/// releases — see [`resolve::apply_snap`].
const SNAP_THRESHOLD_PX: f64 = 12.0;

/// Whether each axis was snapped as of the *previous* `adjust_for_snap`
/// call — `None` between drags. Used only to fire the haptic tick on the
/// false→true edge (not continuously while held) and to detect "this is the
/// first move of a fresh drag" (no entry yet). Single slot, not a
/// per-label registry like `window_drag`'s own state: this module only
/// ever drives the launcher.
fn last_snap_state() -> &'static std::sync::Mutex<Option<resolve::SnapState>> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<Option<resolve::SnapState>>> =
        std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(None))
}

/// The launcher's [`crate::window_drag`] move adjuster: magnetically snaps
/// the live drag position, and drives the guide window + haptic feedback as
/// a side effect. Registered in `lib.rs` alongside the existing
/// `persist_dragged` drop handler.
///
/// This works entirely in `window_drag`'s own Tauri-normalized absolute space.
pub fn adjust_for_snap<R: Runtime>(app: &AppHandle<R>, x: f64, y: f64) -> (f64, f64) {
    let placement = store::load(app);
    if !placement.snap_enabled {
        return (x, y);
    }
    let Some(window) = app.get_webview_window(SPOTLIGHT_LABEL) else {
        return (x, y);
    };
    let (Ok(scale), Ok(size)) = (window.scale_factor(), window.outer_size()) else {
        return (x, y);
    };
    let size = size.to_logical::<f64>(scale);
    let center = (x + size.width / 2.0, y + size.height / 2.0);

    let monitor = window
        .monitor_from_point(center.0, center.1)
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return (x, y);
    };
    let (origin, frames) = monitor_frames(&monitor);
    let reserve_height = reserve_height(app);

    // Monitor-relative, per `monitor_frames`'s own contract (`frames.monitor`
    // is always 0-based) — matches the guide window's own local coordinates,
    // since the guide window is positioned at absolute `origin` and sized to
    // the monitor.
    let target = resolve::snap_targets(frames.monitor, frames.work, size.width, reserve_height);
    // `x, y` are absolute desktop coordinates — `window_drag_move` keeps the
    // live drag in Tauri's own absolute logical space the whole time.
    // Comparing that directly against the
    // monitor-relative `target` would silently never snap on any monitor
    // whose absolute origin isn't `(0, 0)`, so `apply_snap_absolute` adds
    // `origin` back in first — the same step `apply_placement` and
    // `dragged_anchor` both already take after calling
    // `resolve_origin`/before converting through `TravelSpace`.
    let ((snapped_x, snapped_y), state) =
        apply_snap_absolute(x, y, origin, target, SNAP_THRESHOLD_PX);

    handle_snap_feedback(app, origin, frames.monitor, target, size.width, state);

    (snapped_x, snapped_y)
}

/// Combines a monitor's absolute logical `origin` (from [`monitor_frames`])
/// with a monitor-relative `target` (from [`resolve::snap_targets`]) before
/// delegating to [`resolve::apply_snap`], which needs the live position and
/// the target expressed in the same space — and the live position here is
/// always absolute (see the note at its call site in [`adjust_for_snap`]).
/// Kept as a small pure helper so the origin-adjustment step itself is
/// unit-testable without a live window or monitor.
fn apply_snap_absolute(
    x: f64,
    y: f64,
    origin: (f64, f64),
    target: (f64, f64),
    threshold: f64,
) -> ((f64, f64), resolve::SnapState) {
    let target_absolute = (origin.0 + target.0, origin.1 + target.1);
    resolve::apply_snap(x, y, target_absolute, threshold)
}

/// Shows the guide window on the first move of a fresh drag and pushes the guide
/// window's state — but only when it actually changed, so a held-snapped
/// drag doesn't flood IPC at animation-frame rate.
fn handle_snap_feedback<R: Runtime>(
    app: &AppHandle<R>,
    monitor_origin: (f64, f64),
    monitor_size: resolve::Rect,
    target: (f64, f64),
    window_width: f64,
    state: resolve::SnapState,
) {
    let Ok(mut slot) = last_snap_state().lock() else {
        return;
    };
    let previous = *slot;

    if previous.is_none() {
        let _ = crate::snap_guides::service::show(
            app,
            monitor_origin,
            (monitor_size.width, monitor_size.height),
        );
    }

    let changed = match previous {
        Some(p) => p != state,
        None => true,
    };
    if changed {
        let guide_state = crate::snap_guides::SnapGuideState {
            left_x: target.0,
            right_x: target.0 + window_width,
            y: target.1,
            snapped_x: state.x,
            snapped_y: state.y,
        };
        let _ = crate::snap_guides::service::set_state(app, guide_state);
    }

    *slot = Some(state);
}

// --- monitor selection -----------------------------------------------------

/// Cursor-monitor lookup. This is new with #596 — the primary display was
/// previously hardcoded, which is the "it always opens on the wrong screen"
/// half of the issue.
fn pick_monitor<R: Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    choice: LauncherMonitorChoice,
) -> Option<tauri::Monitor> {
    let primary = || window.primary_monitor().ok().flatten();
    match choice {
        LauncherMonitorChoice::Primary => primary(),
        LauncherMonitorChoice::Cursor => app
            .cursor_position()
            .ok()
            .and_then(|p| window.monitor_from_point(p.x, p.y).ok().flatten())
            .or_else(primary),
    }
}

/// A Tauri monitor's absolute logical origin, plus its frames in the neutral
/// (monitor-relative) space.
///
/// Used by `dragged_anchor` and by `adjust_for_snap` (the live drag already
/// works in Tauri's own normalized space).
fn monitor_frames(m: &tauri::Monitor) -> ((f64, f64), Frames) {
    let scale = m.scale_factor();
    let size = m.size().to_logical::<f64>(scale);
    let pos = m.position().to_logical::<f64>(scale);
    let work = m.work_area();
    let work_pos = work.position.to_logical::<f64>(scale);
    let work_size = work.size.to_logical::<f64>(scale);

    (
        (pos.x, pos.y),
        Frames {
            monitor: Rect::new(0.0, 0.0, size.width, size.height),
            work: Rect::new(
                work_pos.x - pos.x,
                work_pos.y - pos.y,
                work_size.width,
                work_size.height,
            ),
        },
    )
}

#[cfg(test)]
mod snap_persistence_tests {
    use super::*;

    #[test]
    fn reserve_height_scales_the_base() {
        let monitor = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let scaled_reserve = reserve_height_for_scale(1.2);

        let (_, y) = resolve_origin(
            &LauncherAnchor::Centered,
            monitor,
            monitor,
            750.0,
            scaled_reserve,
        );

        assert_eq!(scaled_reserve, LAUNCHER_MAX_HEIGHT * 1.2);
        assert_eq!(y, (monitor.height - LAUNCHER_MAX_HEIGHT * 1.2) / 2.0);
    }

    #[test]
    fn apply_snap_absolute_accounts_for_the_monitors_origin() {
        // A monitor sitting away from Tauri's absolute origin, as on a real
        // multi-monitor desktop — this is exactly the case a monitor-relative
        // `target` compared directly against an absolute `x, y` would
        // silently miss (the bug this helper exists to prevent).
        let origin = (1920.0, 100.0);
        let target = (200.0, 50.0); // monitor-relative
        let absolute = (origin.0 + target.0, origin.1 + target.1);
        let (adjusted, state) = apply_snap_absolute(absolute.0, absolute.1, origin, target, 12.0);
        assert_eq!(adjusted, absolute);
        assert_eq!(state, resolve::SnapState { x: true, y: true });
    }

    #[test]
    fn apply_snap_absolute_does_not_snap_on_the_monitor_relative_coordinates_alone() {
        // Sitting exactly at `target`'s own (monitor-relative) numbers is
        // actually far away in absolute space once `origin` is added back
        // in — the case a comparison that forgot the origin would have
        // wrongly reported as snapped.
        let origin = (1920.0, 100.0);
        let target = (200.0, 50.0);
        let (adjusted, state) = apply_snap_absolute(target.0, target.1, origin, target, 12.0);
        assert_eq!(adjusted, target, "far outside the threshold, so unchanged");
        assert_eq!(state, resolve::SnapState { x: false, y: false });
    }

    #[test]
    fn apply_snap_absolute_is_a_no_op_when_the_monitor_sits_at_the_desktop_origin() {
        // The single-monitor case: absolute and monitor-relative coincide,
        // so this must behave identically to calling `resolve::apply_snap`
        // directly.
        let origin = (0.0, 0.0);
        let target = (200.0, 50.0);
        let (adjusted, state) = apply_snap_absolute(205.0, 45.0, origin, target, 12.0);
        assert_eq!(adjusted, target);
        assert_eq!(state, resolve::SnapState { x: true, y: true });
    }

    #[test]
    fn apply_placement_falls_back_when_outer_size_fails() {
        let source = include_str!("service.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !source.contains(r#"map_err(|e| AppError::Platform(format!("outer_size: {e}")))?"#),
            "apply_placement must not abort when outer_size() fails on an unmapped Wayland window"
        );
        assert!(
            source.contains("unwrap_or_else(|_| 750.0 * app.state::<AppState>().ui_scale())"),
            "apply_placement must fall back to scaled 750.0 width when outer_size() fails"
        );
    }
}
