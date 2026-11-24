//! Installed-application discovery from XDG `.desktop` files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::desktop_entry::{DesktopEntry, current_desktop_environments};

pub struct AppEntry {
    pub name: String,
    /// Empty for synthetic benchmark rows, which cannot be launched.
    pub path: PathBuf,
    /// Raw `Icon=` value; resolve to a file with [`resolve_icons`].
    pub icon_name: Option<String>,
}

/// `$XDG_DATA_HOME/applications` first, then each `$XDG_DATA_DIRS` entry.
fn app_dirs() -> Vec<PathBuf> {
    let non_empty = |key| std::env::var(key).ok().filter(|v| !v.trim().is_empty());
    let data_home = non_empty("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let data_dirs =
        non_empty("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    data_home
        .into_iter()
        .chain(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        )
        .map(|dir| dir.join("applications"))
        .collect()
}

pub fn scan() -> Vec<AppEntry> {
    scan_roots(
        &app_dirs(),
        &current_desktop_environments(),
        &crate::locale::detect_locale(),
    )
}

/// Resolves each `Icon=` value to a PNG/SVG file. Slow (hundreds of ms over a
/// large theme like Papirus), so callers run it off the UI thread.
pub fn resolve_icons(names: &[Option<String>]) -> Vec<Option<PathBuf>> {
    let theme = icon_theme();
    names
        .iter()
        .map(|name| name.as_deref().and_then(|name| resolve_icon(name, &theme)))
        .collect()
}

/// The user's icon theme: GTK's settings.ini, else the desktop's gsettings
/// key (Cinnamon and GNOME keep it only there), else the base `hicolor`.
fn icon_theme() -> String {
    if let Some(theme) = freedesktop_icons::default_theme_gtk() {
        return theme;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let schema = if desktop.contains("Cinnamon") {
        "org.cinnamon.desktop.interface"
    } else {
        "org.gnome.desktop.interface"
    };
    Command::new("gsettings")
        .args(["get", schema, "icon-theme"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .trim_matches('\'')
                .to_owned()
        })
        .filter(|theme| !theme.is_empty())
        .unwrap_or_else(|| "hicolor".to_owned())
}

/// `Icon=` is either an absolute file or a theme icon name (sometimes with a
/// stray extension, which the spec forbids but real entries use).
fn resolve_icon(name: &str, theme: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let name = name
        .strip_suffix(".png")
        .or_else(|| name.strip_suffix(".svg"))
        .or_else(|| name.strip_suffix(".xpm"))
        .unwrap_or(name);
    freedesktop_icons::lookup(name)
        .with_size(48)
        .with_theme(theme)
        .with_cache()
        .find()
        // GPUI renders PNG and SVG, not XPM.
        .filter(|found| found.extension().is_some_and(|ext| ext != "xpm"))
}

fn scan_roots(roots: &[PathBuf], desktops: &[String], locale: &str) -> Vec<AppEntry> {
    let desktops: Vec<&str> = desktops.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for root in roots {
        walk(root, root, &mut |id, path| {
            // Desktop-file IDs shadow by precedence: the first root wins, and a
            // hidden entry there still masks the same ID further down.
            if !seen.insert(id) {
                return;
            }
            if let Some(entry) = DesktopEntry::from_file(path) {
                let name = entry.display_name(Some(locale));
                if entry.is_visible(&desktops) && !name.is_empty() {
                    apps.push(AppEntry {
                        name,
                        path: path.to_path_buf(),
                        icon_name: entry.icon.clone(),
                    });
                }
            }
        });
    }
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    apps
}

/// Calls `visit` with each `.desktop` file's ID (`kde/foo.desktop` → `kde-foo.desktop`).
fn walk(root: &Path, dir: &Path, visit: &mut impl FnMut(String, &Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, visit);
        } else if path.extension().is_some_and(|ext| ext == "desktop")
            && let Ok(rel) = path.strip_prefix(root)
        {
            visit(rel.to_string_lossy().replace('/', "-"), &path);
        }
    }
}

pub fn launch(app: &AppEntry) {
    if app.path.as_os_str().is_empty() {
        return;
    }
    // Same as the Tauri app (application/service.rs): the host's `gio launch`
    // handles Exec= field codes, D-Bus activation and startup notification.
    gio("launch", app.path.as_os_str());
}

/// Opens a web link in the default browser. Callers pass `http(s)` URLs only.
pub fn open_url(url: &str) {
    gio("open", url.as_ref());
}

fn gio(verb: &str, target: &std::ffi::OsStr) {
    match Command::new("gio")
        .arg(verb)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Reap in the background so the long-lived daemon collects no zombies.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(error) => eprintln!("asyar-gpui: gio {verb} {target:?} failed: {error}"),
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(name),
            format!("[Desktop Entry]\nType=Application\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn first_root_shadows_later_roots_and_hidden_entries_mask() {
        let tmp = tempfile::tempdir().unwrap();
        let (user, system) = (tmp.path().join("user"), tmp.path().join("system"));
        write(&user, "editor.desktop", "Name=User Editor\nExec=true");
        write(&system, "editor.desktop", "Name=System Editor\nExec=true");
        write(&user, "junk.desktop", "Name=Junk\nExec=true\nHidden=true");
        write(&system, "junk.desktop", "Name=Junk\nExec=true");
        write(&system.join("kde"), "term.desktop", "Name=Term\nExec=true");

        let names: Vec<String> = scan_roots(&[user, system], &[], "en-US")
            .into_iter()
            .map(|a| a.name)
            .collect();

        assert_eq!(names, ["Term", "User Editor"]);
    }

    #[test]
    fn absolute_icon_paths_resolve_only_when_the_file_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let png = tmp.path().join("app.png");
        std::fs::write(&png, b"png").unwrap();
        assert_eq!(resolve_icon(png.to_str().unwrap(), "hicolor"), Some(png));
        let missing = tmp.path().join("gone.png");
        assert_eq!(resolve_icon(missing.to_str().unwrap(), "hicolor"), None);
    }
}
