//! Application "where is it / run it elevated" helpers for the ⌘K action panel.
//!
//! - **Reveal** opens the application's location.
//! - **Run as administrator** has no safe Linux equivalent, so it returns a
//!   platform error and the frontend hides the action.
//!
//! The path validators are plain functions so they are unit-tested on any OS.

use crate::error::AppError;
use std::path::{Path, PathBuf};

const APPS_FOLDER_PREFIX: &str = "shell:AppsFolder\\";

fn is_store_app(path: &str) -> bool {
    path.starts_with(APPS_FOLDER_PREFIX)
}

/// Validates an indexed application path before it is revealed.
pub(crate) fn validate_reveal_path(path: &str) -> Result<PathBuf, AppError> {
    if path.trim().is_empty() {
        return Err(AppError::Validation("path must be non-empty".to_string()));
    }
    if is_store_app(path) {
        return Err(AppError::Platform(
            "Microsoft Store apps have no file location".to_string(),
        ));
    }
    let raw = Path::new(path);
    if !raw.is_absolute() {
        return Err(AppError::Validation(format!(
            "path must be absolute: {path}"
        )));
    }
    if !raw.exists() {
        return Err(AppError::NotFound(format!(
            "application does not exist: {path}"
        )));
    }
    Ok(raw.to_path_buf())
}

/// The path to reveal for an application: the validated path itself.
pub(crate) fn reveal_target(path: &str) -> Result<PathBuf, AppError> {
    validate_reveal_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(dir: &TempDir, name: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, "").unwrap();
        p.to_string_lossy().to_string()
    }

    #[test]
    fn reveal_rejects_empty_and_relative_paths() {
        assert!(matches!(
            validate_reveal_path("  "),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_reveal_path("relative/app.app"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn reveal_rejects_store_apps_with_a_platform_error() {
        let err =
            validate_reveal_path(r"shell:AppsFolder\Microsoft.Foo_8wekyb3d8bbwe!App").unwrap_err();
        assert!(matches!(err, AppError::Platform(_)), "got: {err:?}");
    }

    #[test]
    fn reveal_reports_missing_applications() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("gone.app");
        let err = validate_reveal_path(missing.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "got: {err:?}");
    }

    #[test]
    fn reveal_accepts_an_existing_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let path = touch(&tmp, "Zoom.lnk");
        assert_eq!(validate_reveal_path(&path).unwrap(), PathBuf::from(&path));
    }

    #[test]
    fn reveal_target_returns_the_path_for_non_shortcuts() {
        let tmp = TempDir::new().unwrap();
        let path = touch(&tmp, "Tool.exe");
        assert_eq!(reveal_target(&path).unwrap(), PathBuf::from(&path));
    }
}
