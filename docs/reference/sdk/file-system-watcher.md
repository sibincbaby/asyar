### 8.29 `FileSystemWatcherService` — Watch directories for changes

**Runs in:** worker. The `fsWatcher` proxy is wired only into the worker
ExtensionContext (`asyar-sdk/worker`); the view ExtensionContext does not
expose it. Importing `IFileSystemWatcherService` from `asyar-sdk/contracts`
in view code is allowed for type positions, but `context.getService('fsWatcher')`
in a view-side `ExtensionContext` throws — the watch handle and its push
callback must live in the always-on headless worker (Web Worker) so events keep arriving
after the launcher panel is dismissed.

**Permission required:** `fs:watch`, plus a matching `permissionArgs["fs:watch"]` array of glob patterns in the manifest. The pattern list is the **scope** of what an extension is allowed to watch; calls to `watch(paths)` that escape every declared pattern are rejected at the host boundary with a permission error.

Use this to react to user-initiated changes on the filesystem that happen **outside** Asyar — SSH config tweaks, dotfile refreshes, package list updates, and so on. The host exposes a tiny, stable shape (`{ type: 'change', paths: string[] }`) regardless of which OS-native backend produced the raw event (inotify on Linux).

```typescript
interface FileSystemWatcherOptions {
  /** Default: true */
  recursive?: boolean;
  /** Default: 500. Clamped by the host to [50, 5000] ms. */
  debounceMs?: number;
}

interface FileSystemChangeEvent {
  type: 'change';
  /** Subset of the handle's watched roots that saw activity this tick.
   *  Roots-up coalesced — if 50 files under one root change, the root
   *  appears once. Extensions that don't care about specificity can
   *  ignore this and just re-fetch their source. */
  paths: string[];
}

interface WatcherHandle {
  onChange(cb: (e: FileSystemChangeEvent) => void): () => void;
  dispose(): Promise<void>;
}

interface IFileSystemWatcherService {
  watch(paths: string[], opts?: FileSystemWatcherOptions): Promise<WatcherHandle>;
}
```

**Manifest declaration:**

```json
{
  "permissions": ["fs:watch"],
  "permissionArgs": {
    "fs:watch": ["~/Library/Shortcuts/**", "~/.ssh/config"]
  },
  "background": {
    "main": "dist/worker.js"
  }
}
```

