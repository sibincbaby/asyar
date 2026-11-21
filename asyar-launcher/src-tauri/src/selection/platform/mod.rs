#[cfg(any(target_os = "linux", test))]
pub mod arboard_guard;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
