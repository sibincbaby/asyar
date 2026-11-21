import type { ResolvedIcon } from '../bindings';

/**
 * An icon as callers hold it: a `ResolvedIcon` resolved in Rust
 * (`src-tauri/src/icons.rs`), or a host-internal string such as
 * `'icon:timer'`, an emoji, or a host image URL.
 */
export type IconSource = ResolvedIcon | string | null | undefined;

const HOST_SCHEMES = ['asyar-extension', 'asyar-icon', 'asyar-thumb'] as const;

/**
 * Normalizes a host scheme or data image URL for the current webview.
 * `http://asyar-*.localhost/` URLs are rejected so plain HTTP loopback URLs can
 * never reach an `<img>`.
 */
export function normalizeHostImageUrl(raw: string): string | null {
  if (raw.startsWith('data:image/')) return raw;

  for (const scheme of HOST_SCHEMES) {
    for (const prefix of [`${scheme}://localhost/`, `${scheme}://`]) {
      if (raw.startsWith(prefix)) return raw.length > prefix.length ? raw : null;
    }
  }

  return null;
}

/**
 * Adapter for icon strings that never pass through the Rust resolver
 * (built-in literals, runtime actions). Same rules as `resolve_icon` with no
 * owning extension: relative paths and unknown schemes resolve to `null`.
 */
export function toResolvedIcon(icon: IconSource): ResolvedIcon | null {
  if (!icon) return null;
  if (typeof icon !== 'string') {
    if (icon.kind !== 'image') return icon;
    const src = normalizeHostImageUrl(icon.src);
    if (!src) return null;
    const dark = icon.dark ? normalizeHostImageUrl(icon.dark) : null;
    if (src === icon.src && (dark ?? undefined) === (icon.dark ?? undefined)) return icon;
    return {
      ...icon,
      src,
      ...(icon.dark !== undefined ? { dark } : {}),
    };
  }
  const raw = icon.trim();
  if (raw.startsWith('icon:')) return raw.length > 5 ? { kind: 'glyph', name: raw.slice(5) } : null;
  const hostImage = normalizeHostImageUrl(raw);
  if (hostImage) return { kind: 'image', src: hostImage };
  if (raw.startsWith('https://')) return { kind: 'remote', url: raw };
  if (!raw || /[:./\\]/.test(raw)) return null;
  return { kind: 'emoji', value: raw };
}
