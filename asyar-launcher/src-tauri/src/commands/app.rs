//! Window visibility and focus-lock commands.
//!
//! `show`/`hide` are single-shot; `prepare_show` + `commit_show` is the
//! flash-free reveal path documented on those functions.

use crate::error::AppError;
use crate::{AppState, SPOTLIGHT_LABEL};
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tauri::Manager;
use tauri::{LogicalPosition, Position};

/// Off-screen coordinate used to make the window invisible while keeping it
/// mapped, so the webview's render process stays in its "visible" activity
/// state and keeps pushing frames.
const OFFSCREEN_COORD: f64 = -30_000.0;

/// Phase 1 of the flash-free reveal: make the launcher imperceptible but
/// mapped, so the webview resumes rendering without the user seeing the
/// stale cached frame. The JS caller awaits two rAFs after this returns
/// before calling `commit_show`.
#[tauri::command]
pub fn prepare_show(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    crate::launcher::pop_to_root::on_launcher_revealed(&app_handle);
    // Intentionally NOT setting asyar_visible here: the window is mapped but
    // imperceptible until commit_show runs. A concurrent caller reading
    // is_visible during this window must still take the two-phase path,
    // not the single-shot `show` that would composite mid-prepare.
    capture_previous_foreground(&state)?;
    let window = app_handle
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;
    // Move off-screen before showing: the window stays in the compositor's
    // visible set (so WebKitGTK keeps pushing frames) but the user can't see
    // it. commit_show restores the final position.
    let _ = window.set_position(Position::Logical(LogicalPosition {
        x: OFFSCREEN_COORD,
        y: OFFSCREEN_COORD,
    }));
    let _ = window.show();
    #[cfg(target_os = "linux")]
    crate::mark_launcher_shown(&state);
    Ok(())
}

/// Phase 2 of the flash-free reveal: position the launcher correctly and
/// reveal it. By the time the JS caller invokes this (after two rAFs) the
/// webview has already committed at least one fresh frame while imperceptible,
/// so the user sees the target UI immediately.
#[tauri::command]
pub fn commit_show(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let window = app_handle
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;
    if !log_placement_if_err(
        crate::launcher_placement::service::apply(&app_handle),
        "commit_show",
    ) {
        let _ = window.center();
    }
    #[cfg(target_os = "linux")]
    let _ = crate::platform::linux::present_and_focus_spotlight_window(&window);
    #[cfg(target_os = "linux")]
    crate::mark_launcher_shown(&state);
    // Flip the visibility flag only after the window is actually at its final
    // position — otherwise a concurrent showWindow() that reads is_visible
    // mid-prepare would take the single-shot `show` path before the new view
    // has committed a fresh frame.
    state.asyar_visible.store(true, Ordering::Relaxed);
    state.mark_interaction();
    crate::scripts::inline_scheduler::nudge_reveal();
    Ok(())
}

/// Single-shot reveal for callers that know the window is already visible
/// (no stale-frame risk, so no need for the two-phase dance). Also the
/// fallback used by the shortcut service when its visibility check reports
/// true.
#[tauri::command]
pub fn show(app_handle: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    crate::launcher::pop_to_root::on_launcher_revealed(&app_handle);
    state.asyar_visible.store(true, Ordering::Relaxed);
    state.mark_interaction();
    capture_previous_foreground(&state)?;
    let window = app_handle
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;
    if !log_placement_if_err(
        crate::launcher_placement::service::apply(&app_handle),
        "show",
    ) {
        let _ = window.center();
    }
    let _ = window.show();
    #[cfg(target_os = "linux")]
    let _ = crate::platform::linux::present_and_focus_spotlight_window(&window);
    #[cfg(target_os = "linux")]
    crate::mark_launcher_shown(&state);
    crate::scripts::inline_scheduler::nudge_reveal();
    Ok(())
}

