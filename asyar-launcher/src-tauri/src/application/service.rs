use crate::error::AppError;
use crate::platform::ExtractedIcon;
use crate::search_engine::models::{build_app_id, Application, SearchableItem};
use crate::search_engine::SearchState;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FrontmostApplication {
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: Option<String>,
    pub window_title: Option<String>,
}

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub added: u32,
    pub removed: u32,
    pub total: u32,
}

#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
enum ApplicationLaunchRoute {
    GioDesktopEntry,
    Opener,
}

#[cfg(target_os = "linux")]
fn application_launch_route(path: &Path) -> ApplicationLaunchRoute {
    if path
        .extension()
        .is_some_and(|extension| extension == "desktop")
    {
        ApplicationLaunchRoute::GioDesktopEntry
    } else {
        ApplicationLaunchRoute::Opener
    }
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("gio");
    command.arg("launch").arg(path);
    crate::platform::linux::sanitize_command(&mut command);
    command
}

#[cfg(target_os = "linux")]
fn launch_linux_desktop_entry(path: &Path) -> Result<(), AppError> {
    if !path.is_file() {
        return Err(AppError::Platform(format!(
            "Failed to load desktop entry '{}': missing desktop entry",
            path.display()
        )));
    }

    let status = linux_desktop_entry_command(path)
        .status()
        .map_err(|error| {
            AppError::Platform(format!(
                "Failed to launch desktop entry '{}': {}",
                path.display(),
                error
            ))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Platform(format!(
            "Failed to launch desktop entry '{}': gio exited with {}",
            path.display(),
            status
        )))
    }
}

/// Launches an application using the platform-native mechanism for its path.
pub fn open_application_path<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    path: String,
) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    if application_launch_route(Path::new(&path)) == ApplicationLaunchRoute::GioDesktopEntry {
        return launch_linux_desktop_entry(Path::new(&path));
    }

    #[cfg(target_os = "linux")]
    {
        use gio::prelude::*;
        let launch_context = crate::platform::linux::create_sanitized_app_launch_context();
        let file = gio::File::for_path(&path);
        let uri = file.uri();
        if gio::AppInfo::launch_default_for_uri(uri.as_str(), Some(&launch_context)).is_ok() {
            return Ok(());
        }
    }

    use tauri_plugin_opener::OpenerExt;
    app_handle
        .opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| AppError::Platform(format!("Failed to open path '{}': {}", path, e)))
}

/// Retrieves metadata about the currently focused application.
pub fn get_frontmost_application() -> Result<Option<FrontmostApplication>, AppError> {
    #[cfg(target_os = "linux")]
    {
        if let Some(meta) = crate::platform::linux::get_frontmost_application_metadata() {
            return Ok(Some(FrontmostApplication {
                name: meta.name,
                bundle_id: meta.bundle_id,
                path: meta.path,
                window_title: meta.window_title,
            }));
        }
    }

    Ok(None)
}

/// Scans for applications in default and extra paths, diffs against the search index,
/// and updates the search state.
pub fn sync_application_index<R: tauri::Runtime>(
    app: &AppHandle<R>,
    search_state: &SearchState,
    extra_paths: Vec<PathBuf>,
) -> Result<SyncResult, AppError> {
    // 1. Scan applications
    let mut scanner = AppScanner::new();
    scanner.scan_all(extra_paths)?;

    let icon_cache_dir = icon_cache_dir(app);

    // 2. Build current app set
    let mut current_apps: HashMap<String, Application> = HashMap::new();
    for path_str in &scanner.paths {
        let path = Path::new(path_str);
        // Id keyed on the file name, never the display name — see
        // `bundle_file_name`. Keeps existing ids byte-identical.
        let full_app_id = build_app_id(&bundle_file_name(path), path_str);

        current_apps.insert(
            full_app_id.clone(),
            Application {
                id: full_app_id,
                name: display_name(path),
                path: path_str.clone(),
                usage_count: 0,
                icon: extract_app_icon(path_str, &icon_cache_dir),
                last_used_at: None,
                bundle_id: extract_bundle_id(Path::new(path_str)),
                exe_name: extract_exe_name(path),
            },
        );
    }

    // 3. Get currently indexed app_ IDs
    let indexed_ids: Vec<String> = {
        let items = search_state
            .items
            .read()
            .map_err(|e| AppError::Other(e.to_string()))?;
        items
            .iter()
            .filter_map(|item| {
                let id = item.id();
                if id.starts_with("app_") {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .collect()
    };
    let indexed_set: HashSet<&str> = indexed_ids.iter().map(|s| s.as_str()).collect();
    let current_set: HashSet<&str> = current_apps.keys().map(|s| s.as_str()).collect();

    // 4. Diff
    let to_add: Vec<String> = current_set
        .difference(&indexed_set)
        .map(|s| s.to_string())
        .collect();
    let to_remove: Vec<String> = indexed_set
        .difference(&current_set)
        .map(|s| s.to_string())
        .collect();

    let added = to_add.len() as u32;
    let removed = to_remove.len() as u32;

    // 5. Update SearchState
    //
    // The write lock is taken unconditionally: an app can need updating without
    // being added or removed, and `refresh_display_names` below is the only
    // thing that catches it.
    let renamed = {
        let mut items = search_state
            .items
            .write()
            .map_err(|e| AppError::Other(e.to_string()))?;

        if !to_remove.is_empty() {
            let remove_set: HashSet<String> = to_remove.into_iter().collect();
            items.retain(|item| !remove_set.contains(item.id()));
        }

        let renamed = refresh_display_names(&mut items, &current_apps);

        for id in to_add {
            if let Some(app) = current_apps.remove(&id) {
                items.push(SearchableItem::Application(app));
            }
        }

        renamed
    };

    // 6. Persist
    search_state
        .save_items_to_db()
        .map_err(|e| AppError::Other(format!("Failed to save index: {}", e)))?;

    let total = {
        let items = search_state
            .items
            .read()
            .map_err(|e| AppError::Other(e.to_string()))?;
        items.iter().filter(|i| i.id().starts_with("app_")).count() as u32
    };

    info!(
        "App sync complete: {} added, {} removed, {} renamed, {} total apps",
        added, removed, renamed, total
    );
    Ok(SyncResult {
        added,
        removed,
        total,
    })
}

/// Brings the names of already-indexed apps back in line with what a fresh
/// scan reports, returning how many changed.
///
/// The add/remove diff cannot do this. An app's id is built from its bundle
/// file name (see [`bundle_file_name`]), which no rename of the *displayed*
/// name touches — so such an app sits in both the indexed and the scanned set
/// and is neither added nor removed. Without this pass it would keep whatever
/// name it was first indexed under for the lifetime of the index, including
/// every install indexed before this resolution was fixed.
///
/// Only the name and icon are copied over. `usage_count` and `last_used_at` live
/// on the indexed item and are absent from a freshly scanned one, so swapping the
/// whole struct would silently reset every app's frecency on each scan.
fn refresh_display_names(
    items: &mut [SearchableItem],
    scanned: &HashMap<String, Application>,
) -> u32 {
    let mut renamed = 0;
    for item in items.iter_mut() {
        let SearchableItem::Application(indexed) = item else {
            continue;
        };
        let Some(fresh) = scanned.get(&indexed.id) else {
            continue;
        };
        let mut changed = false;
        if indexed.name != fresh.name {
            indexed.name.clone_from(&fresh.name);
            changed = true;
        }
        if indexed.icon != fresh.icon {
            indexed.icon.clone_from(&fresh.icon);
            changed = true;
        }
        if changed {
            renamed += 1;
        }
        // Not a rename: backfills apps indexed before `exe_name` existed.
        indexed.exe_name.clone_from(&fresh.exe_name);
    }
    renamed
}

pub fn list_applications<R: tauri::Runtime>(
    app: &AppHandle<R>,
    extra_paths: Vec<PathBuf>,
) -> Result<Vec<Application>, AppError> {
    let mut scanner = AppScanner::new();
    scanner.scan_all(extra_paths)?;

    let icon_cache_dir = icon_cache_dir(app);
    let mut applications = Vec::new();

    for path_str in &scanner.paths {
        let path = Path::new(path_str);
        let full_app_id = build_app_id(&bundle_file_name(path), path_str);

        applications.push(Application {
            id: full_app_id,
            name: display_name(path),
            path: path_str.clone(),
            usage_count: 0,
            icon: extract_app_icon(path_str, &icon_cache_dir),
            last_used_at: None,
            bundle_id: extract_bundle_id(Path::new(path_str)),
            exe_name: extract_exe_name(path),
        });
    }

    Ok(applications)
}

struct AppScanner {
    paths: Vec<String>,
    /// Paths already recorded, to skip duplicates when scan roots overlap
    /// (e.g. a custom scan path that is an ancestor of a default one — #410).
    seen: HashSet<String>,
    /// Canonical paths of directories already scanned, preventing infinite
    /// recursion caused by circular symlinks or overlapping scan trees.
    visited_dirs: HashSet<PathBuf>,
}

impl AppScanner {
    fn new() -> Self {
        Self {
            paths: Vec::new(),
            seen: HashSet::new(),
            visited_dirs: HashSet::new(),
        }
    }

    /// Records a discovered bundle, skipping one an earlier scan root already
    /// yielded (#410).
    fn record(&mut self, path: &Path) {
        #[cfg(target_os = "linux")]
        {
            if !is_bundle_visible(path) {
                return;
            }
        }
        if let Some(path_str) = path.to_str() {
            if self.seen.insert(path_str.to_string()) {
                self.paths.push(path_str.to_string());
            }
        }
    }

    fn scan_directory(&mut self, dir_path: &Path) -> Result<(), AppError> {
        self.scan_directory_inner(dir_path, 0)
    }

    fn scan_directory_inner(&mut self, dir_path: &Path, depth: usize) -> Result<(), AppError> {
        const MAX_SCAN_DEPTH: usize = 3;
        if depth > MAX_SCAN_DEPTH {
            return Ok(());
        }

        // A scan root may name a single bundle rather than a folder of them —
        // a custom path a user aimed straight at one app. That bundle *is* the app: record it and
        // stop, because descending would index the helper apps nested in it.
        if is_app_bundle(dir_path) && dir_path.exists() {
            self.record(dir_path);
            return Ok(());
        }
        if !dir_path.is_dir() {
            return Ok(());
        }

        // Guard against cycles: canonicalize directory path before descending.
        let canonical_dir = dir_path
            .canonicalize()
            .unwrap_or_else(|_| dir_path.to_path_buf());
        if !self.visited_dirs.insert(canonical_dir) {
            return Ok(());
        }

        for entry in fs::read_dir(dir_path)?.filter_map(Result::ok) {
            let path = entry.path();
            if is_app_bundle(&path) {
                self.record(&path);
            } else {
                let is_symlink = entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false);
                // Never recurse into directory symlinks (prevents recursive loops,
                // escaping the app directory, and scanning unbounded trees).
                if !is_symlink && path.is_dir() {
                    let _ = self.scan_directory_inner(&path, depth + 1);
                }
            }
        }
        Ok(())
    }

    fn scan_all(&mut self, extra_paths: Vec<PathBuf>) -> Result<(), AppError> {
        let mut directories = get_default_app_scan_paths();
        directories.extend(extra_paths);

        for dir in directories {
            if let Err(e) = self.scan_directory(&dir) {
                info!("Error scanning {:?}: {}", dir, e);
            }
        }

        Ok(())
    }
}

pub fn is_default_app_location(app_path: &str) -> bool {
    let path = Path::new(app_path);
    get_default_app_scan_paths()
        .iter()
        .any(|dir| path.starts_with(dir))
}

pub fn display_path(app_path: &str) -> String {
    let path = Path::new(app_path);
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
        }
    }
    app_path.to_string()
}

