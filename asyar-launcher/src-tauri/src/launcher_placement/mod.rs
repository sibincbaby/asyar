//! Where the launcher window appears, and how the user changes it.
//!
//! Before #596 the position was recomputed on every reveal from a formula
//! hardcoded in two platform-specific places, so there was nothing to drag and
//! nothing to configure. This module owns that decision instead:
//!
//! ```text
//! reveal ─▶ service::apply()
//!            ├─ store::load()        the persisted LauncherPlacement
//!            ├─ pick_monitor()       cursor or primary
//!            └─ resolve::resolve_origin()   PURE — the only copy of the maths
//! ```
//!
//! Dragging goes the other way: [`crate::window_drag`] moves the window, and
//! on drop [`service::persist_dragged`] turns the landing spot back into
//! monitor fractions.

pub mod classify;
pub mod commands;
pub mod resolve;
pub mod service;
pub mod store;
pub mod types;

pub use types::{LauncherAnchor, LauncherMonitorChoice, LauncherPlacement, DEFAULT_TOP_BIAS};

/// The tallest the launcher ever gets. Positions reserve this much room below
/// the top edge so an expanded launcher is never clipped — see
/// [`resolve::resolve_origin`].
pub const LAUNCHER_MAX_HEIGHT: f64 = 480.0;
