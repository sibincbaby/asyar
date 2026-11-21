# Linux-Only Platform Rule (X11 and Wayland)

Asyar targets **Linux only**, on both X11 and Wayland. There are no macOS or Windows code paths, `cfg` branches, CI jobs, or bundle configs in this repository. CI (`.github/workflows/test-and-lint.yml`) builds, lints, and tests on `ubuntu-latest`.

## 1. Platform Boundaries & Compilation

- **Crate & Module Split**: Put framework-free OS primitives in `src-tauri/crates/asyar-platform/` (`linux/`). Put Tauri-coupled OS code in `src-tauri/src/platform/` or trait-backed sibling modules (`power`, `system_actions`, `window_management`, `selection/platform`).
- **Linux Modules, Not Branches**: Platform code lives in Linux modules or behind `#[cfg(target_os = "linux")]`. Do not add `macos`, `windows`, or `#[cfg(not(target_os = "linux"))]` stubs, and do not reintroduce `cfg!(target_os = "macos" | "windows")` booleans.
- **Test Pure Logic Without a Desktop**: Keep parsers, classifiers, and policy helpers (`.desktop` parsing, AppImage environment sanitizing, MPRIS parsing) free of D-Bus, X11, and GTK calls so `cargo test --workspace` runs headless in CI.

## 2. Paths & Schemes

- **Paths**: Linux filesystems are case-sensitive; use `files_scope::starts_with_case_aware` for deny-list checks anyway so the shared helper stays the single owner of that logic. Reject `..` traversal in URI handlers.
- **Custom Schemes**: WebKitGTK serializes `asyar-extension://`, `asyar-icon://`, and `asyar-thumb://` as opaque `"null"` origins. Extension iframe `postMessage` therefore uses `'*'` as target origin (`getExtensionFrameOrigin()`); the security boundary is the iframe sandbox, the scheme handler, and the IPC permission gate.

## 3. Subprocesses, Webviews & Display Servers

- **Sanitize AppImage Children**: Call `crate::platform::linux::sanitize_command(&mut cmd)` before spawning external apps or `xdg-open` so AppImage `LD_LIBRARY_PATH` / `XDG_DATA_DIRS` overrides do not crash host binaries.
- **Use `tauri-plugin-http` from JS**: Webview/extension HTTP must go through `network:fetch` (`tauri-plugin-http`), never raw WebView `fetch()`.
- **Per-Webview `data-platform`**: Every separate webview (`main`, `settings`, `onboarding`, `hud`) sets `document.documentElement.dataset.platform = 'linux'`. Never set an opaque root background in component CSS.
- **Wayland Awareness**: Do not assume global hotkey grabs (`XGrabKey`), synchronous focus-on-show, window enumeration/positioning, or synthetic input/selection capture work on Wayland. Gate or degrade those paths explicitly.
- **Unsupported Features**: Where Linux cannot do something (for example screen-capture OCR), return a clear `AppError::Platform("... not supported on Linux")` rather than faking a result.

For full patterns, examples, and checklists, see `.agents/skills/cross-platform/SKILL.md`.
