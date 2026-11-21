//! File-preview thumbnail generation, cached and served the same way
//! application icons already are (`asyar-icon://` → `icon_cache/`): a
//! content-addressed PNG on disk, served via a dedicated custom URI scheme
//! (`asyar-thumb://` → `thumbnail_cache/`).
//!
//! Unlike the JS-side preview it replaces, none of this is subject to the
//! webview's `@tauri-apps/plugin-fs` capability scope — Rust reads the
//! source file directly, so a path outside the declared fs-scope (which
//! only ever covered `$APPDATA/extensions/**` and
//! `$APPDATA/clipboard_cache/**`, never arbitrary `$HOME` paths) still
//! works.
//!
//! Images are downscaled with the `image` crate. Linux gets image-only
//! thumbnails in this pass; other types fall back to the frontend's
//! existing type/metadata display (documented follow-up: native thumbnail
//! providers).

pub mod cache;
pub mod commands;
pub mod image_thumb;

#[cfg(target_os = "linux")]
pub mod linux;

use std::path::{Path, PathBuf};

/// Caps concurrent thumbnail generations (image decode or
/// provider subprocess) so opening a folder full of never-before-seen large
/// files doesn't spawn dozens of decoders/processes at once.
const MAX_CONCURRENT_GENERATIONS: usize = 3;

pub struct ThumbnailState {
    semaphore: tokio::sync::Semaphore,
}

impl Default for ThumbnailState {
    fn default() -> Self {
        Self {
            semaphore: tokio::sync::Semaphore::new(MAX_CONCURRENT_GENERATIONS),
        }
    }
}

/// Returns a cached or freshly-generated thumbnail's absolute path, or
/// `None` when this file type/platform combination has no thumbnail
/// strategy (the frontend keeps its existing fallback in that case).
pub async fn get_or_generate(
    state: &ThumbnailState,
    cache_dir: &Path,
    path: &Path,
    max_dim: u32,
    priority: bool,
) -> Option<PathBuf> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = cache::cache_key(path, mtime, meta.len(), max_dim);
    let cached = cache_dir.join(&key);
    if tokio::fs::metadata(&cached).await.is_ok() {
        return Some(cached);
    }

    // `priority` (the one file the user is looking at) skips the queue:
    // otherwise it waits behind every row thumbnail of a freshly opened list.
    let _permit = if priority {
        None
    } else {
        Some(state.semaphore.acquire().await.ok()?)
    };
    // Re-check after acquiring the permit — a concurrent request for the
    // same file may have generated it while we waited.
    if tokio::fs::metadata(&cached).await.is_ok() {
        return Some(cached);
    }

    let _ = tokio::fs::create_dir_all(cache_dir).await;
    generate(path, &cached, max_dim).await?;
    cache::evict_if_over_cap(cache_dir, cache::CACHE_CAP_BYTES);
    Some(cached)
}

/// Returns the cached path of `path`'s native OS icon (Finder / Explorer
/// file-type icon), extracting it on first request. `None` when the
/// platform has no per-file icon source — the frontend keeps its SVG fallback.
pub async fn get_or_extract_icon(cache_dir: &Path, path: &Path) -> Option<PathBuf> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let cached = cache_dir.join(cache::icon_cache_key(path, meta.is_dir()));
    if tokio::fs::metadata(&cached).await.is_ok() {
        return Some(cached);
    }
    let src = path.to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || extract_native_icon(&src))
        .await
        .ok()??;
    let _ = tokio::fs::create_dir_all(cache_dir).await;
    write_icon_atomically(cache_dir, &cached, bytes)
        .await
        .then_some(cached)
}

async fn write_icon_atomically(cache_dir: &Path, cached: &Path, bytes: Vec<u8>) -> bool {
    let tmp = cache_dir.join(format!(
        ".tmp-icon-{}-{:x}.png",
        std::process::id(),
        uuid::Uuid::new_v4().as_u128()
    ));
    if tokio::fs::write(&tmp, bytes).await.is_err() {
        return false;
    }
    if tokio::fs::rename(&tmp, cached).await.is_ok() {
        return true;
    }

    // A racing writer may have won between the cache check and rename,
    // which is still success.
    let published = tokio::fs::metadata(cached).await.is_ok();
    let _ = tokio::fs::remove_file(&tmp).await;
    published
}

// Linux `extract_icon` only resolves `.desktop` entries; there's no generic
// file-type icon lookup yet.
#[cfg(target_os = "linux")]
fn extract_native_icon(_path: &Path) -> Option<Vec<u8>> {
    None
}

/// Maps a cached thumbnail path to the URL the webview loads it from —
/// `asyar-thumb://localhost/`. Shared by the host preview command and the extension-gated
/// `files_thumbnail` so both hand out identical URLs.
pub fn thumb_url(cached_path: &Path) -> Result<String, String> {
    let filename = cached_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "generated thumbnail path has no file name".to_string())?;

    Ok(format!("asyar-thumb://localhost/{filename}"))
}

async fn generate(path: &Path, dest: &Path, max_dim: u32) -> Option<()> {
    let path = path.to_path_buf();
    let dest_owned = dest.to_path_buf();
    let is_image = image_thumb::is_supported_image(&path);

    tokio::task::spawn_blocking(move || {
        if is_image {
            return image_thumb::generate(&path, &dest_owned, max_dim).ok();
        }
        linux::generate_via_quicklook(&path, &dest_owned, max_dim).ok()
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod atomic_write_tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_icon_writes_publish_one_complete_file_without_temp_leaks() {
        let dir = std::env::temp_dir().join(format!(
            "atomic_icon_write_test_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cached = dir.join("icon.png");

        let (first, second) = tokio::join!(
            write_icon_atomically(&dir, &cached, vec![1; 4096]),
            write_icon_atomically(&dir, &cached, vec![2; 4096])
        );

        assert!(first || second);
        let bytes = std::fs::read(&cached).unwrap();
        assert!(bytes == vec![1; 4096] || bytes == vec![2; 4096]);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod priority_tests {
    use super::*;

    #[tokio::test]
    async fn priority_request_does_not_wait_for_a_saturated_queue() {
        let dir = std::env::temp_dir().join(format!("thumb_priority_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.png");
        image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4))
            .save_with_format(&src, image::ImageFormat::Png)
            .unwrap();

        let state = ThumbnailState::default();
        let _held = state
            .semaphore
            .acquire_many(MAX_CONCURRENT_GENERATIONS as u32)
            .await
            .unwrap();
        let cache = dir.join("cache");
        let wait = std::time::Duration::from_secs(5);

        let queued = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            get_or_generate(&state, &cache, &src, 64, false),
        )
        .await;
        assert!(queued.is_err(), "non-priority request must queue");
        let got = tokio::time::timeout(wait, get_or_generate(&state, &cache, &src, 64, true))
            .await
            .expect("priority request must not queue");
        assert!(got.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