/// Returns whether the launcher is currently intended to be visible. Mirrors
/// the `asyar_visible` atomic; the JS side reads this to decide whether to
/// use the two-phase reveal or the single-shot `show` fallback.
#[tauri::command]
pub fn is_visible(state: tauri::State<'_, AppState>) -> bool {
    state.asyar_visible.load(Ordering::Relaxed)
}

/// Hides the launcher window.
#[tauri::command]
pub fn hide(app_handle: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.asyar_visible.store(false, Ordering::Relaxed);
    state.mark_interaction();
    let window = app_handle
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    }
    crate::launcher::pop_to_root::on_launcher_hidden(&app_handle);
    Ok(())
}

/// Remember which window was frontmost before we steal focus, so it can be
/// restored later.
fn capture_previous_foreground(state: &tauri::State<'_, AppState>) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    {
        let wid = crate::window_management::linux::capture_active_window_id();
        let mut prev = state
            .linux_prev_window_id
            .lock()
            .map_err(|_| AppError::Lock)?;
        *prev = wid;
    }
    let _ = state;
    Ok(())
}

/// Locks or unlocks keyboard focus to prevent the window from losing focus.
#[tauri::command]
pub fn set_focus_lock(state: tauri::State<'_, AppState>, locked: bool) {
    state.focus_locked.store(locked, Ordering::Relaxed);
}

/// Mirrors `!isCompactIdle` from the launcher frontend so the panel resign
/// handler can tell whether the window is in a committed expanded state
/// (typed query, active extension view, active context chip, Show More)
/// that must survive hide/show without racing JS. TS owns the decision;
/// this command is a pure sink.
#[tauri::command]
pub fn set_launcher_keep_expanded(state: tauri::State<'_, AppState>, keep_expanded: bool) {
    state
        .launcher_keep_expanded
        .store(keep_expanded, Ordering::Relaxed);
}

/// No-op on Linux (resizes are never presentation-gated); kept so the
/// frontend can call it without platform guards.
#[tauri::command]
pub fn confirm_launcher_paint() {}

/// No-op on Linux (resizes are never presentation-gated); kept so the
/// frontend can call it without platform guards.
#[tauri::command]
pub fn cancel_launcher_resize() {}

/// Logs placement errors without aborting the reveal sequence.
fn log_placement_if_err<E: std::fmt::Display>(res: Result<(), E>, caller: &str) -> bool {
    match res {
        Ok(()) => true,
        Err(err) => {
            log::warn!("[{caller}] launcher placement failed (continuing reveal): {err}");
            false
        }
    }
}

/// Validates a launcher height value before it reaches platform window APIs.
/// Rejects NaN, Infinity, and values outside the allowed range.
fn validate_launcher_height(height: f64) -> Result<(), AppError> {
    if !height.is_finite() {
        return Err(AppError::Validation(format!(
            "launcher height must be finite, got {height}"
        )));
    }
    if !(50.0..=2000.0).contains(&height) {
        return Err(AppError::Validation(format!(
            "launcher height must be between 50 and 2000, got {height}"
        )));
    }
    Ok(())
}

fn scaled_height(base: f64, scale: f64) -> f64 {
    base * scale
}

/// Resizes the launcher window height, keeping the top edge pinned. The
/// `defer_until_next_ca_commit` and `after_next_presentation_update` gates
/// are ignored on Linux.
#[tauri::command]
pub fn set_launcher_height(
    app_handle: AppHandle,
    height: f64,
    defer_until_next_ca_commit: Option<bool>,
    after_next_presentation_update: Option<bool>,
) -> Result<(), AppError> {
    validate_launcher_height(height)?;
    let scale = app_handle.state::<AppState>().ui_scale();
    let height = scaled_height(height, scale);
    let window = app_handle
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;

    let _ = defer_until_next_ca_commit;
    let _ = after_next_presentation_update;
    use tauri::LogicalSize;
    let size = window
        .inner_size()
        .map_err(|e| AppError::Platform(e.to_string()))?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical_width = size.width as f64 / scale;
    let _ = window.set_size(tauri::Size::Logical(LogicalSize {
        width: logical_width,
        height,
    }));

    Ok(())
}

