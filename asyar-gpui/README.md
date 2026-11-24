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

## Install alongside the Tauri app (Cinnamon)

This is how the preview was set up on the development machine (Linux Mint
Cinnamon, X11), next to the installed Tauri app, which keeps Alt+Space.

```bash
cargo build --release
install -Dm755 target/release/asyar-gpui ~/.local/bin/asyar-gpui
strip ~/.local/bin/asyar-gpui
```

A menu entry at `~/.local/share/applications/asyar-gpui.desktop` runs
`~/.local/bin/asyar-gpui`. It does not start at login; launch it once from the
menu after a reboot.

Super+Space is bound as a Cinnamon custom shortcut (`custom3`). Cinnamon
ships Super+Space as "switch input source", so that action was reduced to the
keyboard's layout key first:

```bash
gsettings set org.cinnamon.desktop.keybindings.wm switch-input-source "['XF86Keyboard']"
gsettings set org.cinnamon.desktop.keybindings.wm switch-input-source-backward "['<Shift>XF86Keyboard']"
K=org.cinnamon.desktop.keybindings.custom-keybinding:/org/cinnamon/desktop/keybindings/custom-keybindings/custom3/
gsettings set $K name 'Asyar (GPUI)'
gsettings set $K command "$HOME/.local/bin/asyar-gpui"
gsettings set $K binding "['<Super>space']"
gsettings set org.cinnamon.desktop.keybindings custom-list "['custom0', 'custom1', 'custom2', 'custom3']"
```

### Undo

Restore Super+Space to input-source switching:

```bash
gsettings set org.cinnamon.desktop.keybindings.wm switch-input-source "['<Super>space', 'XF86Keyboard']"
gsettings set org.cinnamon.desktop.keybindings.wm switch-input-source-backward "['<Shift><Super>space', '<Shift>XF86Keyboard']"
```

Remove the shortcut (on that machine `custom0`–`custom2` are other shortcuts,
so keep them in the list):

```bash
gsettings set org.cinnamon.desktop.keybindings custom-list "['custom0', 'custom1', 'custom2']"
gsettings reset-recursively org.cinnamon.desktop.keybindings.custom-keybinding:/org/cinnamon/desktop/keybindings/custom-keybindings/custom3/
```

Remove the launcher itself:

```bash
pkill -x asyar-gpui
rm ~/.local/bin/asyar-gpui ~/.local/share/applications/asyar-gpui.desktop
```

## Measured (2026-11-21)

Dell Latitude 7430 (Intel Iris Xe, Mesa Vulkan), Linux Mint Cinnamon, X11,
2560×1440 at scale 2, 120 installed apps.

| Metric                           | Result                                   |
| -------------------------------- | ---------------------------------------- |
| App index (120 `.desktop` files) | 10 ms                                    |
| Cold start → first frame         | 221 ms (1.57 s with a cold shader cache) |
| Summon → first frame (parked)    | p50 18.9 ms, p95 26.4 ms (n=20)          |
| Summon focus success             | 20/20                                    |
| Keystroke → echo, 10k rows       | p50 1.7 ms, p95 13.6 ms, max 16.2 ms     |
| Keystroke → results, 10k rows    | p50 19.4 ms, p95 41.7 ms (ranker-bound)  |
| Ranking, 120 apps                | ~0.2 ms                                  |
| Memory, hidden daemon            | 65 MB PSS / 115 MB RSS (one process)     |
| Memory, window shown             | 83 MB PSS / 135 MB RSS                   |
| Idle CPU, hidden (60 s)          | 0.016 %, 2 wakeups/s                     |
| Binary (stripped)                | 31 MB; links no GTK or WebKit            |

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

## Features

- App search over `.desktop` entries with the shared `asyar-search` ranker.
- App icons from the user's icon theme (GTK `settings.ini`, else the
  Cinnamon/GNOME gsettings key, else `hicolor`), resolved on a background
  thread after the first frame (about 105 ms for 127 apps with Papirus).
- Inline calculator answers from the shared `asyar-calculator` crate, shown
  above app results; Enter copies the answer.
- Reads the Tauri app's settings (read-only, reloaded on every summon): the
  theme (`light`, `dark` or `system`) from `settings.dat`, and the calculator's
  number format and preferred currency from `asyar_data.db`. Both live in
  `$XDG_DATA_HOME/org.asyar`.
- Currency conversion with the Tauri app's rates cache
  (`calculator/rates.json`), refreshed in the background when older than the
  calculator's refresh interval; queries never wait on the network.
- Centers the launcher on the monitor under the pointer on X11 (gpui-ce sets
  no position hint, so the window manager would otherwise place it).
- Glass look after macOS Liquid Glass: a translucent tint over a blurred
  backdrop, a bright rim, a top sheen and a soft shadow, in light and dark.
  KDE Plasma on Wayland blurs through the compositor. On X11, where window
  managers like Cinnamon offer no blur, the launcher captures what sits behind
  it just before showing, blurs a downscaled copy (about 22 ms per summon at
  1360×880) and draws that; it is a still snapshot. Without a compositor the
  panel is drawn opaque with no shadow.

## Plugins

A plugin is a CLI that describes its own commands. Drop one manifest per
plugin into `~/.config/asyar-gpui/plugins/`:

```json
{ "name": "tool", "program": "/home/me/.local/bin/mytool" }
```

`program` must be an absolute path (a hotkey-started daemon has a minimal
`PATH`). Changes are picked up on the next summon: adding or deleting a
manifest, or rebuilding the program.

The contract:

- `<program> commands --json` prints a catalog:
  `{"data": {"commands": [{"name": "issue list", "summary": "…", "safety": "read", "args": [{"name": "<id>", "required": true}]}]}}`.
- Every `read` command with no required arguments becomes a launcher command
  (`tool issue list`). Commands that write are not offered yet.
- Running one calls `<program> <words…> --json` with no shell and a 30 s
  timeout, and expects `{"ok": true, "data": …}` or
  `{"ok": false, "error": {"code", "message"}}` on stdout.
- The first array in `data` (preferably `items` or `results`) becomes the
  list. Rows take `title`, `subject` or `name` as the title, a short `id` and
  `status`, `assignee`, `project`… as the subtitle, and `url` as the link.
  Only `http(s)` links open; Enter copies the title otherwise. An object with
  no array is shown as key/value rows.

In a plugin view, typing filters the loaded rows in memory, and Esc goes back.
The last result shows at once while a fresh run loads.

## Not covered

Tray, a native `Exec=` launcher (uses `gio launch`), extensions,
accessibility, and machines without Vulkan. See [ROADMAP.md](ROADMAP.md) for
the migration plan.
