# FAQ

> Quick answers to common questions.

## Is Asyar free?

Yes. Asyar is free to use, with no account and no subscription. Everything — apps, commands, calculator, clipboard history, snippets, scripts, and the extension store — works without signing in.

## Does Asyar work offline?

Yes. Everything that does not require a network request works fully offline: app search, command execution, clipboard history, text snippets, scripts, calculator, and all locally installed extensions.

Features that need the internet:

- **AI queries** — sent to your configured AI provider's API.
- **Extension store** — browsing and installing extensions requires internet access.
- **Browser bookmark/history search** — the companion bridge runs locally, but the initial pairing requires the browser to be reachable.

If you lose your connection mid-session, all offline features continue to work without interruption.

## Where is my data stored?

All of your Asyar data is stored locally on your computer, inside your user profile directory. This includes your settings, snippets, shortcuts, clipboard history, agents, and any other data you create.

There is no cloud sync. To move your data to another computer, export a backup — see [Backup](./backup.md).

Asyar never sells or shares your data with third parties.

## How do I get more extensions?

Open the Asyar launcher and search for "Store", then press `Enter`. The built-in extension store lets you browse, search, and install extensions and themes in one click. Extensions are installed directly to your computer; no account is required.

You can also install an extension from a local file: open **Settings → Extensions**, click "Install from file", and pick the extension package. Extension developers can make that package from their project with `asyar pack`, which writes an `.asyar` file (unlink the extension first with `asyar unlink`, or Asyar keeps loading it from the project folder).

To remove an extension, find it in **Settings → Extensions**, open its detail panel, and click Uninstall.

## Can I import my data from Raycast?

Yes. Search for **Import from Raycast** in the launcher, or open **Settings → Backup → Import from Raycast…**. It reads a Raycast export file (`.rayconfig`, with or without a password) or the plain JSON files from Raycast's Export Snippets/Export Quicklinks commands, and brings over your snippets, quicklinks (as portals), and app hotkeys. See [Import from Raycast](./features/raycast-import.md) for details on what does and doesn't carry over.

## Related

- [Getting Started](./getting-started.md)
- [Settings](./settings.md)
- [Backup](./backup.md)
- [Import from Raycast](./features/raycast-import.md)
- [Troubleshooting](./troubleshooting.md)
