//! Spike: a native GPUI launcher shell for Linux.
//!
//! Measures whether a GPU-rendered Rust UI beats the Tauri/WebKitGTK launcher on
//! idle memory, summon latency and typing latency. It reuses Asyar's own
//! `.desktop` parser and the `asyar-search` ranker, so only the UI is new.
//!
//! Run `asyar-gpui` once to start the daemon; run it again (e.g. from a
//! compositor keybinding) to toggle the window.

mod apps;
mod launcher;

// shortcut: compiled in by path to avoid linking GTK3 through asyar-platform;
// move these two files into a GTK-free crate if the spike is adopted.
#[path = "../../asyar-launcher/src-tauri/crates/asyar-platform/src/linux/desktop_entry.rs"]
#[allow(dead_code, clippy::collapsible_if)]
mod desktop_entry;
#[path = "../../asyar-launcher/src-tauri/crates/asyar-platform/src/locale.rs"]
#[allow(dead_code, clippy::collapsible_if)]
mod locale;

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("asyar-gpui.sock")
}

fn main() {
    let path = socket_path();
    // A running daemon owns the socket: forward a toggle and exit. Binding this
    // binary to a compositor shortcut is the global hotkey on X11 and every
    // Wayland compositor, with no input grabbing.
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"toggle");
        return;
    }
    let _ = std::fs::remove_file(&path); // stale socket left by a crashed daemon
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("asyar-gpui: cannot bind {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    launcher::run(listener);
}
