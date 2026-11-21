# Privacy-First & Local-First Outbound Data Egress Policy

Asyar is a local-first platform built on the guarantee that user data stays private and on-device by default. This fork has no Asyar account, no cloud sync, no telemetry, and no crash-report upload — do not add them back.

## 1. The Core Egress Invariant

**NO USER DATA EVER LEAVES THE DEVICE THROUGH ASYAR UNLESS THE USER EXPLICITLY ASKED FOR IT.**

All outbound network requests must be justified by a user-initiated action before any socket is opened or HTTP payload constructed.

## 2. Allowed Egress Channels

| Channel                                                              | Purpose                                                   | Notes                                                                               |
| -------------------------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| **Extension Store** (asyar.org catalog)                              | Browse, install, and check updates for Store extensions   | Sends no user data beyond what the catalog request itself requires.                 |
| **User-configured AI / web-search providers**                        | Requests the user triggers, with the user's own API keys  | Messages are secret-redacted first; API keys never leave the encrypted local store. |
| **Extension network access** (`network`, `network:lan`, `oauth:use`) | Whatever the extension declared and the user consented to | Gated by the permission system; see `05-architecture.md`.                           |

Anything else (analytics, usage sharing, crash reports, settings/content sync, app self-update checks) does not exist and must not be introduced without an explicit product decision from the user.

## 3. Defense-in-Depth Requirements

1. **Rust-Level Enforcement**: Never rely solely on frontend guards. Rust command handlers must independently enforce permission and consent rules before initiating network calls.
2. **Local-only analytics**: Usage Stats are recorded in the local SQLite database and are never transmitted.
3. **Local backup only**: Profile export/import writes to a file the user chooses; API keys are never included.