The `background.main` entry is mandatory — `fs:watch` runs in the worker
iframe, so the manifest must declare a worker entry. Each pattern must
resolve **under `$HOME` or `/tmp`**. Patterns resolving outside (`/etc`,
`/usr`, another user's home, etc.) are rejected at manifest load time. Glob
syntax is [`globset`](https://docs.rs/globset/) — `*`, `**`, `?`, `[abc]`,
`{a,b}` all work. Leading `~/` expands to the user's home directory at load
time.

---

#### Example: SSH config extension — single-file watch

```typescript
// worker.ts
const handle = await fsWatcher.watch(['~/.ssh/config']);
handle.onChange(async (ev) => {
  log.info(`SSH config changed: ${ev.paths.join(', ')}`);
  const hosts = await parseSshConfig();
  await state.set('ssh.hosts', hosts);
});
```

#### Example: Dotfiles extension — multiple roots, non-recursive

```typescript
// worker.ts
const handle = await fsWatcher.watch(['~/.vimrc', '~/.zshrc', '~/.gitconfig'], {
  recursive: false,
  debounceMs: 250,
});

handle.onChange(() => {
  void state.set('dotfiles.stale', true);
});
```

---

#### Surfacing changes to the view

The view never receives fs-watch events directly. Instead, the worker
writes the post-processed result into extension state, and the view
subscribes:

```typescript
// In the view (e.g. inside a Svelte component's onMount):
import type { ExtensionContext, ExtensionStateProxy } from 'asyar-sdk/view';

const stateProxy = context.getService<ExtensionStateProxy>('state');

const initial = (await stateProxy.get('ssh.hosts')) as SshHost[] | null;
let hosts = $state<SshHost[]>(initial ?? []);

const unsubscribe = await stateProxy.subscribe('ssh.hosts', (v) => {
  hosts = (v as SshHost[] | null) ?? [];
});

return () => void unsubscribe();
```

This is the **load-bearing pattern** — it keeps the view free to be
Dormant for the 7 minutes the iframe-lifecycle state machine permits. The
worker keeps writing while the view is gone; when the view re-mounts, its
first `state.get(...)` reads the latest snapshot.

---

#### Subscription semantics

- **One handle = one watch scope.** Handles are immutable after `watch()`. To change the watched paths, dispose the existing handle and create a new one.
- **Multiple callbacks per handle.** Call `onChange` as many times as you like. Each returns its own unsubscribe function.
- **Ref-counted lifecycle.** Disposing a handle drops all its callbacks and frees the host-side debouncer. Disposing is idempotent — a second call is a silent no-op.
- **Auto-cleanup.** All handles for an extension are closed automatically on extension uninstall or disable. You don't need a teardown hook for the normal case.

#### Pattern validation at watch time

The host validates that every path passed to `watch()` is matched by at least one of the manifest's declared patterns. An extension with `permissionArgs["fs:watch"]: ["~/Library/Shortcuts/**"]` cannot `watch(['/etc/hosts'])` — the request is rejected with a permission error before any OS-level watcher is opened.

#### Resource limits

- Max 16 simultaneous handles per extension.
- Max 64 paths per handle.
- Max 512 paths across all extensions combined.

Breaches surface as a host-side validation error. These numbers are large enough that no well-behaved extension will hit them; they exist so a single buggy or malicious extension can't exhaust the host's kernel-level FS-watch budget (Linux inotify imposes per-process ceilings).

#### Worker survival vs. view Dormant

Push events travel through the launcher's prefer-worker push bridge:
Rust emits `asyar:fs-watch` → the bridge looks up the extension's
`iframe[data-role="worker"]` → it posts `asyar:event:fs-watch:push` to the
worker's `contentWindow`. The worker iframe runs with `keep_alive: None`
— it is mounted on enable, unmounted on disable/uninstall, and never
evicted on idle. There is no Dormant-window silent-drop for fs-watch
because the worker is always there to receive.

The view iframe, on the other hand, is `keep_alive: Some(120 s)` and is
evicted ~2 minutes after the user dismisses the panel. Calling
`fs.watch(...)` from a view-side `ExtensionContext` throws
`Service "fsWatcher" not registered` — there is no fsWatcher proxy on the
view bag, by design.

#### Platform coverage

| Platform | Backend | Notes                                                     |
| -------- | ------- | --------------------------------------------------------- |
| Linux    | inotify | Per-user inotify-instance and watch-count ceilings apply. |

The extension-facing event shape is the same regardless of backend. Linux-specific failure modes (e.g. the `/proc/sys/fs/inotify/max_user_watches` limit) surface as a watch-time error from `watch()` — there's no silent dropped-subscriber mode.

#### Lifecycle

| Event                                       | Effect on handles                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `handle.dispose()`                          | Host closes the debouncer; callbacks stop firing immediately.                                                                                                                                                                                                                                                                                                 |
| Worker deactivate (e.g. extension disabled) | All handles closed by the host's `remove_all_for_extension` sweep. Callbacks cease. On re-enable, the worker's `activate()` should re-issue any `watch()` calls it needs.                                                                                                                                                                                     |
| Extension uninstall                         | Same as disable + manifest gone.                                                                                                                                                                                                                                                                                                                              |
| Worker iframe reload (dev hot-reload)       | Host-side handles persist briefly until the next disable/uninstall; the worker should re-issue `watch()` on its next activate. The pattern in [`sdk-playground`'s fsWatch controller](../../../extensions/sdk-playground/src/worker/subscriptions/fsWatch.ts) — read `fsWatch.active` from state on activate and re-issue — is the canonical idempotent boot. |
| Launcher restart                            | Handles do not persist. Worker `activate()` must re-issue `watch()`; the playground demo pattern (boot from `state.get('fsWatch.active')`) is the recommended idiom.                                                                                                                                                                                          |

#### Related runtime notes

- [Worker / view runtime](../explanation/extension-runtime.md) — state machine, mailbox, eviction policy.
- [IPC bridge](../explanation/ipc-bridge.md) — per-message protocol for Tier 2 extensions.