/// Applies a discrete launcher UI scale to both WebKit rendering and the
/// native window. The frontend continues to use the base 750 x 480 geometry;
/// scaling is confined to this native boundary.
#[tauri::command]
pub fn set_ui_scale(app_handle: AppHandle, scale: f64) -> Result<(), AppError> {
    const BASE_WIDTH: f64 = 750.0;
    const COMPACT_HEIGHT: f64 = 96.0;

    let scale = crate::bootstrap::windows::clamp_ui_scale(scale);
    app_handle.state::<AppState>().set_ui_scale(scale);

    let Some(window) = app_handle.get_webview_window(SPOTLIGHT_LABEL) else {
        log::warn!("[launcher] cannot apply UI scale: launcher window not found");
        return Ok(());
    };
    let base_height = if crate::bootstrap::windows::read_launch_view(&app_handle) == "compact" {
        COMPACT_HEIGHT
    } else {
        crate::launcher_placement::LAUNCHER_MAX_HEIGHT
    };

    if let Err(error) = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
        width: BASE_WIDTH * scale,
        height: base_height * scale,
    })) {
        log::warn!("[launcher] failed to resize for UI scale: {error}");
    }

    if let Err(error) = window.set_zoom(scale) {
        log::warn!("[launcher] failed to apply UI zoom: {error}");
    }
    if let Err(error) = crate::launcher_placement::service::apply(&app_handle) {
        log::warn!("[launcher] failed to reposition after UI scale change: {error}");
    }

    Ok(())
}

/// No-op on Linux — the command still succeeds so the JS caller does not
/// need platform guards.
#[tauri::command]
pub fn set_panel_appearance(app_handle: AppHandle, pref: String) -> Result<(), AppError> {
    let _ = (app_handle, pref);
    Ok(())
}

/// Shows the launcher panel using the same logic as the global-hotkey handler.
///
/// Intended for use from non-command code (e.g., `onboarding::window`) that
/// holds an `&AppHandle` but not a `State<'_, AppState>`. Accesses managed
/// state via `app.state::<AppState>()`.
pub fn show_launcher(app: &AppHandle) -> Result<(), AppError> {
    crate::launcher::pop_to_root::on_launcher_revealed(app);
    let state = app.state::<AppState>();
    state.asyar_visible.store(true, Ordering::Relaxed);
    // Intentionally omits capture_previous_foreground: the caller is
    // the onboarding completion flow, where the previously-focused
    // window is Asyar's own onboarding window. Recording it would
    // corrupt the snippet/paste target on the first launch.
    let window = app
        .get_webview_window(SPOTLIGHT_LABEL)
        .ok_or_else(|| AppError::NotFound("launcher window".to_string()))?;
    #[cfg(target_os = "linux")]
    let _ = crate::platform::linux::setup_spotlight_window(&window);
    if !log_placement_if_err(
        crate::launcher_placement::service::apply(app),
        "show_launcher",
    ) {
        let _ = window.center();
    }
    let _ = window.show();
    #[cfg(target_os = "linux")]
    let _ = crate::platform::linux::present_and_focus_spotlight_window(&window);
    #[cfg(target_os = "linux")]
    crate::mark_launcher_shown(&state);
    Ok(())
}

/// Cleanly exits the application.
#[tauri::command]
pub fn quit_app(app_handle: AppHandle) {
    app_handle.exit(0);
}

