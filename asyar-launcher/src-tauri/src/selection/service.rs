use crate::selection::error::SelectionError;
use crate::selection::platform;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

static SELECTION_MUTEX: Mutex<()> = Mutex::const_new(());

pub async fn get_selected_text() -> Result<Option<String>, SelectionError> {
    let _lock = SELECTION_MUTEX.lock().await;

    // Step 1 — fast path (no clipboard touch)
    #[cfg(target_os = "linux")]
    {
        if let Some(text) = platform::get_selected_text_via_a11y().await {
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
    }

    // Step 2 — clipboard trick fallback
    let guard = platform::ClipboardGuard::new();

    // Capture the before-marker
    let before = platform::clipboard_change_marker();

    // Post the copy chord
    crate::platform::input::post_copy_chord_to_frontmost();

    // Poll up to 250ms in 10ms increments
    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if platform::clipboard_change_marker() != before {
            break; // clipboard changed — read it now
        }
    }

    // If marker didn't change, we assume no selection
    if platform::clipboard_change_marker() == before {
        log::info!("[selection] clipboard fallback: no change after copy chord");
        return Ok(None);
    }

    // Read new contents
    use arboard::Clipboard;
    let text = Clipboard::new()
        .map_err(|e| SelectionError::OperationFailed(e.to_string()))?
        .get_text()
        .map_err(|e| SelectionError::OperationFailed(e.to_string()))?;

    // Guard DROP handles restore
    drop(guard);

    Ok(Some(text))
}

pub async fn get_selected_finder_items(
    _app: &tauri::AppHandle,
) -> Result<Vec<String>, SelectionError> {
    let _lock = SELECTION_MUTEX.lock().await;

    // Primarily platform-specific logic
    platform::get_selected_finder_items()
}
