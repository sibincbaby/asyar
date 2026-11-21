use std::path::{Path, PathBuf};

pub struct ChromiumVariant {
    pub id: &'static str,
    /// Linux sub-path under `~/.config`
    pub linux_subpath: &'static str,
}

pub fn chromium_variants() -> &'static [ChromiumVariant] {
    &[
        ChromiumVariant {
            id: "chrome",
            linux_subpath: "google-chrome",
        },
        ChromiumVariant {
            id: "brave",
            linux_subpath: "BraveSoftware/Brave-Browser",
        },
        ChromiumVariant {
            id: "arc",
            linux_subpath: "Arc/User Data",
        },
        ChromiumVariant {
            id: "edge",
            linux_subpath: "microsoft-edge",
        },
        ChromiumVariant {
            id: "vivaldi",
            linux_subpath: "vivaldi",
        },
    ]
}

pub struct FirefoxVariant {
    pub id: &'static str,
    pub linux_subpath: &'static str,
}

pub fn firefox_variants() -> &'static [FirefoxVariant] {
    &[
        FirefoxVariant {
            id: "firefox",
            // Linux Firefox lives at ~/.mozilla/firefox (relative to home, NOT under
            // ~/.config). No `../` — a leading `../` would climb above home AND, in
            // tempdir-scoped tests, escape the sandbox to read the real filesystem.
            linux_subpath: ".mozilla/firefox",
        },
        FirefoxVariant {
            id: "librewolf",
            linux_subpath: ".librewolf",
        },
    ]
}

pub fn chromium_user_data_root(home: &Path, variant_id: &str) -> PathBuf {
    let variant = chromium_variants()
        .iter()
        .find(|v| v.id == variant_id)
        .expect("unknown chromium variant");
    #[cfg(target_os = "linux")]
    {
        home.join(".config").join(variant.linux_subpath)
    }
}

pub fn firefox_profiles_dir(home: &Path, variant_id: &str) -> PathBuf {
    let variant = firefox_variants()
        .iter()
        .find(|v| v.id == variant_id)
        .expect("unknown firefox variant");
    #[cfg(target_os = "linux")]
    {
        home.join(variant.linux_subpath).join("Profiles")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_variants_listed() {
        let variants = chromium_variants();
        assert!(variants.iter().any(|v| v.id == "chrome"));
        assert!(variants.iter().any(|v| v.id == "brave"));
        assert!(variants.iter().any(|v| v.id == "arc"));
        assert!(variants.iter().any(|v| v.id == "edge"));
        assert!(variants.iter().any(|v| v.id == "vivaldi"));
    }

    #[test]
    fn firefox_variants_listed() {
        let variants = firefox_variants();
        assert!(variants.iter().any(|v| v.id == "firefox"));
        assert!(variants.iter().any(|v| v.id == "librewolf"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn chromium_user_data_root_linux_chrome() {
        let home = std::path::Path::new("/home/test");
        let root = chromium_user_data_root(home, "chrome");
        assert_eq!(
            root,
            std::path::Path::new("/home/test/.config/google-chrome")
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn firefox_profiles_dir_linux_stays_within_home() {
        // Regression guard for the `../` escape bug: the resolved path MUST stay
        // under home (no climbing above it), so tempdir-scoped scans stay hermetic.
        let home = std::path::Path::new("/home/test");
        let dir = firefox_profiles_dir(home, "firefox");
        assert!(
            dir.starts_with("/home/test"),
            "firefox dir escaped home: {dir:?}"
        );
        assert!(
            !dir.to_string_lossy().contains(".."),
            "firefox dir contains ..: {dir:?}"
        );
    }
}
