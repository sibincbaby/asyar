//! Platform-specific abstractions.
//!
//! Each sub-module exposes safe wrappers around OS-level APIs,
//! with `// SAFETY:` comments on every underlying `unsafe` block.

pub mod input;
pub mod linux;
#[cfg(target_os = "linux")]
pub mod linux_key_resolver;

pub use asyar_platform::{extract_icon_outcome, ExtractedIcon};

// Re-export the platform icon extractor under a unified name
#[cfg(target_os = "linux")]
pub use linux::extract_icon;

// Likewise for the localized bundle display name.
#[cfg(target_os = "linux")]
pub use linux::localized_bundle_name;
