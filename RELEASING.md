# Releasing Asyar

This monorepo contains two independently-released packages. Each has its own flow:

| Package                          | Trigger             | Destination                                                                   |
| -------------------------------- | ------------------- | ----------------------------------------------------------------------------- |
| **Launcher** (`asyar-launcher/`) | Push a `v*` tag     | GitHub Releases                                                               |
| **SDK** (`asyar-sdk/`)           | Push a `sdk-v*` tag | npm: [`asyar-sdk`](https://www.npmjs.com/package/asyar-sdk) + GitHub Releases |

## Ordering: release the SDK first

When a release carries changes to **both** packages, **publish the SDK before the
launcher.** The launcher's release script pins its `asyar-sdk` dependency to
whatever version sits in `asyar-sdk/package.json` **on disk** — it never asks npm.
So bumping the SDK locally and then releasing the launcher first would pin the
launcher to a version that does not exist on npm yet, and every extension install
that resolves `asyar-sdk` would fail.

Check whether the SDK actually changed since its last published tag:

```bash
git diff --stat "sdk-v$(node -p "require('./asyar-sdk/package.json').version")"..HEAD -- asyar-sdk/
```

Empty output means the published SDK is current and you can release the launcher
on its own. Any output means the SDK goes first.

Two things in the SDK reach extension authors directly, so they are worth checking
before deciding the order:

- **`asyar-sdk/cli/`** — the `asyar` CLI that authors run to build and package.
  A packaging fix here does nothing for them until it is on npm.
- **`asyar-sdk/package.json` `exports`** — a new subpath (e.g. `./vite`) is
  invisible to consumers until published. `scaffoldService.ts` probes npm with
  `npm view asyar-sdk exports` at scaffold time and falls back to an older
  template, so a new subpath starts being used the moment it is published — no
  launcher release required.

---

> **Preview first.** Add `--dry-run` to any release command to print exactly what
> it would do (version, files, git commands) **without changing anything or
> touching the network**. The working tree must be clean before a real release —
> the script aborts with `✖ Working tree is not clean.` otherwise, so commit your
> work first.

## Releasing the launcher (desktop app)

The launcher uses a release helper script (a thin wrapper over the shared `scripts/release-lib.mjs`) that keeps every version reference in sync and never depends on npm.

### How to release

From the monorepo root:

```bash
pnpm --filter asyar-launcher run release <patch|minor|major|beta> [--dry-run] [--yes|-y]
```

Or from inside `asyar-launcher/`:

```bash
cd asyar-launcher
pnpm run release <patch|minor|major|beta> [--dry-run] [--yes|-y]
```

> Add `--dry-run` to preview the bump without publishing. Add `--yes` (or `-y`) to bypass interactive confirmation prompts (e.g. In CI or automated scripts).

### Keywords

| Keyword | Bump                | Example                                    |
| ------- | ------------------- | ------------------------------------------ |
| `patch` | x.y.Z               | `0.1.0` → `0.1.1`                          |
| `minor` | x.Y.0               | `0.1.0` → `0.2.0`                          |
| `major` | X.0.0               | `1.0.0` → `2.0.0`                          |
| `beta`  | numeric pre-release | `0.1.0` → `0.1.0-1`, `0.1.0-1` → `0.1.0-2` |

> The script always produces numeric pre-release suffixes (`0.1.0-1`, never `0.1.0-beta`). No manual care needed.

### What the release script does

1. **Duplicate-tag guard**: aborts immediately if `vX.Y.Z` already exists on the remote, so a version is never released twice
2. **Smart Beta-to-Stable guard**: when cutting a stable release, checks git history for prior beta tags (`vX.Y.Z-*`) and warns if un-tested commits were merged to `main` since the last beta or if no beta was released; prompts for confirmation before continuing (`--yes` / `-y` to bypass in CI)
3. **Local SDK version**: reads the SDK version from `asyar-sdk/package.json` on disk — **no npm call** (the workspace always builds the local SDK, so a release can't be blocked by npm being down)
4. **Version sync**: bumps `asyar-launcher/package.json` (its own version + the declared `asyar-sdk` dependency), `src-tauri/Cargo.toml`, `Cargo.lock`, the `scaffoldService.ts` offline fallback, and the root `pnpm-lock.yaml`
5. **Git operations**: commits on a new `release/vX.Y.Z` branch, opens a **PR**, and pushes the `vX.Y.Z` tag (it does _not_ push straight to `main`)
6. **CI trigger**: the `v*` tag triggers `.github/workflows/release-launcher.yml`
7. **Build matrix**: Linux amd64 + arm64 (AppImage and `.deb` per arch) — both required
8. **GitHub Release**: published directly (not a draft) with auto-generated notes
9. **Pre-release detection**: tags with a hyphen (e.g., `v0.1.0-1`) are automatically marked "Pre-release"

> The Rust `SUPPORTED_SDK_VERSION` is **not** set by the release script — `src-tauri/build.rs` injects it at compile time from the local `asyar-sdk/package.json`, so it can never drift.

Linux builds also publish the standalone summon helpers `asyar-summon_amd64`
and `asyar-summon_aarch64`. `install.sh` installs the matching helper beside the
AppImage, and re-running it refreshes both files. The versioned
`org.asyar.Launcher1` interface and `--show-on-start` argument must remain
backward compatible because an upgraded AppImage does not replace an already-installed helper.

### Manual step after CI

The tag triggers the build from the `release/vX.Y.Z` branch, so the installers always carry the correct version even before the PR is merged. After CI is green:

1. **Merge the release PR.** This lands the version bump on `main` so the _next_ release calculates from the right number.

That's it — the GitHub Release is published automatically. Asyar has no self-updater; users upgrade by installing the new `.deb` or AppImage.

---

## Releasing the SDK (npm package)

The SDK uses a release helper script mirroring the launcher's, with one distinguishing detail: SDK tags are prefixed `sdk-v*` (the launcher owns `v*`).

### How to release

From the monorepo root:

```bash
pnpm release:sdk <patch|minor|major|beta>
# equivalent to: pnpm --filter asyar-sdk run release <patch|minor|major|beta>
```

Or from inside `asyar-sdk/`:

```bash
cd asyar-sdk
pnpm run release <patch|minor|major|beta>
```

### Keywords

Same semantics as the launcher's flow:

| Keyword | Bump                | Example                                    |
| ------- | ------------------- | ------------------------------------------ |
| `patch` | x.y.Z               | `2.7.0` → `2.7.1`                          |
| `minor` | x.Y.0               | `2.7.0` → `2.8.0`                          |
| `major` | X.0.0               | `2.7.0` → `3.0.0`                          |
| `beta`  | numeric pre-release | `2.7.0` → `2.7.0-1`, `2.7.0-1` → `2.7.0-2` |

### What the release script does

1. **Duplicate-tag guard**: aborts if `sdk-vX.Y.Z` already exists on the remote
2. **Version bump**: updates `asyar-sdk/package.json`
3. **Lockfile sync**: runs `pnpm install` at the monorepo root
4. **Git operations**: commits on a new `release/sdk-vX.Y.Z` branch, opens a **PR**, and pushes the `sdk-vX.Y.Z` tag
5. **CI trigger**: the `sdk-v*` tag fires `.github/workflows/release-sdk.yml`
6. **Build + publish**: SDK is built (`pnpm run build:all`) and published to npm — the publish step is **idempotent** (it skips if that version is already on npm, so a re-run is always safe)
7. **GitHub Release**: created with auto-generated release notes; tags containing a hyphen (e.g., `sdk-v2.8.0-1`) are marked **Pre-release**

After CI is green, **merge the release PR** to land the version bump on `main`.

### Required GitHub secret

The SDK release workflow needs `NPM_TOKEN` configured at **Settings → Secrets and variables → Actions** with publish rights for `asyar-sdk`. Generate an "Automation" token at [npmjs.com/settings/Xoshbin/tokens](https://www.npmjs.com/settings/Xoshbin/tokens).

---

## Distributing via a CLI install script

Closes the original ask in [discussion #315](https://github.com/Xoshbin/asyar/discussions/315)
("can I install Asyar from the CLI?"). `install.sh` currently targets the upstream repository
(`REPO="Xoshbin/asyar"`), so it installs upstream releases, not this fork's. Point `REPO` at the
fork before advertising it. Linux users can run:

```bash
curl -fsSL https://raw.githubusercontent.com/Xoshbin/asyar/main/install.sh | sh
```

This downloads and runs [`install.sh`](install.sh) from the repo root, which detects the
arch, queries the GitHub Releases API for the newest release, downloads the matching
AppImage and `asyar-summon` helper, and installs them — no manual `.AppImage` handling. To pin a specific version instead of the newest one:

```bash
ASYAR_VERSION=0.1.1-38 curl -fsSL https://raw.githubusercontent.com/Xoshbin/asyar/main/install.sh | sh
```

### Scope decisions

- **AppImage only, no `.deb` detection.** The release also publishes `.deb` assets, but
  adding distro detection (`apt` vs. not), sudo escalation for `dpkg -i`, and a different
  install location would roughly double the script's surface for marginal gain — the
  AppImage is distro-agnostic by design (the same reason discussion #315's original answer
  picked it), needs no root, and already covers both `aarch64`/`amd64`. It's installed to
  `~/.local/bin/asyar` (override with `ASYAR_INSTALL_DIR`), with a PATH check that prints the
  `export PATH=...` line to add if `~/.local/bin` isn't already on it.
- **No update-checking, always (re)installs.** Asyar has no self-updater, so re-running this
  script is how you upgrade. If an install already exists
  (`~/.local/bin/asyar`) the script prints its detected version and replaces it
  rather than prompting; a `curl | sh` pipe has no interactive terminal to prompt against
  safely, and re-running to force a clean reinstall is itself a reasonable use case.
- **GitHub's `/releases/latest` isn't used.** Every Asyar release so far is a numeric
  pre-release (`v0.1.1-N`, hyphenated tag), and `/releases/latest` explicitly excludes
  pre-releases — it 404s today. The script queries the plain release list with `per_page=1`
  and takes the newest entry instead, with no `jq` dependency (asset URLs are pulled out of
  the JSON with `grep`/`sed`, matched against the confirmed real asset names:
  `asyar_{version}_aarch64.AppImage` / `asyar_{version}_amd64.AppImage` — note the
  arch-naming inconsistency: the AppImage uses `aarch64`/`amd64`, while the `.deb` assets use
  `arm64` rather than `aarch64`).
- Written in POSIX `sh`, not bash — the documented invocation pipes into `sh`, which is
  `dash` on Debian/Ubuntu, not bash. No arrays, no `[[ ]]`, no bashisms. Checked with
  `shellcheck -s sh install.sh` (clean); `shfmt` isn't installed in this repo so there's no
  auto-formatter for it, matching the "no existing shell lint convention" state noted when
  this was scoped.

## When a release fails

Every step is re-runnable without burning a version number. If a CI run fails
(npm publish, a build target), fix the cause and re-run the
workflow with the **same tag**: GitHub → **Actions** → the workflow →
**Run workflow** (workflow_dispatch) → enter the existing tag. A `concurrency`
guard prevents two runs of the same tag from colliding, npm publish is
idempotent — so re-running is always safe.

### Symptom → recovery

| Scenario                                                                           | What's safe                                                                            | Recovery                                                                                                                                                                                                                                                                                                |
| ---------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SDK `npm publish` fails (bad token / version exists / network)                     | Tag + bump on the release branch/PR; npm unchanged                                     | Fix the cause → re-run **release-sdk.yml** via _Run workflow_ with the same tag. Publish is idempotent: it skips if the version is already on npm.                                                                                                                                                      |
| One launcher build target fails (flaky build / runner down)                        | Other artifacts built; nothing published yet                                           | Re-run **release-launcher.yml** via _Run workflow_ with the same tag — no new version. The concurrency guard prevents races.                                                                                                                                                                            |
| `install.sh` fails for a user (asset match broken, network, GitHub API rate limit) | Not part of the release pipeline — nothing published is affected, no version to re-tag | Fix `install.sh` directly on `main`; no tag/version bump needed, since the raw URL always serves current `main`. If it's GitHub's unauthenticated API rate limit (60 req/hr per IP), the workaround is downloading the matching asset from the Releases page directly instead of re-running the script. |
| Tag pushed by mistake / wrong version                                              | —                                                                                      | `git push --delete origin <tag>`, delete the GitHub release, then re-release. The duplicate-tag guard otherwise blocks a duplicate version.                                                                                                                                                             |
| Launcher release while npm is down                                                 | N/A                                                                                    | Not a failure mode — the launcher reads the SDK version from local `asyar-sdk/package.json`.                                                                                                                                                                                                            |
| A release script half-finished locally (Ctrl-C / error mid-run)                    | The script cleans up its leftover local branch/tag on the next run                     | Run it again (`--dry-run` first to preview); the duplicate-tag guard blocks an accidental re-release of the same version.                                                                                                                                                                               |

**Notes**

- Launcher publish intentionally requires **both** targets (amd64 and arm64) — the release must never be half-published. The improvement is one-click recovery, not relaxing that invariant.
- The build runs from the tag (the release branch's bump commit), so released binaries always carry the right version even before the PR merges. Merge the release PR so `main` carries the bump and the next release calculates correctly.

## Quick reference

| To release…             | Run                                                                                                                     |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| The launcher            | `pnpm --filter asyar-launcher run release patch` (or `minor` / `major` / `beta`; add `--dry-run` to preview) — tag `v*` |
| The SDK                 | `pnpm release:sdk patch` (or `minor` / `major` / `beta`; add `--dry-run` to preview) — tag `sdk-v*`                     |
| Re-run a failed release | GitHub → Actions → the workflow → **Run workflow** → same tag                                                           |
