//! Tauri command wrappers for the window management feature.
//!
//! Each command requires the `window:manage` permission and dispatches to the
//! appropriate platform implementation.

use crate::error::AppError;
use crate::permissions::ExtensionPermissionRegistry;
use crate::window_management::types::{
    validate_bounds_update, AppWindowInfo, WindowBounds, WindowBoundsUpdate,
};
use crate::AppState;

/// Returns the bounds of the frontmost OS application window.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_get_bounds(
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
) -> Result<WindowBounds, AppError> {
    permissions.check(&extension_id, "window:manage")?;

    let wid = *state
        .linux_prev_window_id
        .lock()
        .map_err(|_| AppError::Lock)?;
    crate::window_management::linux::get_window_bounds(wid)
}

/// Updates the bounds of the frontmost OS application window.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_set_bounds(
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), AppError> {
    permissions.check(&extension_id, "window:manage")?;
    let update = WindowBoundsUpdate {
        x,
        y,
        width,
        height,
    };
    validate_bounds_update(&update)?;

    let wid = *state
        .linux_prev_window_id
        .lock()
        .map_err(|_| AppError::Lock)?;
    crate::window_management::linux::set_window_bounds(wid, &update)
}

/// Toggles the fullscreen state of the frontmost OS application window.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_set_fullscreen(
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
    enable: bool,
) -> Result<(), AppError> {
    permissions.check(&extension_id, "window:manage")?;

    let wid = *state
        .linux_prev_window_id
        .lock()
        .map_err(|_| AppError::Lock)?;
    crate::window_management::linux::set_window_fullscreen(wid, enable)
}

/// Returns all available monitors with their logical bounds.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub fn window_management_get_monitors(
    app: tauri::AppHandle,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
) -> Result<Vec<WindowBounds>, AppError> {
    permissions.check(&extension_id, "window:manage")?;

    let monitors = app
        .available_monitors()
        .map_err(|e| AppError::Platform(format!("available_monitors: {e}")))?;

    let mut result = Vec::new();
    for m in monitors {
        let scale = m.scale_factor();
        let size = m.size().to_logical::<f64>(scale);
        let pos = m.position().to_logical::<f64>(scale);
        result.push(WindowBounds {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        });
    }

    Ok(result)
}

/// Applies a preset layout to the frontmost OS application window relative to the monitor it is on.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub fn window_management_apply_preset(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
    preset_id: String,
) -> Result<(), AppError> {
    permissions.check(&extension_id, "window:manage")?;

    let current_bounds = {
        #[cfg(target_os = "linux")]
        {
            let wid = *state
                .linux_prev_window_id
                .lock()
                .map_err(|_| AppError::Lock)?;
            crate::window_management::linux::get_window_bounds(wid)?
        }
    };

    let monitor_bounds: Vec<WindowBounds> = app
        .available_monitors()
        .map_err(|e| AppError::Platform(format!("available_monitors: {e}")))?
        .into_iter()
        .map(|m| {
            let scale = m.scale_factor();
            let size = m.size().to_logical::<f64>(scale);
            let pos = m.position().to_logical::<f64>(scale);
            WindowBounds {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect();

    let action = crate::window_management::layout::calculate_preset_action(
        &preset_id,
        &current_bounds,
        &monitor_bounds,
    )?;

    match action {
        crate::window_management::layout::PresetAction::Fullscreen(enable) => {
            #[cfg(target_os = "linux")]
            {
                let wid = *state
                    .linux_prev_window_id
                    .lock()
                    .map_err(|_| AppError::Lock)?;
                crate::window_management::linux::set_window_fullscreen(wid, enable)?;
            }
        }
        crate::window_management::layout::PresetAction::Bounds(update) => {
            #[cfg(target_os = "linux")]
            {
                let wid = *state
                    .linux_prev_window_id
                    .lock()
                    .map_err(|_| AppError::Lock)?;
                crate::window_management::linux::set_window_bounds(wid, &update)?;
            }
        }
        crate::window_management::layout::PresetAction::Noop => {}
    }

    Ok(())
}

/// Returns all open OS application windows across displays.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_list_windows(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
) -> Result<Vec<AppWindowInfo>, AppError> {
    permissions.check(&extension_id, "window:manage")?;

    #[cfg(target_os = "linux")]
    return crate::window_management::linux::list_windows(&app);
}

/// Brings the target application window to the foreground and hides the launcher.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_focus_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
    id: String,
) -> Result<(), AppError> {
    permissions.check(&extension_id, "window:manage")?;

    #[cfg(target_os = "linux")]
    {
        crate::window_management::linux::focus_window(&id)?;
        let _ = crate::commands::app::hide(app, state);
        Ok(())
    }
}

/// Closes the target application window.
/// Requires 'window:manage' permission.
#[tauri::command]
#[allow(unused_variables)]
pub async fn window_management_close_window(
    permissions: tauri::State<'_, ExtensionPermissionRegistry>,
    extension_id: Option<String>,
    id: String,
) -> Result<(), AppError> {
    permissions.check(&extension_id, "window:manage")?;

    #[cfg(target_os = "linux")]
    {
        crate::window_management::linux::close_window(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::ExtensionPermissionRegistry;

    fn make_registry_with(id: &str, perms: &[&str]) -> ExtensionPermissionRegistry {
        let reg = ExtensionPermissionRegistry::new();
        let mut inner = reg.inner.lock().unwrap();
        inner.insert(
            id.to_string(),
            perms.iter().map(|s| s.to_string()).collect(),
        );
        drop(inner);
        reg
    }

    #[test]
    fn permission_check_blocks_missing_permission() {
        let reg = make_registry_with("ext-1", &["clipboard:read"]);
        let result = reg.check(&Some("ext-1".to_string()), "window:manage");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Permission(_)));
    }

    #[test]
    fn permission_check_allows_with_correct_permission() {
        let reg = make_registry_with("ext-1", &["window:manage"]);
        assert!(reg
            .check(&Some("ext-1".to_string()), "window:manage")
            .is_ok());
    }

    #[test]
    fn validate_bounds_update_rejects_all_none_in_command() {
        let update = WindowBoundsUpdate {
            x: None,
            y: None,
            width: None,
            height: None,
        };
        assert!(validate_bounds_update(&update).is_err());
    }
}
