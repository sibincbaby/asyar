---
name: cross-platform
description: Use when writing or modifying OS-specific Rust code, paths, subprocesses, custom URI schemes, webview CSS/origins, keyboard shortcuts, or platform-gated features for Linux (X11/Wayland) in Asyar.
---

# Linux Platform Rules in Asyar

## Overview

Asyar supports **Linux only**, on **X11 and Wayland**. There are no macOS or Windows targets. The platform surface to get right is the split between X11 (everything works) and Wayland (the compositor restricts global hotkeys, window control, and input injection), plus AppImage / `.deb` packaging and WebKitGTK rendering.

## When to Use

- Adding or editing code behind `#[cfg(target_os = "linux")]` or `#[cfg(unix)]`
- Working in `asyar-platform`, `src-tauri/src/platform/`, `window_management`, `selection`, `power`, `system_actions`, `system_events`, or `shell`
- Manipulating filesystem paths, `canonicalize()`, path scopes, or URI schemes (`asyar-extension://`, `asyar-icon://`, `asyar-thumb://`)
- Spawning child processes, openers, or sidecars
- Adding webviews, `data-platform` CSS rules, `postMessage` origin checks, or HTTP requests from JS
- Handling global shortcuts, window focus/blur, or keyboard layout resolution

Do **not** load this skill for pure domain logic with no OS, path, window, process, or webview interaction.

## What NOT to Do

- Do not add `macos` / `windows` modules, `#[cfg(target_os = "macos" | "windows")]` branches, `#[cfg(not(target_os = "linux"))]` stubs, or `cfg!(target_os = ...)` booleans.
- Do not add per-OS wording, icons, or permission UI (accessibility, calendar, dock icon, Finder/Explorer labels). Use Linux terms: "file manager", "trash", "Log Out".
- Do not add per-OS CI jobs, bundle config (`.icns`, `.ico`, entitlements, `Info.plist`), or package-manager channels (Homebrew, winget).

---

## 1. Rust Platform Architecture & Testability

### Crate Boundaries

| Layer                  | Location                                                             | Responsibility                                                                                                                                       |
| ---------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Framework-free OS APIs | `asyar-launcher/src-tauri/crates/asyar-platform/src/`                | Native X11/Wayland/D-Bus wrappers, `ics`, `media`, `calendar`, `shortcuts`, `locale`, `linux_key_resolver`, `linux/` helpers. Zero Tauri dependency. |
| Tauri-coupled OS code  | `asyar-launcher/src-tauri/src/platform/`                             | GTK window handles, Linux D-Bus launcher service (`launcher_dbus`), desktop-entry and environment helpers.                                           |
| Trait-backed features  | `power`, `system_actions`, `window_management`, `selection/platform` | Define a platform-neutral state + trait (`PowerBackend`, `SystemActionsBackend`) with a `linux` sibling module.                                      |

### Keep Logic Testable Headless

Keep parsing, classification, and decision logic free of D-Bus/X11/GTK calls and pass OS-specific inputs as parameters. `cargo test --workspace` runs on a headless `ubuntu-latest` runner.

- `.desktop` parsing, AppImage `calculate_sanitized_environment`, and MPRIS parsing are pure functions tested without a session bus.
- `src-tauri/src/lib.rs` extracts `linux_webkit_dmabuf_env_var(is_linux, user_provided, opt_out)` so both branches are unit-tested.

### Lint Discipline

Never put an unused import or helper outside its matching `#[cfg(...)]` block — `cargo clippy --workspace --all-targets -- -D warnings` fails on dead-code/unused-import warnings.

---

## 2. Paths, Filesystems & URI Schemes

### Case-Aware Filesystem Checks

Use `files_scope::starts_with_case_aware(&path, &root)` for deny-list checks so the shared helper stays the single owner of that policy. For allow-list containment, verify the resolved path's relationship to the allowed root; do not rely on this helper alone.

- Reject `..` traversal in flat-cache URI handlers (`uri_schemes.rs::read_cache_file`).
- Keep custom URL schemes at least 2 characters long (`opener_scope::validate_declared_scheme`).
- Use the `file-id` crate (`file_index/file_id.rs`) for stable file identity.

### Custom Schemes on WebKitGTK

WebKitGTK serializes custom Tauri schemes (`asyar-extension://`, `asyar-icon://`, `asyar-thumb://`) as opaque `"null"` origins.

- **Rust scheme handlers (`uri_schemes.rs::scheme_path`)**: Accept `{scheme}://localhost/...` and `{scheme}://...` forms.
- **Extension iframe origins (`lib/ipc/extensionOrigin.ts`)**: `getExtensionFrameOrigin()` returns `'*'` because `postMessage(msg, 'asyar-extension://...')` throws against an opaque origin. The security boundary is the iframe sandbox, the scheme handler, and the `ExtensionIpcRouter` permission gate.

---

## 3. Subprocesses, HTTP Stacks & Native Libraries

