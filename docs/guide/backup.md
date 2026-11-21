# Backup

> Export your Asyar data to a file and restore it on this or another computer.

Asyar is local-first: there is no account and no cloud sync. To move your data between machines, or to keep a safety copy, use a local backup file.

## Export & import a backup

Backups let you save a snapshot of your Asyar data to a file and restore it on the same computer or a different one.

**Exporting a backup:**

1. Open **Settings → Backup**.
2. Check the data categories you want to include (for example, Snippets, Agents, Shortcuts).
3. Optionally set a password to encrypt sensitive fields in the file. If you leave it blank, those fields are stripped from the exported file automatically.
4. Click **Export…** and choose where to save the file.

> **API keys are never included in a backup**, with or without a password. After you restore a backup on another device, enter your keys again in **Settings → AI**. If you import an older backup that still contains keys, Asyar moves them into its encrypted key store on this device.

**Importing a backup:**

1. Open **Settings → Backup**.
2. Click **Choose Backup File…** and select a previously exported backup.
3. If the file is password-protected, enter the password when prompted.
4. A preview dialog shows each category in the backup along with counts (local vs. incoming) and any conflicts.
5. For each category, choose how to handle conflicts: **Merge** (keep both), **Replace** (overwrite with backup data), or **Skip** (leave your current data untouched).
6. Click **Restore** to apply.

## Related

- [Settings](./settings.md)
- [FAQ](./faq.md)
- [Troubleshooting](./troubleshooting.md)
