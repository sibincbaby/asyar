---
order: 10
---

# Host OS Integration & Data Flow

## 8. OS Communication

Asyar relies strictly on Rust for handling raw Operating System tasks.

- **Global Hotkey Registration:** Managed via the `tauri_plugin_global_shortcut` crate. The core logic sits in `lib.rs:setup_global_shortcut()`.
  - **Conflict Handling:** If a shortcut registration fails (e.g., the user attempts to bind a key combination already reserved by the OS or another app), the `Err` is caught and logged to standard error (`eprintln!`).
  - **Wayland Compositor Summoning:** On Wayland, background global hotkey grabs are blocked by protocol security. Instead, the desktop compositor (Hyprland, Sway, GNOME, KDE) owns the keybinding and executes `asyar-summon`, a lightweight helper binary that invokes `org.asyar.Launcher1.Toggle` over session D-Bus (~3.5ms) and falls back to `asyar` single-instance CLI execution when the D-Bus service is unreachable. The `tauri_plugin_single_instance` plugin also intercepts direct `asyar` re-execution without a deep link and calls `toggle_launcher()`. A 250 ms blur grace period (`BLUR_HIDE_GRACE`) ignores spurious early `Focused(false)` events that land before the compositor grants window focus.
  - **User Experience:** Crucially, a hotkey conflict **does not crash the app**. Asyar continues its startup sequence and launches successfully. However, there is no automatic fallback hotkey assigned. The user will simply find the hotkey unresponsive and must use the System Tray icon to open the app and rebind the shortcut in settings.
- **System Tray (two independent code paths):**
  - **Asyar's own tray** lives in `src-tauri/src/tray.rs`. It owns a single fixed-menu `TrayIcon` (Settings / Quit). Extensions never write to it, and its menu never mutates at runtime.
  - **Extension trays** live in `src-tauri/src/extension_tray/` (Raycast `MenuBarExtra` model). Each top-level `IStatusBarItem` an extension registers becomes its own `TrayIcon` keyed by `(extension_id, item_id)`, stored in `ExtensionTrayManager`. `registerItem` / `updateItem` / `unregisterItem` correspond to Tauri commands `tray_register_item` / `tray_update_item` / `tray_unregister_item`. There is no debounce and no shared menu rebuild — every call is forwarded straight to the manager.
  - Clicks on extension menu items fire `asyar:tray-item-click` with payload `{extensionId, event: {itemPath, checked?}}`; `trayClickBridge` (in `src/services/statusBar/`) fans the event to the owning iframe as `asyar:event:statusBar:click` so the SDK's `StatusBarServiceProxy` can dispatch the registered `onClick`. Clicks on Asyar's own tray are handled inline in `tray.rs` and are never forwarded to extensions.
  - Extension uninstall / disable in `extensions/lifecycle.rs` calls `ExtensionTrayManager::remove_all_for_extension` so every tray icon belonging to that extension vanishes immediately.
  - See `docs/reference/sdk/status-bar.md` for the SDK-facing contract and the platform fidelity matrix.
