#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("ACCESSIBILITY_UNAVAILABLE: {0}")]
    AccessibilityUnavailable(String),
    #[error("CLIPBOARD_RESTORE_FAILED: {0}")]
    ClipboardRestoreFailed(String),
    #[error("OPERATION_FAILED: {0}")]
    OperationFailed(String),
}

impl From<SelectionError> for crate::error::AppError {
    fn from(e: SelectionError) -> Self {
        crate::error::AppError::Platform(e.to_string())
    }
}
