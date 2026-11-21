//! OS platform abstractions for Asyar.
//!
//! Provides isolated, safe wrappers around platform-specific OS APIs for Linux
//! without dependencies on the desktop framework.

pub mod calendar;
pub mod error;
pub(crate) mod ics;
pub mod input;
pub mod keyboard;
pub mod locale;
pub mod media;
pub mod shortcuts;
pub mod window;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub mod linux_key_resolver;

pub use calendar::*;
pub use error::PlatformError;
pub use input::*;
pub use keyboard::*;
pub use locale::*;
pub use media::*;
pub use shortcuts::*;
pub use window::{ThemePreference, WindowStyling};

/// Result of extracting an application or file icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedIcon {
    /// A real icon extracted from the target binary, bundle, or icon resource.
    Icon(Vec<u8>),
    /// A generic shell placeholder returned while a target is missing or locked.
    Placeholder(Vec<u8>),
}

#[cfg(target_os = "linux")]
pub use linux::extract_icon;

/// Extracts an icon while distinguishing real icons from generic shell placeholders.
pub fn extract_icon_outcome(path: &std::path::Path) -> Option<ExtractedIcon> {
    extract_icon(path).map(ExtractedIcon::Icon)
}

#[cfg(target_os = "linux")]
pub use linux::localized_bundle_name;
