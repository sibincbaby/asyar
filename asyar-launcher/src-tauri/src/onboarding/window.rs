//! Onboarding window lifecycle helpers.
//!
//! Creates, shows, and closes the dedicated `onboarding` webview window.
//! `open_if_needed` is called from `setup_app` (Task 7 owns that call site).

use crate::error::AppError;
use tauri::{AppHandle, Manager};

pub const WINDOW_LABEL: &str = "onboarding";
const WINDOW_URL: &str = "/onboarding";
// Roomier than the launcher panel: the guided tour is a two-column stage —
// instruction/setup content on one side, a vibrant tilted feature image on the
// other — so it needs space to breathe.
const WINDOW_WIDTH: f64 = 1000.0;
const WINDOW_HEIGHT: f64 = 700.0;

/// Open the onboarding window (creates if it doesn't exist; focuses if it does).
pub fn open(app: &AppHandle) -> Result<(), AppError> {
    if let Some(existing) = app.get_webview_window(WINDOW_LABEL) {
        existing
            .show()
            .map_err(|e| AppError::Other(format!("show onboarding: {e}")))?;
        existing
            .set_focus()
            .map_err(|e| AppError::Other(format!("focus onboarding: {e}")))?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(app, WINDOW_LABEL, tauri::WebviewUrl::App(WINDOW_URL.into()))
        .title(crate::locale::app_native_text(
            app,
            crate::locale::NativeText::OnboardingTitle,
        ))
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(false)
        .center()
        // Not always-on-top: the tour guides users to the REAL launcher, which must
        // overlay this window when summoned. Onboarding stays a passive guide layer.
        .always_on_top(false)
        .decorations(false)
        .transparent(true)
        // No drop shadow: the transparent window casts a large soft shadow onto
        // whatever sits behind it, which reads as a translucent "footer" band over
        // the content behind the floating window. The rounded card reads as a
        // window without it.
        .shadow(false)
        .visible(true)
        .focused(true)
        .build()
        .map_err(|e| AppError::Other(format!("create onboarding: {e}")))?;

    Ok(())
}

/// Open the onboarding window only if `settings.onboarding.completed != true`.
/// Called from `setup_app` (Task 7 owns the call site).
pub fn open_if_needed(app: &AppHandle) -> Result<(), AppError> {
    if crate::onboarding::persistence::read_onboarding_completed(app) {
        return Ok(());
    }
    open(app)
}

/// Close the onboarding window if open. No-op if not.
pub fn close(app: &AppHandle) -> Result<(), AppError> {
    if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
        w.close()
            .map_err(|e| AppError::Other(format!("close onboarding: {e}")))?;
    }
    Ok(())
}

/// Show the launcher panel. Mirrors whatever code path the global hotkey uses.
pub fn show_launcher_panel(app: &AppHandle) -> Result<(), AppError> {
    crate::commands::app::show_launcher(app)
}
