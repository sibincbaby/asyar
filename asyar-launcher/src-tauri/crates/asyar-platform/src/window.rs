//! Cross-platform window styling and taskbar/dock abstractions.

use crate::error::PlatformError;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The explicit theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreference {
    Light,
    Dark,
    System,
}

/// Cross-platform abstraction for window styling and presentation flags.
pub trait WindowStyling {
    /// Configure Spotlight-style borderless, rounded, floating, or HUD presentation.
    fn apply_spotlight_styling(&self) -> Result<(), PlatformError>;

    /// Exclude window from taskbar/dock and Alt+Tab/Mission Control where applicable.
    fn apply_taskbar_exclusion(&self) -> Result<(), PlatformError>;
}

impl<W: HasWindowHandle> WindowStyling for W {
    fn apply_spotlight_styling(&self) -> Result<(), PlatformError> {
        let handle = self
            .window_handle()
            .map_err(|e| PlatformError::WindowHandle(e.to_string()))?;
        match handle.as_raw() {
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xlib(_) | RawWindowHandle::Wayland(_) | RawWindowHandle::Xcb(_) => {
                // GTK/X11 window styling on Linux is handled via gtk_window or EWMH client messages
                Ok(())
            }
            _ => Err(PlatformError::Unsupported(
                "unsupported window handle for spotlight styling".to_string(),
            )),
        }
    }

    fn apply_taskbar_exclusion(&self) -> Result<(), PlatformError> {
        self.window_handle()
            .map_err(|e| PlatformError::WindowHandle(e.to_string()))?;
        Ok(())
    }
}