/// Shows or hides Asyar's main menu-bar / system tray icon.
#[tauri::command]
pub fn set_tray_icon_visible(app_handle: AppHandle, visible: bool) -> Result<(), AppError> {
    if let Some(tray) = app_handle.tray_by_id(crate::tray::TRAY_ID) {
        tray.set_visible(visible)
            .map_err(|e| AppError::Platform(format!("Failed to set tray visibility: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::windows::clamp_ui_scale;

    #[test]
    fn rejects_nan() {
        assert!(validate_launcher_height(f64::NAN).is_err());
    }

    #[test]
    fn rejects_positive_infinity() {
        assert!(validate_launcher_height(f64::INFINITY).is_err());
    }

    #[test]
    fn rejects_negative_infinity() {
        assert!(validate_launcher_height(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn rejects_zero() {
        assert!(validate_launcher_height(0.0).is_err());
    }

    #[test]
    fn rejects_negative() {
        assert!(validate_launcher_height(-10.0).is_err());
    }

    #[test]
    fn rejects_below_minimum() {
        assert!(validate_launcher_height(49.9).is_err());
    }

    #[test]
    fn rejects_above_maximum() {
        assert!(validate_launcher_height(2000.1).is_err());
    }

    #[test]
    fn accepts_compact_height() {
        assert!(validate_launcher_height(96.0).is_ok());
    }

    #[test]
    fn accepts_default_height() {
        assert!(validate_launcher_height(480.0).is_ok());
    }

    #[test]
    fn scaled_launcher_height_multiplies_by_scale() {
        assert!((scaled_height(96.0, 1.4) - 134.4).abs() < f64::EPSILON * 134.4);
        assert_eq!(scaled_height(480.0, 1.0), 480.0);
    }

    #[test]
    fn set_ui_scale_clamps_out_of_range() {
        assert_eq!(clamp_ui_scale(0.3), 1.0);
        assert_eq!(clamp_ui_scale(1.2), 1.2);
        assert_eq!(clamp_ui_scale(5.0), 1.2);
    }

    #[test]
    fn accepts_minimum_boundary() {
        assert!(validate_launcher_height(50.0).is_ok());
    }

    #[test]
    fn accepts_maximum_boundary() {
        assert!(validate_launcher_height(2000.0).is_ok());
    }

    #[test]
    fn error_is_validation_variant() {
        let err = validate_launcher_height(f64::NAN).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn log_placement_if_err_returns_true_on_ok_and_false_on_err() {
        assert!(log_placement_if_err::<&str>(Ok(()), "test"));
        assert!(!log_placement_if_err(
            Err("wayland monitor unavailable"),
            "test"
        ));
    }

    #[test]
    fn reveal_paths_use_fail_soft_placement_and_linux_show_launcher_marks_shown() {
        let source = include_str!("app.rs").split("#[cfg(test)]").next().unwrap();
        assert!(
            !source.contains("crate::launcher_placement::service::apply(&app_handle)?"),
            "reveal paths must not abort on placement errors with `?`"
        );
        assert!(
            !source.contains("crate::launcher_placement::service::apply(app)?"),
            "show_launcher must not abort on placement errors with `?`"
        );

        let commit_show_body = source
            .split("pub fn commit_show")
            .nth(1)
            .expect("commit_show function")
            .split("pub fn show(")
            .next()
            .expect("commit_show body");
        assert!(
            commit_show_body.contains("window.center()"),
            "commit_show must fall back to window.center() if placement fails after off-screen prepare_show"
        );

        let show_body = source
            .split("pub fn show(")
            .nth(1)
            .expect("show function")
            .split("pub fn is_visible")
            .next()
            .expect("show body");
        assert!(
            show_body.contains("window.center()"),
            "show must fall back to window.center() if placement fails"
        );

        let show_launcher_body = source
            .split("pub fn show_launcher")
            .nth(1)
            .expect("show_launcher function")
            .split("pub fn quit_app")
            .next()
            .expect("show_launcher body");
        assert!(
            show_launcher_body.contains("window.center()"),
            "show_launcher must fall back to window.center() if placement fails"
        );
        assert!(
            show_launcher_body.contains("setup_spotlight_window"),
            "show_launcher must call setup_spotlight_window on Linux"
        );
        assert!(
            show_launcher_body.contains("mark_launcher_shown"),
            "show_launcher must call mark_launcher_shown on Linux to prevent spurious Wayland blur hide"
        );
    }
}