| Concern                | Linux behaviour                                                                                                                                                                                                         |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AppImage env leak      | Call `crate::platform::linux::sanitize_command(&mut cmd)` before spawning external apps. Restores `*_ORIG` env vars (`LD_LIBRARY_PATH`, `XDG_DATA_DIRS`, `QT_PLUGIN_PATH`) (`asyar-platform/src/linux/environment.rs`). |
| `PATH` lookup          | `which`                                                                                                                                                                                                                 |
| Webview/extension HTTP | `tauri-plugin-http` (`network:fetch`). Never unify `reqwest` (host) with `tauri-plugin-http`.                                                                                                                           |
| Keychain / Secrets     | freedesktop Secret Service -> `FileKeyStore` fallback. `crypto/keystore.rs` degrades to a `0600` `keystore-v1.dat` on headless/Secret-Service-free systems.                                                             |
| Native static libs     | Pin static features where a system library would otherwise leak into release binaries (e.g., `liblzma-sys = { features = ["static"] }`).                                                                                |

---

## 4. Webviews, CSS & Window Lifecycle

### Multi-Webview `data-platform` & Styling

Asyar has multiple independent webviews (`main`, `settings`, `onboarding`, `hud`, `sticky`). Each owns a separate DOM and JS realm:

- **Set `data-platform` in every shell**: `AppShell.svelte`, `routes/settings/+page.svelte`, `routes/onboarding/+layout.svelte`, and `routes/hud/+page.svelte` each set `document.documentElement.dataset.platform = 'linux'`.
- **Never set an opaque root background**: The platform layer owns window composition (opaque surfaces on Linux). See `.agents/skills/design-language/SKILL.md`.

### Window Show / Hide / Focus

- **Wayland**: Clients cannot self-focus; `set_focus()` sends an `xdg-activation` request that may be delayed or ignored (`input:follow_mouse`). `lib.rs` applies `BLUR_HIDE_GRACE` (`250 ms`) via `blur_hide_is_spurious` so premature `Focused(false)` events do not immediately hide the launcher.
- **Quitting**: Closing a window must never quit Asyar (`RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit()` in `lib.rs`); only explicit `app.exit(0)` paths quit.

---

## 5. Linux Wayland, X11 & WebKitGTK

- **Detect Wayland consistently**: Check `WAYLAND_DISPLAY` non-empty or `XDG_SESSION_TYPE` case-insensitively equal to `"wayland"` (`commands/shortcuts.rs`).
- **Global Shortcuts**: X11 `XGrabKey` / `tauri_plugin_global_shortcut` fails on Wayland. Wayland compositors invoke the `asyar-summon` sidecar (`asyar-launcher/src-tauri/asyar-summon`), which calls `org.asyar.Launcher1.Toggle` over D-Bus (`platform/linux/launcher_dbus.rs`) with single-instance CLI fallback.
- **Input Simulation & Selection Capture**: Snippet expansion and `selection:read` clipboard-trick fallback work on X11 and are unsupported on Wayland.
- **WebKitGTK Rendering**: `lib.rs::apply_linux_webkit_dmabuf_workaround()` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before webview creation unless user-set or `ASYAR_ALLOW_WEBKIT_DMABUF=1`. AppImage builds also install a Fontconfig rule hiding COLRv1 fonts from FreeType ABI mismatches.
- **Packaging**: Linux-specific Tauri config lives in `tauri.linux.conf.json` (bundles `asyar-summon`, declares the `libxdo3` `.deb` dependency). Releases publish an AppImage and a `.deb` per architecture (amd64, arm64).
- **Unsupported on Linux**: Screen-capture OCR (`ocr/linux.rs`) returns "not yet supported on Linux". Say so plainly in docs and errors; do not delete the API.

---

## 6. Shortcuts & Input

- **Canonical Action Shortcuts**: Use `Mod[+Alt][+Shift]+Key` (`lib/keyboard/actionShortcut.ts`) where `Mod` renders and binds as `Ctrl`. Never use raw `Super`, `Ctrl`, or glyphs in action declarations.
- **Keyboard Layouts**: Never assume US QWERTY for global key hooks; route through `linux_key_resolver` in `asyar-platform`.
- **Linux Terminology**: Use "file manager" (not Finder/Explorer), "Trash", and "Log Out" in UI strings and docs.

---

## 7. CI Verification

`.github/workflows/test-and-lint.yml` runs on `ubuntu-latest`:

| Job               | What It Catches                                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `frontend`        | Frontend + SDK tests, Prettier, `check:design`, `svelte-kit sync` + `check`.                                           |
| `test-rust-linux` | Rust formatting, Clippy (`-D warnings`), `cargo doc`, `cargo test --workspace`, Secret Service + `FileKeyStore` tests. |
| `test-and-lint`   | Aggregate gate across the jobs above.                                                                                  |

Before concluding a platform change:

1. Verify the Linux module still matches the trait/function signature it implements.
2. Run `pnpm check:ci` (or `pnpm check:ci:changed`) locally.
