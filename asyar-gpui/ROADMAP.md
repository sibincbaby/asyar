# Native GPUI UI: migration plan

Replace the Tauri + Svelte + WebKitGTK frontend with a native GPUI shell,
without breaking the extensions already published in the Store.

## Decisions

- **UI toolkit:** [`gpui-kit`](https://github.com/longbridge/gpui-kit) 0.11 on
  `gpui-pre` (a Zed GPUI snapshot), for its ready native components. It
  replaced `gpui-ce`; upgrades are deliberate because GPUI is pre-1.0.
- **Extensions: hybrid, webview on demand.** GPUI draws the launcher, every
  built-in, and the host-rendered parts of extensions (search results,
  actions, preferences, arguments). Extension workers and `role: 'view'`
  iframes keep running in WebKitGTK, but only in a separate extension-host
  process that starts when an installed extension needs to run and can exit
  when none does. With no extension running, Asyar links and loads no
  WebKit at all. The extension contract (manifest, SDK, IPC shapes) does not
  change.
- **Strangler migration.** The Tauri app stays the shipping product until the
  GPUI shell reaches parity. Both read the same data directory
  (`~/.local/share/org.asyar`), so only one of them may run at a time.

## Target architecture

```
asyar-gpui  (GPUI shell, always running)
  ├─ launcher window, built-in views, settings
  ├─ asyar-core  (Tauri-free domain logic, shared with the Tauri app meanwhile)
  │    └─ asyar-storage, asyar-search, asyar-calculator, asyar-platform
  └─ extension-host client ──Unix socket, JSON──▶ asyar --extension-host
                                                   (WebKitGTK, on demand:
                                                    workers + view windows)
```

## Milestones

1. **Shell daily driver (in progress).** Apps with icons, inline calculator
   results, launch, summon/dismiss, keyboard navigation. Builds on the spike's
   parked window, background ranking and socket summon.
2. **Extract `asyar-core`.** Move domain modules out of `src-tauri` into a
   Tauri-free crate behind two small seams: an event sink (instead of
   `AppHandle::emit`) and app paths (instead of Tauri's path resolver). The
   Tauri app consumes the crate, so its 4k+ tests keep guarding behaviour.
   Most modules touch Tauri in only 1–4 files; the 21k-line `commands/`
   layer stays Tauri-only glue.
3. **Port built-ins** one at a time in order of daily use: clipboard history,
   snippets (Wayland expansion via uinput), file search, notes, window
   management, the rest.
4. **Settings UI** in GPUI, reading and writing the same `settings.dat`.
5. **Extension host.** Run the existing TypeScript extension runtime (router,
   service registry, workers, view iframes) in the Tauri binary, started on
   demand as `asyar --extension-host`. Define the GPUI ↔ host socket protocol
   for results, actions, view requests and service calls.
6. **Wayland.** Layer-shell launcher (wired, untested), GlobalShortcuts
   portal with the `asyar-gpui` socket toggle as fallback, data-control
   clipboard.
7. **Retire the Svelte launcher.** The Tauri binary shrinks to the extension
   host; packaging ships `asyar-gpui` as the main executable.

## Known risks

- GPUI's X11 backend sets no window position hint, so the window manager
  places the launcher (worked around in `x11_place.rs`; upstream fix needed).
- GPUI has no window show/hide: X11 unmaps the window directly; Wayland
  re-creates it per summon (~100 ms) until GPUI gains one.
- Accessibility (AccessKit) and IME input need checking per milestone.
- Machines without Vulkan are not supported by GPUI's Linux renderer.