pub fn normalize_scan_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.len() <= 1 {
        return trimmed.to_string();
    }
    trimmed
        .trim_end_matches(['/', std::path::MAIN_SEPARATOR])
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPathValidationError {
    Empty,
    Relative,
    TildeUnsupported,
    FileUrlUnsupported,
    NotFound,
    NotDirectory,
    FilesystemRoot,
    HomeDirectory,
    Duplicate,
}

impl ScanPathValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Relative => "relative",
            Self::TildeUnsupported => "tilde_unsupported",
            Self::FileUrlUnsupported => "file_url_unsupported",
            Self::NotFound => "not_found",
            Self::NotDirectory => "not_directory",
            Self::FilesystemRoot => "filesystem_root",
            Self::HomeDirectory => "home_directory",
            Self::Duplicate => "duplicate",
        }
    }
}

/// Validates a user-entered application scan directory and returns its
/// canonical filesystem path. Text shortcuts and URLs are deliberately
/// rejected: accepting them here would make persisted paths platform- and
/// shell-dependent. The filesystem root and the user's home directory are
/// too broad for recursive application discovery and are rejected as a
/// privacy and performance safeguard.
pub fn validate_user_scan_path(
    path: &str,
    existing_paths: &[String],
) -> Result<PathBuf, ScanPathValidationError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ScanPathValidationError::Empty);
    }
    if trimmed.starts_with('~') {
        return Err(ScanPathValidationError::TildeUnsupported);
    }
    if trimmed.to_ascii_lowercase().starts_with("file://") {
        return Err(ScanPathValidationError::FileUrlUnsupported);
    }

    let candidate = Path::new(trimmed);
    if !candidate.is_absolute() {
        return Err(ScanPathValidationError::Relative);
    }
    if !candidate.exists() {
        return Err(ScanPathValidationError::NotFound);
    }
    if !candidate.is_dir() {
        return Err(ScanPathValidationError::NotDirectory);
    }

    let canonical = fs::canonicalize(candidate).map_err(|_| ScanPathValidationError::NotFound)?;
    if canonical.parent().is_none() {
        return Err(ScanPathValidationError::FilesystemRoot);
    }
    if dirs::home_dir()
        .and_then(|home| fs::canonicalize(home).ok())
        .is_some_and(|home| home == canonical)
    {
        return Err(ScanPathValidationError::HomeDirectory);
    }
    if existing_paths.iter().any(|existing| {
        let normalized = normalize_scan_path(existing);
        !normalized.is_empty()
            && fs::canonicalize(normalized).is_ok_and(|existing| existing == canonical)
    }) {
        return Err(ScanPathValidationError::Duplicate);
    }

    Ok(canonical)
}

