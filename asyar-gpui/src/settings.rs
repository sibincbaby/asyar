//! Reads the settings the Tauri app writes, so both frontends agree.
//!
//! Read-only: the Tauri app's Settings window stays the editor until the GPUI
//! shell has its own. Loaded on every summon, which picks up changes without a
//! file watcher (one small JSON read and one indexed SQLite lookup).

use std::path::{Path, PathBuf};

use asyar_calculator::locale::{self, NumberFormat};
use gpui::{Hsla, Window, WindowAppearance, hsla};
use rusqlite::{Connection, OpenFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: Theme,
    pub number_format: NumberFormat,
    pub preferred_currency: String,
    /// How old currency rates may get before a refresh, in hours.
    pub rates_ttl_hours: f64,
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(data_dir().as_deref())
    }

    fn load_from(dir: Option<&Path>) -> Self {
        let settings: Option<serde_json::Value> = dir
            .and_then(|dir| std::fs::read(dir.join("settings.dat")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let theme = match settings
            .as_ref()
            .and_then(|s| s.pointer("/settings/appearance/theme"))
            .and_then(|v| v.as_str())
        {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            _ => Theme::System,
        };
        let pref = |key| dir.and_then(|dir| calculator_pref(&dir.join("asyar_data.db"), key));
        Self {
            theme,
            // "auto" (the default) follows the system locale, as in the Tauri app.
            number_format: pref("numberFormat")
                .and_then(|v| locale::from_preference(&v))
                .unwrap_or_else(locale::detect),
            preferred_currency: pref("preferredCurrency")
                .map(|v| v.trim().to_uppercase())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "USD".to_owned()),
            rates_ttl_hours: pref("refreshInterval")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|h| *h > 0.0)
                .unwrap_or(asyar_calculator::currency::DEFAULT_TTL_HOURS),
        }
    }

    pub fn is_dark(&self, window: &Window) -> bool {
        match self.theme {
            Theme::Light => false,
            Theme::Dark => true,
            Theme::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }
}

/// Glass palette: translucent tint over a blurred backdrop, a bright rim and
/// a soft, colourless selection, after macOS's Liquid Glass look.
pub struct Glass {
    pub tint: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub selection: Hsla,
    pub divider: Hsla,
    pub rim: Hsla,
    pub sheen: Hsla,
}

impl Glass {
    /// `frosted`: something blurred sits behind the tint, so it can be thin.
    pub fn new(dark: bool, frosted: bool) -> Self {
        let (ink, paper) = if dark { (1.0, 0.09) } else { (0.08, 1.0) };
        let tint_alpha = match (frosted, dark) {
            (true, false) => 0.55,
            (true, true) => 0.5,
            (false, _) => 0.9,
        };
        Self {
            tint: hsla(0., 0., paper, tint_alpha),
            text: hsla(0., 0., ink, 0.92),
            muted: hsla(0., 0., ink, 0.55),
            selection: hsla(0., 0., ink, if dark { 0.14 } else { 0.08 }),
            divider: hsla(0., 0., ink, if dark { 0.10 } else { 0.07 }),
            rim: hsla(0., 0., 1., if dark { 0.18 } else { 0.65 }),
            sheen: hsla(0., 0., 1., if dark { 0.06 } else { 0.28 }),
        }
    }
}

/// `$XDG_DATA_HOME/org.asyar` (Tauri's app data dir for identifier `org.asyar`).
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map(|dir| dir.join("org.asyar"))
}

/// An extension-level (empty `command_id`), unencrypted calculator preference.
fn calculator_pref(db: &Path, key: &str) -> Option<String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.query_row(
        "SELECT value FROM extension_preferences
         WHERE extension_id = 'calculator' AND command_id = '' AND key = ?1 AND is_encrypted = 0",
        [key],
        |row| row.get(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_theme_and_calculator_preferences() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.dat"),
            r#"{"settings":{"appearance":{"theme":"dark"}}}"#,
        )
        .unwrap();
        let conn = Connection::open(dir.path().join("asyar_data.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE extension_preferences (extension_id TEXT, command_id TEXT, key TEXT,
                 value TEXT, is_encrypted INTEGER);
             INSERT INTO extension_preferences VALUES
                 ('calculator', '', 'numberFormat', 'comma', 0),
                 ('calculator', '', 'preferredCurrency', 'eur', 0),
                 ('calculator', '', 'refreshInterval', '12', 0);",
        )
        .unwrap();

        let s = Settings::load_from(Some(dir.path()));
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.number_format, NumberFormat::Comma);
        assert_eq!(s.preferred_currency, "EUR");
        assert_eq!(s.rates_ttl_hours, 12.0);
    }

    #[test]
    fn missing_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load_from(Some(dir.path()));
        assert_eq!(s.theme, Theme::System);
        assert_eq!(s.number_format, locale::detect());
        assert_eq!(s.preferred_currency, "USD");
        assert_eq!(s.rates_ttl_hours, 6.0);
    }
}
