# asyar-gpui (spike)

A native GPUI launcher shell for Linux, built to measure whether a GPU-rendered
Rust UI can replace the Tauri + WebKitGTK frontend. It reuses Asyar's
`.desktop` parser (`asyar-platform`) and ranker (`asyar-search`), so only the UI
is new.

UI stack: [`gpui-ce`](https://github.com/gpui-ce/gpui-ce) 0.3.0, a community
fork of Zed's GPUI. Upstream `gpui` 0.2.2 on crates.io has no Wayland
layer-shell yet.

## Run

```bash
cargo run --release              # starts the daemon and shows the window
./target/release/asyar-gpui      # again: toggles the window (bind to a hotkey)
ASYAR_GPUI_SYNTHETIC=10000 cargo run --release   # pad with 10k rows to stress typing
```

Keys: type to search, Up/Down to select, Enter to launch, Escape or focus loss
to hide. Stop the daemon with `pkill -x asyar-gpui`.

## Measured (2026-11-21)

Dell Latitude 7430 (Intel Iris Xe, Mesa Vulkan), Linux Mint Cinnamon, X11,
2560×1440 at scale 2, 120 installed apps.

| Metric                              | Result                                     |
| ----------------------------------- | ------------------------------------------ |
| App index (120 `.desktop` files)    | 10 ms                                      |
| Cold start → first frame            | 221 ms (1.57 s with a cold shader cache)   |
| Summon → first frame (parked)       | p50 18.9 ms, p95 26.4 ms (n=20)            |
| Summon focus success                | 20/20                                      |
| Keystroke → echo, 10k rows          | p50 1.7 ms, p95 13.6 ms, max 16.2 ms       |
| Keystroke → results, 10k rows       | p50 19.4 ms, p95 41.7 ms (ranker-bound)    |
| Ranking, 120 apps                   | ~0.2 ms                                    |
| Memory, hidden daemon               | 65 MB PSS / 115 MB RSS (one process)       |
| Memory, window shown                | 83 MB PSS / 135 MB RSS                     |
| Idle CPU, hidden (60 s)             | 0.016 %, 2 wakeups/s                       |
| Binary (stripped)                   | 31 MB; links no GTK or WebKit              |

For context, the README's Tauri numbers (macOS, all processes) are 419 MB idle
memory and 0.64 % idle CPU. The Tauri build has not been measured on this
machine.

## Findings

- Re-creating the window per summon costs ~110 ms (new GPU surface); parking it
  with `Window::set_visible` gives ~19 ms.
- The shared `asyar-search` ranker takes 25–50 ms for multi-word queries over
  10k rows. It runs on a background thread here so typing never waits on it.
- `gpui-ce`'s X11 backend sets no position hint, so the window manager picks the
  placement and `Bounds::centered` is ignored.
- Wayland layer-shell is wired up (`WindowKind::LayerShell`, falling back to a
  normal window on GNOME) but not yet tested on a Wayland session.

## Not covered

App icons, tray, a native `Exec=` launcher (uses `gio launch`), extensions,
accessibility, and machines without Vulkan.