pub fn display_parent_dir(app_path: &str) -> String {
    let parent = Path::new(app_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(app_path);
    display_path(parent)
}

pub(crate) fn is_bundle_visible(path: &Path) -> bool {
    if path.extension().is_some_and(|ext| ext == "desktop") {
        return crate::platform::linux::is_visible_desktop_file(path);
    }
    true
}

pub fn get_linux_default_app_scan_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    let mut add_path = |p: PathBuf| {
        if seen.insert(p.clone()) {
            paths.push(p);
        }
    };

    // 1. $XDG_DATA_HOME/applications (defaults to ~/.local/share/applications)
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        let trimmed = data_home.trim();
        if !trimmed.is_empty() {
            add_path(PathBuf::from(trimmed).join("applications"));
        }
    } else if let Some(home) = dirs::home_dir() {
        add_path(home.join(".local/share/applications"));
    }

    // 2. $XDG_DATA_DIRS/applications (defaults to /usr/local/share/applications and /usr/share/applications)
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':') {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                add_path(PathBuf::from(trimmed).join("applications"));
            }
        }
    } else {
        add_path(PathBuf::from("/usr/local/share/applications"));
        add_path(PathBuf::from("/usr/share/applications"));
    }

    // 3. Flatpak applications (user & system exports)
    if let Some(home) = dirs::home_dir() {
        add_path(home.join(".local/share/flatpak/exports/share/applications"));
    }
    add_path(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    // 4. Snap applications
    add_path(PathBuf::from("/var/lib/snapd/desktop/applications"));
    add_path(PathBuf::from("/snap/share/applications"));

    // 5. Nix applications (user profile & system profile)
    if let Some(home) = dirs::home_dir() {
        add_path(home.join(".nix-profile/share/applications"));
    }
    add_path(PathBuf::from(
        "/nix/var/nix/profiles/default/share/applications",
    ));

    paths
}

pub fn get_default_app_scan_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        get_linux_default_app_scan_paths()
    }
}

fn is_app_bundle(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        path.extension().map(|e| e == "desktop").unwrap_or(false)
    }
}

/// Extract a platform-native bundle / process identifier for an installed app.
///
/// Works the same whether the path comes from a default scan location or from
/// a user-configured custom scan directory — the logic is purely
/// path-content-based (.desktop), no registry lookup.
/// The bundle's file name on disk, without extension.
///
/// This is the app's stable identity: `build_app_id` embeds it, and the id
/// in turn keys usage stats, user-assigned aliases and item shortcuts.
/// It must never be swapped for a display name, or all three break silently
/// the first time a user's language changes what the OS reports.
pub(crate) fn bundle_file_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unknown_App")
        .to_string()
}

/// What the user sees for this app: the name translated into the user's
/// language where the bundle ships one, the plain file name otherwise.
///
/// Deliberately separate from [`bundle_file_name`] — search matches both, but
/// only the file name may reach `build_app_id`.
pub(crate) fn display_name(path: &Path) -> String {
    crate::platform::localized_bundle_name(path).unwrap_or_else(|| bundle_file_name(path))
}

pub(crate) fn extract_bundle_id(path: &Path) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::DesktopEntry::from_file(path)
            .and_then(|entry| entry.extract_bundle_id())
    }
}

fn resolve_linux_exe_name(
    exec_program: &str,
    path_env: Option<&std::ffi::OsStr>,
) -> Option<String> {
    const LINUX_EXE_WRAPPERS: &[&str] = &[
        "env", "sh", "bash", "dash", "zsh", "flatpak", "bwrap", "python", "python3", "node",
        "electron", "java",
    ];

    let initial_lower = Path::new(exec_program)
        .file_name()
        .and_then(|s| s.to_str())?
        .to_lowercase();
    if initial_lower.is_empty() || LINUX_EXE_WRAPPERS.contains(&initial_lower.as_str()) {
        return None;
    }

    let candidate = if exec_program.contains('/') {
        Some(PathBuf::from(exec_program))
    } else {
        path_env.and_then(|paths| {
            std::env::split_paths(paths)
                .map(|dir| dir.join(exec_program))
                .find(|p| p.exists())
        })
    };

    if let Some(canonical) = candidate.and_then(|p| fs::canonicalize(p).ok()) {
        if let Some(canonical_base) = canonical.file_name().and_then(|s| s.to_str()) {
            let canonical_lower = canonical_base.to_lowercase();
            if !canonical_lower.is_empty()
                && !LINUX_EXE_WRAPPERS.contains(&canonical_lower.as_str())
            {
                return Some(canonical_lower);
            }
        }
    }

    Some(initial_lower)
}

fn resolve_linux_desktop_entry_exe_name(
    entry: &crate::platform::linux::DesktopEntry,
    path_env: Option<&std::ffi::OsStr>,
) -> Option<String> {
    if entry.is_flatpak_exec() {
        let basename = entry.exec_basename()?;
        return resolve_linux_exe_name(&basename, None);
    }
    let prog = entry.exec_program_path()?;
    resolve_linux_exe_name(&prog, path_env)
}

/// Lowercased executable basename the running indicator matches against
/// `AppPresenceQuery::running_keys`. `None` wherever no executable can be
/// named, so the app simply gets no dot.
pub(crate) fn extract_exe_name(path: &Path) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::DesktopEntry::from_file(path).and_then(|entry| {
            resolve_linux_desktop_entry_exe_name(&entry, std::env::var_os("PATH").as_deref())
        })
    }
}

/// Resolves the directory used to cache extracted app icons. Icons are never
/// purged from here: clipboard history references source-app icons from this
/// directory and only regenerates them on a new capture.
fn icon_cache_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|p| p.join("icon_cache"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/asyar_icon_cache"))
}

pub(crate) fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    let limit = s.len().min(max_bytes);
    &s[..s.floor_char_boundary(limit)]
}

pub(crate) fn truncate_cache_key(cache_key: &str, max_bytes: usize) -> std::borrow::Cow<'_, str> {
    if cache_key.len() <= max_bytes {
        return std::borrow::Cow::Borrowed(cache_key);
    }

    use std::hash::{Hash, Hasher};
    let mut h = fnv::FnvHasher::default();
    cache_key.hash(&mut h);
    let hash_str = format!("{:016x}", h.finish());

    let prefix_limit = max_bytes.saturating_sub(1 + hash_str.len());
    let prefix = truncate_to_char_boundary(cache_key, prefix_limit);
    std::borrow::Cow::Owned(format!("{prefix}_{hash_str}"))
}

/// Returns the timestamp that determines icon freshness for an application:
/// the `.desktop` file's own modification time.
fn icon_source_mtime(app_path: &Path) -> Option<std::time::SystemTime> {
    app_path.metadata().ok()?.modified().ok()
}

fn cached_icon_is_fresh(cached: &Path, source_mtime: Option<std::time::SystemTime>) -> bool {
    if !cached.exists() {
        return false;
    }
    let Some(source_mtime) = source_mtime else {
        return true;
    };
    match cached.metadata().and_then(|meta| meta.modified()) {
        Ok(cached_mtime) => {
            // Check for matching timestamps. Accounts for sub-second precision variance
            // across differing filesystems by allowing < 1s difference.
            cached_mtime == source_mtime
                || cached_mtime
                    .duration_since(source_mtime)
                    .or_else(|_| source_mtime.duration_since(cached_mtime))
                    .map(|d| d.as_secs() == 0)
                    .unwrap_or(false)
        }
        Err(_) => false,
    }
}

/// Writes a "no icon found" marker. The source mtime is stored in the marker's
/// contents so a changed source retries immediately.
fn write_missing_marker(marker: &Path, source_mtime: Option<std::time::SystemTime>) {
    let _ = std::fs::create_dir_all(marker.parent().unwrap());
    let contents = source_mtime
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_default();
    let _ = std::fs::write(marker, contents);
}

