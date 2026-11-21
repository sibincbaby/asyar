/**
 * Returns the postMessage targetOrigin to use when the launcher posts a
 * message TO an extension iframe.
 *
 * ## Why this is `'*'`
 *
 * Each extension is loaded from its own custom scheme
 * (`asyar-extension://{extensionId}/...`). WebKit treats custom schemes as
 * **opaque origins**, which serialize as the
 * literal string `"null"`. So a call like
 *
 *     iframe.contentWindow.postMessage(msg, 'asyar-extension://org.asyar.pomodoro');
 *
 * fails with *"Unable to post message to asyar-extension://org.asyar.pomodoro.
 * Recipient has origin null."* because the recipient's actual origin is
 * `"null"`, not the scheme URL. Returning `'null'` as a literal string
 * also doesn't match cleanly, so the pragmatic answer is `'*'`.
 *
 * ## Why `'*'` is safe here
 *
 * `targetOrigin` is not the security boundary for extension iframes —
 * that boundary is enforced at multiple other layers:
 *
 *   - The `sandbox="allow-scripts allow-same-origin ..."` attribute on
 *     the iframe element, which restricts what the iframe can do with
 *     its own content.
 *   - The custom scheme itself: only the Tauri runtime can answer
 *     `asyar-extension://{id}/...` requests, and only with the content
 *     of that extension's `dist/` directory.
 *   - The permission gate in `ExtensionIpcRouter` that validates every
 *     incoming message against the extension's declared manifest
 *     permissions before dispatching.
 *
 * The postMessage `targetOrigin` check would only be load-bearing if an
 * attacker could inject a malicious document into the iframe's window
 * — and if they could, the sandbox would already have been broken.
 * Meanwhile, the whole iframe→host direction also uses `'*'` via
 * `MessageBroker.send`, so host→iframe using `'*'` is symmetric.
 */
export function getExtensionFrameOrigin(_extensionId: string): string {
  return '*';
}
