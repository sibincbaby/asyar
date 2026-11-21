//! OS-level activation of the browser application that owns a tab.
//!
//! Only macOS needs this (Arc reimplements window management, so the
//! companion's focus call never raises it); on Linux [`activate`] is a no-op.

use crate::browser::types::BrowserKey;

/// Best-effort OS-level activation of the browser owning a tab. A no-op on Linux.
pub fn activate(key: &BrowserKey) {
    let _ = key;
}