/// A negative result is retried when the source mtime differs from the one
/// stored when the marker was written.
fn missing_marker_is_fresh(marker: &Path, source_mtime: Option<std::time::SystemTime>) -> bool {
    if marker.metadata().and_then(|m| m.modified()).is_err() {
        return false;
    }
    // The source mtime is stored in the marker contents; a changed source means
    // the marker is stale regardless of age.
    let source_unchanged = match source_mtime {
        None => true,
        Some(current) => {
            let Ok(contents) = std::fs::read_to_string(marker) else {
                return false;
            };
            let Ok(stored_secs) = contents.trim().parse::<u64>() else {
                return false;
            };
            let stored = std::time::UNIX_EPOCH + std::time::Duration::from_secs(stored_secs);
            stored == current
                || stored
                    .duration_since(current)
                    .or_else(|_| current.duration_since(stored))
                    .map(|d| d.as_secs() == 0)
                    .unwrap_or(false)
        }
    };
    source_unchanged
}

fn stamp_cache_file(path: &Path, source_mtime: Option<std::time::SystemTime>) {
    let Some(time) = source_mtime else {
        return;
    };
    let _ = std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(time));
}

/// Builds the cache file stem for an application's extracted icon.
fn app_icon_cache_base_name(app_path: &str) -> String {
    let raw = app_path
        .replace(['/', '\\', ':', ' '], "_")
        .replace(".app", "")
        .replace(".desktop", "")
        .replace(".exe", "");
    truncate_cache_key(&raw, 200).into_owned()
}

pub(crate) fn extract_app_icon(app_path: &str, cache_dir: &Path) -> Option<String> {
    let base_name = app_icon_cache_base_name(app_path);
    let cache_filename_png = format!("{base_name}.png");
    let cache_file_png = cache_dir.join(&cache_filename_png);
    let cache_filename_svg = format!("{base_name}.svg");
    let cache_file_svg = cache_dir.join(&cache_filename_svg);
    let missing_marker = cache_dir.join(format!("{base_name}.missing"));

    let source_mtime = icon_source_mtime(Path::new(app_path));

    let icon_url = move |filename: &str| -> String { format!("asyar-icon://localhost/{filename}") };

    if cached_icon_is_fresh(&cache_file_png, source_mtime) {
        return Some(icon_url(&cache_filename_png));
    }
    if cached_icon_is_fresh(&cache_file_svg, source_mtime) {
        return Some(icon_url(&cache_filename_svg));
    }
    if missing_marker_is_fresh(&missing_marker, source_mtime) {
        return None;
    }

    let entry = IconCacheFiles {
        cache_dir: cache_dir.to_path_buf(),
        png_file: cache_file_png,
        png_name: cache_filename_png,
        svg_file: cache_file_svg,
        svg_name: cache_filename_svg,
        missing_marker,
    };

    match crate::platform::extract_icon_outcome(Path::new(app_path)) {
        Some(ExtractedIcon::Icon(bytes)) => {
            store_extracted_icon(&entry, bytes, source_mtime, &icon_url)
        }
        Some(ExtractedIcon::Placeholder(bytes)) => {
            // Soft cache: write the placeholder so the user still sees an icon,
            // but do not stamp it with `source_mtime` — it must be re-extracted
            // on the next scan and repairs itself when the target reappears.
            store_extracted_icon(&entry, bytes, None, &icon_url)
        }
        None => on_extraction_failed(&entry, source_mtime, &icon_url),
    }
}

/// All the cache paths for a single app icon entry.
struct IconCacheFiles {
    cache_dir: PathBuf,
    png_file: PathBuf,
    png_name: String,
    svg_file: PathBuf,
    svg_name: String,
    missing_marker: PathBuf,
}

/// Writes extracted icon bytes to the cache and removes the negative marker.
/// `stamp` is the source mtime to stamp the file with; `None` means a soft
/// cached placeholder that must stay retryable.
fn store_extracted_icon(
    entry: &IconCacheFiles,
    bytes: Vec<u8>,
    stamp: Option<std::time::SystemTime>,
    icon_url: &dyn Fn(&str) -> String,
) -> Option<String> {
    let _ = std::fs::create_dir_all(&entry.cache_dir);
    let is_svg = bytes.starts_with(b"<") || bytes.windows(4).take(32).any(|w| w == b"<svg");
    let (chosen_filename, chosen_file, obsolete_file) = if is_svg {
        (&entry.svg_name, &entry.svg_file, &entry.png_file)
    } else {
        (&entry.png_name, &entry.png_file, &entry.svg_file)
    };
    if std::fs::write(chosen_file, bytes).is_ok() {
        let _ = std::fs::remove_file(obsolete_file);
        let _ = std::fs::remove_file(&entry.missing_marker);
        stamp_cache_file(chosen_file, stamp);
        return Some(icon_url(chosen_filename));
    }
    None
}