- **Filesystem Access:** Leverages `tauri_plugin_fs`. Host paths strictly use Tauri's path resolution API (`appDataDir()`) to ensure compliance with each platform's data directory conventions.
- **Clipboard Access:** Leveraged via the `tauri_plugin_clipboard_manager`.
- **Locale & Internationalization (`crate::locale`):** Centralizes all BCP-47 and POSIX locale parsing, candidate resolution, and CLDR formatting inside Rust. It resolves localized application display names from Linux XDG `.desktop` entries using a **Language-First** hierarchy, while evaluating number formatting (decimal separators and grouping in the natural-language calculator) using a **Region-First** hierarchy. `LocaleService` is managed in Tauri state and dynamically refreshes when the host OS locale changes without requiring application restarts. See [Locale Subsystem & Internationalization](./locale-and-internationalization.md).
- **Selection Capture:** The `selection:read` capability (exposed to extensions through `SelectionService`) reads the user's current selection — text from the frontmost app or items from the frontmost file manager. The AT-SPI2 accessibility fast path is not implemented yet, so on Linux Asyar uses a clipboard-trick: snapshot → post Ctrl+C to the frontmost window (via `enigo`) → compare a content hash of the clipboard before and after for up to 250 ms → read → restore. Restore is RAII-guarded so any error path still puts the original clipboard contents back; only the text representation is snapshotted. File-manager items are read from a clipboard `text/uri-list`. A static `tokio::sync::Mutex` serialises selection operations across all extensions to prevent concurrent clipboard-trick races. Wayland is unsupported (same constraint as snippet expansion). The shared key-event posting helpers live in `src-tauri/src/platform/input.rs` and are used by both paste and copy code paths.
- **Window Management:** Asyar declares three windows in `tauri.conf.json`:
  - **`main` (the launcher)** behaves like a launcher search bar rather than a standard application window.
    - **Linux:** Window management follows Tauri's standard window APIs (a borderless, transparent, always-on-top window). Hide-on-focus-loss uses Tauri's window event system; no blur or vibrancy effect is applied and surfaces are opaque. On Wayland, clients cannot self-focus, so the 250 ms `BLUR_HIDE_GRACE` described above applies.
  - **`settings`** is a separate webview with its own JS context. It's loaded lazily and lives at `/settings`. Per-context singletons (auth, sync, feedback) are re-initialized inside `settings/+page.svelte`'s `onMount` because they live in a different JS realm than the main window's.
  - **`hud`** is a small transparent always-on-top webview at `/hud` used by the `FeedbackService.showHUD(...)` primitive. It is pre-declared with `visible: false` and is shown on demand by the `show_hud` Tauri command, which positions it at the bottom-center of the cursor's monitor (via the `monitor` toolkit crate), emits a `hud:show` event with the title, and schedules an auto-hide via a tracked `tauri::async_runtime::JoinHandle` stored in `HudState`. Lifecycle lives in `src-tauri/src/hud_window/service.rs`; the Tauri command `commands/hud.rs` is a thin wrapper. Because the HUD is its own window, it survives the main launcher window being hidden — that's the entire point of the primitive.

---

## 11. Data Flow Diagrams

### App Startup Sequence

```ascii
+------------+       +-------------+       +--------------+       +-------------------+       +-------+
| Tauri Init | ----> | Rust Setup  | ----> | WebView Load | ----> | SvelteKit Boots   | ----> | Ready |
+------------+       +-------------+       +--------------+       | (appInitializer)  |       +-------+
                                                                  +---------+---------+
                                                                            |
                                                                  +---------v---------+
                                                                  | ExtensionLoader   |
                                                                  | - glob Built-ins  |
                                                                  | - FS Installed    |
                                                                  +-------------------+
```

### Tier 1 Command Execution

```ascii
+------------+       +-------------------+       +-------------------------+
| User Input | ----> |   ResultsList     | ----> |  handleCommandAction()  |
+------------+       | (Select / Enter)  |       |  (extensionManager.ts)  |
                     +-------------------+       +-----------+-------------+
                                                             | navigateToView('ext_id/DefaultView')
+-------------------------+      +-------------------+       |
|    SvelteKit Reacts     | <--- | $activeView Store | <-----+
| (+page.svelte bounds)   |      |      Updates      |
+------------+------------+      +-------------------+
             | (isBuiltIn == true)
+------------v-------------+     +-------------------------+
| getLoadedExtensionModule | --> |  <svelte:component />   |
| (Extracts Svelte Class)  |     | (Mounts in Core DOM)    |
+--------------------------+     +-------------------------+
```

### Tier 2 Command Execution (Isolates)

```ascii
+------------+       +-------------------+       +-------------------------+
| User Input | ----> |   ResultsList     | ----> |  handleCommandAction()  |
+------------+       | (Select / Enter)  |       |  (extensionManager.ts)  |
                     +-------------------+       +-----------+-------------+
                                                             | navigateToView('ext_id/DefaultView')
+-------------------------+      +-------------------+       |
|    SvelteKit Reacts     | <--- | $activeView Store | <-----+
| (+page.svelte bounds)   |      |      Updates      |
+------------+------------+      +-------------------+
             | (isBuiltIn == false)
+------------v-------------+     +-------------------------+
|    <ExtensionIframe>     | --> | Rust URL Intercept      |
| src="asyar-extension://" |     | (stream from AppData)   |
+------------+-------------+     +-----------+-------------+
                                             |
+--------------------------+     +-----------v-------------+
| Extension Context Boots  | <-- |   index.html Injected   |
| (IPC & MessageBroker)    |     |   (Isolated Webview)    |
+--------------------------+     +-------------------------+
```

---