/// Extraction failed: preserve any previously working cached icon rather than
/// dropping it, but never re-stamp it — an unstamped placeholder must stay
/// retryable, and stamping would make it forever-fresh.
fn on_extraction_failed(
    entry: &IconCacheFiles,
    source_mtime: Option<std::time::SystemTime>,
    icon_url: &dyn Fn(&str) -> String,
) -> Option<String> {
    if entry.png_file.exists() {
        return Some(icon_url(&entry.png_name));
    }
    if entry.svg_file.exists() {
        return Some(icon_url(&entry.svg_name));
    }

    // Cache the negative result so future launches skip expensive extraction.
    // The source mtime is stored in the marker contents; the marker is not
    // stamped with it.
    let _ = std::fs::create_dir_all(&entry.cache_dir);
    write_missing_marker(&entry.missing_marker, source_mtime);

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_get_default_app_scan_paths_is_non_empty() {
        let paths = get_default_app_scan_paths();
        assert!(
            !paths.is_empty(),
            "get_default_app_scan_paths() must return at least one path on every platform"
        );
    }

    /// An app as the index holds it, carrying frecency data a scan never has.
    fn indexed_app(id: &str, name: &str, usage_count: u32) -> SearchableItem {
        SearchableItem::Application(Application {
            id: id.to_string(),
            name: name.to_string(),
            path: format!("/Applications/{name}.app"),
            usage_count,
            icon: None,
            last_used_at: Some(1_700_000_000),
            bundle_id: None,
            exe_name: None,
        })
    }

    /// The same app as a fresh scan reports it: no usage history.
    fn scanned_app(id: &str, name: &str) -> (String, Application) {
        (
            id.to_string(),
            Application {
                id: id.to_string(),
                name: name.to_string(),
                path: format!("/Applications/{name}.app"),
                usage_count: 0,
                icon: None,
                last_used_at: None,
                bundle_id: None,
                exe_name: None,
            },
        )
    }

    #[test]
    fn test_refresh_display_names_updates_a_stale_name() {
        // The reported bug: an app indexed as "Photos" before the localized
        // name resolved correctly keeps its id, so the add/remove diff never
        // touches it. This pass is what makes the new name reach the index.
        let mut items = vec![indexed_app("app_Photos", "Photos", 0)];
        let scanned = HashMap::from([scanned_app("app_Photos", "Fotos")]);

        assert_eq!(refresh_display_names(&mut items, &scanned), 1);
        assert_eq!(items[0].get_name(), "Fotos");
    }

    #[test]
    fn test_refresh_display_names_preserves_frecency() {
        // Guards the reason only the name is copied: replacing the indexed
        // item with the scanned one would zero the usage history of every
        // renamed app on the very next scan.
        let mut items = vec![indexed_app("app_Photos", "Photos", 42)];
        let scanned = HashMap::from([scanned_app("app_Photos", "Fotos")]);

        refresh_display_names(&mut items, &scanned);

        let SearchableItem::Application(app) = &items[0] else {
            panic!("item must stay an application");
        };
        assert_eq!(app.usage_count, 42, "usage count must survive a rename");
        assert_eq!(
            app.last_used_at,
            Some(1_700_000_000),
            "last use must survive a rename"
        );
    }

    #[test]
    fn test_refresh_display_names_leaves_matching_and_unscanned_items_alone() {
        let mut items = vec![
            indexed_app("app_Safari", "Safari", 7),
            // Not in the scan — e.g. an app on a scan path the user just
            // removed. Untouched here; the remove diff owns that case.
            indexed_app("app_Ghost", "Ghost", 3),
        ];
        let scanned = HashMap::from([scanned_app("app_Safari", "Safari")]);

        assert_eq!(
            refresh_display_names(&mut items, &scanned),
            0,
            "an unchanged name must not count as a rename"
        );
        assert_eq!(items[0].get_name(), "Safari");
        assert_eq!(items[1].get_name(), "Ghost");
    }

    #[test]
    fn test_scanner_records_a_scan_root_that_is_itself_a_bundle() {
        // A scan entry may name a single bundle rather than a folder of them —
        // a custom path a user aimed straight at one
        // app. Only a root's children were ever considered, so such an entry
        // yielded nothing at all.
        let tmp = std::env::temp_dir().join("asyar_test_bundle_root");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        #[cfg(target_os = "linux")]
        let app_path = tmp.join("rooted.desktop");

        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=Rooted\nExec=rooted\n",
        )
        .unwrap();

        let mut scanner = AppScanner::new();
        scanner.scan_directory(&app_path).unwrap();

        assert_eq!(
            scanner.paths,
            vec![app_path.to_str().unwrap().to_string()],
            "a scan root that is itself a bundle must be recorded"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_scanner_ignores_a_bundle_scan_root_that_is_absent() {
        // A scan root that no longer exists must degrade to "not indexed", not
        // to a row pointing at nothing.
        #[cfg(target_os = "linux")]
        let missing = std::env::temp_dir().join("asyar_absent_bundle.desktop");
        let _ = fs::remove_dir_all(&missing);

        let mut scanner = AppScanner::new();
        scanner.scan_directory(&missing).unwrap();

        assert!(scanner.paths.is_empty());
    }

    #[test]
    fn test_display_name_falls_back_to_file_name_for_unknown_bundle() {
        // A path the OS knows nothing about must still yield the file name
        // rather than an empty string, or the app would index as nameless.
        let tmp = std::env::temp_dir().join("asyar_display_name_fallback_Widget.app");
        assert_eq!(
            display_name(&tmp),
            "asyar_display_name_fallback_Widget",
            "unknown bundles fall back to the file name"
        );
    }

    #[test]
    fn test_bundle_file_name_is_independent_of_display_name() {
        // The id is built from this, so it must stay the plain file stem.
        // If this ever tracked the display name, every localized app would
        // get a new id and silently lose its usage stats, aliases and
        // shortcuts on the first scan after a language change.
        assert_eq!(
            bundle_file_name(Path::new("/System/Applications/Photos.app")),
            "Photos"
        );
    }

    #[test]
    fn test_normalize_scan_path_trims_whitespace() {
        assert_eq!(normalize_scan_path("  /Applications  "), "/Applications");
    }

    #[test]
    fn test_normalize_scan_path_strips_trailing_forward_slash() {
        assert_eq!(normalize_scan_path("/Applications/"), "/Applications");
    }

    #[test]
    fn test_normalize_scan_path_preserves_root_slash() {
        assert_eq!(normalize_scan_path("/"), "/");
    }

    #[test]
    fn test_normalize_scan_path_returns_empty_for_blank_input() {
        assert_eq!(normalize_scan_path("   "), "");
    }

    #[test]
    fn validate_user_scan_path_rejects_nonexistent_path() {
        let temp = tempfile::tempdir().unwrap();
        let nonexistent = temp.path().join("missing-application-directory");

        assert_eq!(
            validate_user_scan_path(nonexistent.to_str().unwrap(), &[]),
            Err(ScanPathValidationError::NotFound)
        );
    }

    #[test]
    fn validate_user_scan_path_rejects_relative_and_tilde_paths() {
        assert_eq!(
            validate_user_scan_path("Applications", &[]),
            Err(ScanPathValidationError::Relative)
        );
        assert_eq!(
            validate_user_scan_path("~/Applications", &[]),
            Err(ScanPathValidationError::TildeUnsupported)
        );
    }

    #[test]
    fn validate_user_scan_path_rejects_file_urls() {
        assert_eq!(
            validate_user_scan_path("file:///Applications", &[]),
            Err(ScanPathValidationError::FileUrlUnsupported)
        );
    }

    #[test]
    fn validate_user_scan_path_rejects_files_and_filesystem_root() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("not-a-directory");
        std::fs::write(&file, b"test").unwrap();
        let canonical_temp = std::fs::canonicalize(temp.path()).unwrap();
        let mut filesystem_root = canonical_temp.as_path();
        while let Some(parent) = filesystem_root.parent() {
            filesystem_root = parent;
        }

        assert_eq!(
            validate_user_scan_path(file.to_str().unwrap(), &[]),
            Err(ScanPathValidationError::NotDirectory)
        );
        assert_eq!(
            validate_user_scan_path(filesystem_root.to_str().unwrap(), &[]),
            Err(ScanPathValidationError::FilesystemRoot)
        );
    }

    #[test]
    fn validate_user_scan_path_rejects_home_as_overly_broad() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(
            validate_user_scan_path(home.to_str().unwrap(), &[]),
            Err(ScanPathValidationError::HomeDirectory)
        );
    }

    #[test]
    fn validate_user_scan_path_returns_canonical_path_and_detects_canonical_duplicate() {
        let temp = tempfile::tempdir().unwrap();
        let apps = temp.path().join("Applications");
        std::fs::create_dir(&apps).unwrap();
        let canonical = std::fs::canonicalize(&apps).unwrap();

        assert_eq!(
            validate_user_scan_path(apps.to_str().unwrap(), &[]),
            Ok(canonical.clone())
        );
        assert_eq!(
            validate_user_scan_path(
                apps.to_str().unwrap(),
                &[canonical.to_string_lossy().into_owned()]
            ),
            Err(ScanPathValidationError::Duplicate)
        );
    }

    #[test]
    fn test_is_default_app_location_handles_unrelated_paths() {
        assert!(!is_default_app_location("/nonexistent/path/App.app"));
    }

    #[test]
    fn test_display_path_tildes_home_prefix() {
        let input = dirs::home_dir()
            .unwrap()
            .join("Applications")
            .join("Foo.app");
        let expected = format!(
            "~{sep}Applications{sep}Foo.app",
            sep = std::path::MAIN_SEPARATOR
        );
        assert_eq!(display_path(input.to_str().unwrap()), expected);
    }

    #[test]
    fn test_display_path_passes_through_non_home_paths() {
        let outside = outside_home_path();
        assert_eq!(display_path(&outside), outside);
    }

    #[test]
    fn test_display_path_handles_bare_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(display_path(home.to_str().unwrap()), "~");
    }

    #[test]
    fn test_display_parent_dir_strips_app_bundle_and_tildes_home() {
        let input = dirs::home_dir()
            .unwrap()
            .join("Applications")
            .join("Ice.app");
        let expected = format!("~{sep}Applications", sep = std::path::MAIN_SEPARATOR);
        assert_eq!(display_parent_dir(input.to_str().unwrap()), expected);
    }

    #[test]
    fn test_display_parent_dir_passes_through_non_home_parent() {
        let outside = outside_home_path();
        let parent = Path::new(&outside)
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(display_parent_dir(&outside), parent);
    }

    /// Returns an absolute path guaranteed not to live under `$HOME`. Used by tests that need to exercise the non-home branch.
    fn outside_home_path() -> String {
        "/opt/asyar-test/Foo.app".to_string()
    }

    #[test]
    fn test_display_path_does_not_confuse_prefix_collision() {
        // Prefix must match at a path boundary, not mid-segment — e.g. a path
        // that starts with the home string but diverges before the next
        // separator should be returned unchanged.
        let home = dirs::home_dir().unwrap();
        let sibling = format!("{}_other", home.to_str().unwrap());
        assert_eq!(display_path(&sibling), sibling);
    }

    #[test]
    fn test_is_app_bundle_no_extension_is_false() {
        assert!(!is_app_bundle(Path::new("/some/path/without_extension")));
    }

    #[test]
    fn test_is_app_bundle_wrong_extension_is_false() {
        assert!(!is_app_bundle(Path::new("/tmp/somefile.txt")));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_is_app_bundle_linux_dot_desktop() {
        assert!(is_app_bundle(Path::new(
            "/usr/share/applications/firefox.desktop"
        )));
    }

    #[test]
    fn test_scanner_new_is_empty() {
        let scanner = AppScanner::new();
        assert!(scanner.paths.is_empty());
    }

    #[test]
    fn test_scanner_scan_nonexistent_dir_does_not_crash() {
        let mut scanner = AppScanner::new();
        let result = scanner.scan_directory(Path::new("/tmp/nonexistent_asyar_apps_12345"));
        assert!(result.is_ok());
        assert!(scanner.paths.is_empty());
    }

    #[test]
    fn test_scanner_scan_all_with_extra_paths_does_not_crash() {
        let mut scanner = AppScanner::new();
        let extra = PathBuf::from("/tmp/nonexistent_asyar_apps");
        let result = scanner.scan_all(vec![extra]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_scanner_discovers_app_bundles_in_temp_dir() {
        let tmp = std::env::temp_dir().join("asyar_test_scanner");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        #[cfg(target_os = "linux")]
        let app_path = tmp.join("test.desktop");

        // Create a fake .desktop entry
        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=Test\nExec=test\n",
        )
        .unwrap();

        let mut scanner = AppScanner::new();
        let _ = scanner.scan_directory(&tmp);

        assert_eq!(scanner.paths.len(), 1);
        assert!(scanner.paths[0].to_lowercase().contains("test"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_scanner_dedupes_overlapping_scan_dirs() {
        // Reproduces #410: a custom scan path that is an ancestor of a default
        // scan path makes the same app bundle reachable through two scan roots.
        // The scanner must return each path exactly once — duplicate paths
        // produce duplicate app IDs downstream, which crashes the keyed list
        // in the Applications settings tab.
        let tmp = std::env::temp_dir().join("asyar_test_overlap_scan");
        let _ = fs::remove_dir_all(&tmp);
        let child = tmp.join("child");
        fs::create_dir_all(&child).unwrap();

        #[cfg(target_os = "linux")]
        let app_path = child.join("overlap.desktop");

        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=Overlap\nExec=overlap\n",
        )
        .unwrap();

        let mut scanner = AppScanner::new();
        // Simulate scan_all visiting both an ancestor (custom path) and the
        // nested default path. The ancestor scan recurses into `child`.
        scanner.scan_directory(&tmp).unwrap();
        scanner.scan_directory(&child).unwrap();

        assert_eq!(
            scanner.paths.len(),
            1,
            "overlapping scan roots must not yield duplicate paths, got {:?}",
            scanner.paths
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn test_scanner_handles_symlink_loop_without_hanging() {
        let tmp = std::env::temp_dir().join("asyar_test_symlink_loop");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create a circular symlink: tmp/loop -> tmp
        let loop_link = tmp.join("loop");
        let _ = std::os::unix::fs::symlink(&tmp, &loop_link);

        let app_path = tmp.join("normal.desktop");

        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=Normal\nExec=normal\n",
        )
        .unwrap();

        let mut scanner = AppScanner::new();
        // Must complete promptly without recursion stack overflow or hang
        let res = scanner.scan_directory(&tmp);
        assert!(res.is_ok());
        assert_eq!(scanner.paths.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_scanner_respects_max_depth() {
        let tmp = std::env::temp_dir().join("asyar_test_max_depth");
        let _ = fs::remove_dir_all(&tmp);

        // depth 0: tmp
        // depth 1: d1
        // depth 2: d2
        // depth 3: d3
        // depth 4: d4 -> TooDeep.app (exceeds MAX_SCAN_DEPTH = 3)
        let deep_dir = tmp.join("d1").join("d2").join("d3").join("d4");
        fs::create_dir_all(&deep_dir).unwrap();

        let app_path = deep_dir.join("toodeep.desktop");

        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=TooDeep\nExec=toodeep\n",
        )
        .unwrap();

        let mut scanner = AppScanner::new();
        scanner.scan_directory(&tmp).unwrap();
        assert!(
            scanner.paths.is_empty(),
            "App bundles beyond MAX_SCAN_DEPTH must not be indexed, found: {:?}",
            scanner.paths
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn test_scanner_skips_directory_symlinks_to_external_trees() {
        let base = std::env::temp_dir().join("asyar_test_symlink_dir_skip");
        let _ = fs::remove_dir_all(&base);
        let scan_dir = base.join("scan_root");
        let external_dir = base.join("external");
        fs::create_dir_all(&scan_dir).unwrap();
        fs::create_dir_all(&external_dir).unwrap();

        let app_path = external_dir.join("external.desktop");

        fs::write(
            &app_path,
            b"[Desktop Entry]\nType=Application\nName=External\nExec=external\n",
        )
        .unwrap();

        // Symlink a directory inside scan_root pointing to external
        let link_dir = scan_dir.join("external_link");
        let _ = std::os::unix::fs::symlink(&external_dir, &link_dir);

        let mut scanner = AppScanner::new();
        scanner.scan_directory(&scan_dir).unwrap();
        assert!(
            scanner.paths.is_empty(),
            "Directory symlinks outside app bundles must not be followed"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_frontmost_application_struct_serializes() {
        let app = FrontmostApplication {
            name: "Safari".to_string(),
            bundle_id: Some("com.apple.Safari".to_string()),
            path: None,
            window_title: Some("Apple".to_string()),
        };
        let json = serde_json::to_string(&app).unwrap();
        assert!(json.contains("Safari"));
        assert!(json.contains("bundleId"));
        assert!(json.contains("windowTitle"));
    }

    #[test]
    fn test_sync_result_serializes() {
        let result = SyncResult {
            added: 5,
            removed: 2,
            total: 100,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"added\":5"));
        assert!(json.contains("\"removed\":2"));
        assert!(json.contains("\"total\":100"));
    }

    #[test]
    fn test_extract_app_icon_negative_caching() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");

        // Non-existent path returns None and creates .missing marker
        let result = extract_app_icon("/nonexistent/path/app.desktop", &cache_dir);
        assert!(result.is_none());

        let marker = cache_dir.join(format!(
            "{}.missing",
            app_icon_cache_base_name("/nonexistent/path/app.desktop")
        ));
        assert!(marker.exists(), "missing marker must be created");

        // Subsequent call immediately returns None via marker check
        let result2 = extract_app_icon("/nonexistent/path/app.desktop", &cache_dir);
        assert!(result2.is_none());
    }

    #[test]
    fn test_extract_app_icon_multibyte_boundary_does_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");

        // Construct a path whose cache_key has multi-byte char '』' spanning bytes 198..201
        let prefix = "a".repeat(198);
        let path = format!("{prefix}』extra.exe");

        // Under un-guarded byte slicing, this panics at byte offset 200
        let result = extract_app_icon(&path, &cache_dir);
        assert!(result.is_none());

        let expected_key = app_icon_cache_base_name(&path);
        let marker = cache_dir.join(format!("{expected_key}.missing"));
        assert!(
            marker.exists(),
            "missing marker must be created with safe truncated filename"
        );
    }

    #[test]
    fn test_truncate_to_char_boundary_handles_multibyte_at_boundary() {
        // Shorter than limit
        assert_eq!(truncate_to_char_boundary("hello", 200), "hello");

        // Exactly at limit
        let exact = "a".repeat(200);
        assert_eq!(truncate_to_char_boundary(&exact, 200), exact.as_str());

        // Longer ASCII string truncated to limit
        let longer = "a".repeat(250);
        let truncated_longer = truncate_to_char_boundary(&longer, 200);
        assert_eq!(truncated_longer.len(), 200);

        // 3-byte char '』' (U+300F) spanning bytes 198..201 across 200-byte boundary
        let s3 = format!("{}』extra", "a".repeat(198));
        let truncated3 = truncate_to_char_boundary(&s3, 200);
        assert_eq!(truncated3.len(), 198);
        assert_eq!(truncated3, &"a".repeat(198));

        // 4-byte emoji '🎉' (U+1F389) spanning bytes 199..203 across 200-byte boundary
        let s4 = format!("{}🎉extra", "a".repeat(199));
        let truncated4 = truncate_to_char_boundary(&s4, 200);
        assert_eq!(truncated4.len(), 199);
        assert_eq!(truncated4, &"a".repeat(199));

        // Multi-byte char fitting completely before or exactly at the limit (bytes 197..200)
        let s_fits = format!("{}』extra", "a".repeat(197));
        let truncated_fits = truncate_to_char_boundary(&s_fits, 200);
        assert_eq!(truncated_fits.len(), 200);
        assert!(truncated_fits.ends_with('』'));

        // Edge cases
        assert_eq!(truncate_to_char_boundary("", 200), "");
        assert_eq!(truncate_to_char_boundary("hello", 0), "");
    }

    #[test]
    fn test_truncate_cache_key_avoids_collisions_for_shared_prefix() {
        // Shorter keys remain untouched
        assert_eq!(truncate_cache_key("calculator", 200).as_ref(), "calculator");

        // Two keys longer than 200 bytes sharing the first 210 bytes
        let prefix = "a".repeat(210);
        let key1 = format!("{prefix}_app1");
        let key2 = format!("{prefix}_app2");

        let truncated1 = truncate_cache_key(&key1, 200);
        let truncated2 = truncate_cache_key(&key2, 200);

        assert_ne!(
            truncated1, truncated2,
            "different keys sharing a prefix must not collide"
        );
        assert!(truncated1.len() <= 200);
        assert!(truncated2.len() <= 200);

        // Multi-byte chars near the prefix limit within the truncated cache key
        let mb_key = format!("{}』extra_app", "a".repeat(182));
        let truncated_mb = truncate_cache_key(&mb_key, 200);
        assert!(truncated_mb.len() <= 200);
    }

    #[test]
    fn test_refresh_display_names_copies_icon() {
        let mut items = vec![SearchableItem::Application(Application {
            id: "app_1".into(),
            name: "Demo".into(),
            path: "/Applications/Demo.app".into(),
            usage_count: 5,
            icon: None,
            last_used_at: Some(1_700_000_000),
            bundle_id: None,
            exe_name: None,
        })];

        let mut scanned = HashMap::new();
        scanned.insert(
            "app_1".into(),
            Application {
                id: "app_1".into(),
                name: "Demo Renamed".into(),
                path: "/Applications/Demo.app".into(),
                usage_count: 0,
                icon: Some("asyar-icon://localhost/demo.png".into()),
                last_used_at: None,
                bundle_id: None,
                exe_name: None,
            },
        );

        let updated = refresh_display_names(&mut items, &scanned);
        assert_eq!(updated, 1);
        let SearchableItem::Application(app) = &items[0] else {
            panic!("expected application item");
        };
        assert_eq!(app.name, "Demo Renamed");
        assert_eq!(app.icon, Some("asyar-icon://localhost/demo.png".into()));
        assert_eq!(app.usage_count, 5);
        assert_eq!(app.last_used_at, Some(1_700_000_000));
    }

    fn icon_cache_files(cache_dir: &Path, stem: &str) -> IconCacheFiles {
        IconCacheFiles {
            cache_dir: cache_dir.to_path_buf(),
            png_file: cache_dir.join(format!("{stem}.png")),
            png_name: format!("{stem}.png"),
            svg_file: cache_dir.join(format!("{stem}.svg")),
            svg_name: format!("{stem}.svg"),
            missing_marker: cache_dir.join(format!("{stem}.missing")),
        }
    }

    fn set_test_mtime(path: &Path, time: std::time::SystemTime) {
        #[cfg(unix)]
        {
            std::fs::File::open(path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }
    }

    #[test]
    fn test_cached_icon_is_fresh_requires_exact_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_file = tmp.path().join("test.png");
        std::fs::write(&cache_file, b"png").unwrap();

        let time_a =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let time_b =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_100);

        set_test_mtime(&cache_file, time_a);

        // Matching mtime -> fresh
        assert!(cached_icon_is_fresh(&cache_file, Some(time_a)));

        // Different mtime (newer bundle) -> stale
        assert!(!cached_icon_is_fresh(&cache_file, Some(time_b)));

        // Different mtime (older bundle, e.g. restored from archive) -> also stale
        set_test_mtime(&cache_file, time_b);
        assert!(!cached_icon_is_fresh(&cache_file, Some(time_a)));

        // None source_mtime -> relies on existence
        assert!(cached_icon_is_fresh(&cache_file, None));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_extract_app_icon_fallback_preserves_stale_cache_and_stays_retryable() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // An invalid app file where platform extraction returns None
        let app_file = tmp.path().join("nonexistent_extractor_target.unknown");
        std::fs::write(&app_file, b"data").unwrap();

        let old_time =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        let new_time =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        set_test_mtime(&app_file, new_time);

        let cache_key = app_file
            .to_string_lossy()
            .replace(['/', '\\', ':', ' '], "_");
        let base_name = truncate_cache_key(&cache_key, 200);
        let cached = cache_dir.join(format!("{base_name}.png"));
        std::fs::write(&cached, b"preserved-fallback").unwrap();
        set_test_mtime(&cached, old_time);

        let result = extract_app_icon(app_file.to_str().unwrap(), &cache_dir);
        assert!(result.is_some(), "must return preserved fallback");
        assert_eq!(std::fs::read(&cached).unwrap(), b"preserved-fallback");

        // The preserved fallback must NOT be re-stamped: an unstamped cache file
        // stays retryable, so it repairs itself once extraction succeeds again.
        assert!(
            !cached_icon_is_fresh(&cached, Some(new_time)),
            "fallback cache file must stay unstamped so extraction is retried"
        );
    }

    #[test]
    fn test_missing_marker_fresh_requires_unchanged_source_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let marker = cache_dir.join("demo.missing");
        let source_mtime = std::time::SystemTime::now();
        write_missing_marker(&marker, Some(source_mtime));

        // Unchanged source: fresh.
        assert!(missing_marker_is_fresh(&marker, Some(source_mtime)));

        // A changed source mtime retries immediately.
        let changed = source_mtime + std::time::Duration::from_secs(10);
        assert!(!missing_marker_is_fresh(&marker, Some(changed)));
    }

    #[test]
    fn test_stale_missing_marker_is_retried() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let marker = cache_dir.join("_nonexistent_path_app.missing");
        let source_mtime = std::time::SystemTime::now();
        write_missing_marker(&marker, Some(source_mtime));

        // Fresh marker: short-circuits without touching it.
        assert!(missing_marker_is_fresh(&marker, Some(source_mtime)));
        assert!(extract_app_icon("/nonexistent/path/app.desktop", &cache_dir).is_none());

        // A rewritten marker with no source mtime is fresh.
        write_missing_marker(&marker, None);
        assert!(
            missing_marker_is_fresh(&marker, None),
            "rewritten marker must be fresh"
        );
    }

    #[test]
    fn test_placeholder_icon_is_soft_cached_and_not_stamped() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let entry = icon_cache_files(&cache_dir, "demo");
        let source_mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(1000);
        let url = store_extracted_icon(&entry, b"placeholder-bytes".to_vec(), None, &|filename| {
            format!("http://asyar-icon.localhost/{filename}")
        });
        assert_eq!(
            url,
            Some("http://asyar-icon.localhost/demo.png".to_string())
        );
        assert_eq!(
            std::fs::read(&entry.png_file).unwrap(),
            b"placeholder-bytes"
        );
        // Not stamped with source_mtime: the next scan treats it as stale and
        // re-extracts, so a reappearing target repairs the cache.
        assert!(!cached_icon_is_fresh(&entry.png_file, Some(source_mtime)));
    }

    #[test]
    fn test_real_icon_is_stamped_with_source_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("icon_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let entry = icon_cache_files(&cache_dir, "demo");
        let source_mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(1000);
        let url = store_extracted_icon(
            &entry,
            b"real-png-bytes".to_vec(),
            Some(source_mtime),
            &|filename| format!("asyar-icon://localhost/{filename}"),
        );
        assert_eq!(url, Some("asyar-icon://localhost/demo.png".to_string()));
        assert!(cached_icon_is_fresh(&entry.png_file, Some(source_mtime)));
    }

    #[test]
    fn test_get_linux_default_app_scan_paths_contains_xdg_and_flatpak() {
        let paths = get_linux_default_app_scan_paths();
        assert!(!paths.is_empty());
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains("applications")));
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains("flatpak")));
    }

    #[test]
    fn test_is_bundle_visible_respects_freedesktop_rules() {
        let tmp = tempfile::tempdir().unwrap();

        let visible_app = tmp.path().join("visible.desktop");
        std::fs::write(
            &visible_app,
            "[Desktop Entry]\nType=Application\nName=Visible App\nExec=visible\n",
        )
        .unwrap();
        assert!(is_bundle_visible(&visible_app));

        let hidden_app = tmp.path().join("hidden.desktop");
        std::fs::write(
            &hidden_app,
            "[Desktop Entry]\nType=Application\nName=Hidden App\nNoDisplay=true\nExec=hidden\n",
        )
        .unwrap();
        assert!(!is_bundle_visible(&hidden_app));

        let deleted_app = tmp.path().join("deleted.desktop");
        std::fs::write(
            &deleted_app,
            "[Desktop Entry]\nType=Application\nName=Deleted App\nHidden=true\nExec=deleted\n",
        )
        .unwrap();
        assert!(!is_bundle_visible(&deleted_app));
    }

    #[test]
    fn test_localized_bundle_name_linux_desktop_file() {
        let tmp = tempfile::tempdir().unwrap();
        let app_path = tmp.path().join("sample.desktop");
        std::fs::write(
            &app_path,
            "[Desktop Entry]\nType=Application\nName=Default Name\nName[fr]=Nom Francais\nExec=sample\n",
        )
        .unwrap();

        let name = crate::platform::linux::localized_bundle_name(&app_path);
        // Returns some localized name or fallback
        assert!(name.is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_desktop_path_selects_gio_launch_route() {
        assert_eq!(
            application_launch_route(Path::new("/usr/share/applications/AmneziaVPN.desktop")),
            ApplicationLaunchRoute::GioDesktopEntry
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_non_desktop_path_selects_opener_route() {
        assert_eq!(
            application_launch_route(Path::new("/opt/example/bin/example")),
            ApplicationLaunchRoute::Opener
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_desktop_path_with_spaces_is_not_tokenized() {
        assert_eq!(
            application_launch_route(Path::new(
                "/home/example/My Applications/Example App.desktop"
            )),
            ApplicationLaunchRoute::GioDesktopEntry
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_desktop_entry_command_uses_host_gio_with_one_path_argument() {
        let path = Path::new("/home/example/My Applications/Example App.desktop");
        let command = linux_desktop_entry_command(path);

        assert_eq!(command.get_program(), "gio");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("launch"), path.as_os_str()]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nonexistent_linux_desktop_entry_returns_controlled_error() {
        let missing = Path::new("/definitely/missing/Example App.desktop");

        let error = launch_linux_desktop_entry(missing).unwrap_err();

        assert!(matches!(error, AppError::Platform(_)));
        assert!(error.to_string().contains("Example App.desktop"));
    }

    #[test]
    fn get_frontmost_application_returns_ok() {
        let result = get_frontmost_application();
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_linux_exe_name_follows_symlinks_and_preserves_wrapper_link_name() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        let opt_dir = tmp.path().join("opt");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&opt_dir).unwrap();

        let real_app = opt_dir.join("real-app-bin");
        std::fs::write(&real_app, "").unwrap();
        let abs_link = bin_dir.join("my-link");
        symlink(&real_app, &abs_link).unwrap();

        assert_eq!(
            resolve_linux_exe_name(abs_link.to_str().unwrap(), None),
            Some("real-app-bin".to_string())
        );

        let real_binary = opt_dir.join("real-binary");
        std::fs::write(&real_binary, "").unwrap();
        let path_link = bin_dir.join("bare-cmd");
        symlink(&real_binary, &path_link).unwrap();

        assert_eq!(
            resolve_linux_exe_name("bare-cmd", Some(bin_dir.as_os_str())),
            Some("real-binary".to_string())
        );

        let electron_bin = opt_dir.join("electron");
        std::fs::write(&electron_bin, "").unwrap();
        let app_link = bin_dir.join("my-electron-app");
        symlink(&electron_bin, &app_link).unwrap();

        assert_eq!(
            resolve_linux_exe_name(app_link.to_str().unwrap(), None),
            Some("my-electron-app".to_string())
        );

        // Flatpak commands run inside the sandbox and must not follow host PATH symlinks.
        let flatpak_entry = crate::platform::linux::DesktopEntry {
            exec: Some(
                "/usr/bin/flatpak run --command=/app/bin/bare-cmd org.example.App".to_string(),
            ),
            ..Default::default()
        };
        assert_eq!(
            resolve_linux_desktop_entry_exe_name(&flatpak_entry, Some(bin_dir.as_os_str())),
            Some("bare-cmd".to_string())
        );
    }
}
