# Keyboard Shortcuts

> Every Asyar shortcut in one place. The same list appears in-app under "Help".

![The in-app Help view cheat sheet](../images/keyboard-shortcuts-help.png)
_Figure: the in-app Help view cheat sheet._

## Global

These shortcuts work anywhere inside the Asyar launcher window.

| Shortcut                          | What it does                                           |
| --------------------------------- | ------------------------------------------------------ |
| The hotkey you chose during setup | Show or hide Asyar from anywhere on your computer      |
| `Ctrl+,`                          | Open Settings                                          |
| `Ctrl+K`                          | Toggle the action panel                                |
| `Ctrl+P`                          | Toggle the search-bar dropdown (when one is shown)     |
| `@`                               | Scope search exclusively to runnable commands          |
| `Tab`                             | Fill command arguments, or switch to AI / context mode |
| `↑` / `↓`                         | Move between results                                   |
| `Enter`                           | Run the selected result                                |
| `Esc`                             | Clear the search → go back from a view → hide Asyar    |

The show/hide hotkey is user-configurable in **Settings → Shortcuts**.

**Hyper Key (`✦`) Support:** If you configure a shortcut using all four modifiers (`Ctrl + Alt + Shift + Super`), Asyar renders the combination with the clean `✦` glyph (e.g. `✦Space`).

**Standalone Function Keys (`F1`–`F24`):** Function keys (including extended keys `F13`–`F24`) can be assigned as hotkeys directly without requiring modifiers.

With an empty search field and the first result selected, press `↑` to recall the most recent query. Keep pressing `↑` for older queries and `↓` to move back toward an empty field. Asyar saves a query when you use a result or clear the field with `Esc`.

Action shortcuts (everything listed under _In a view_ and _Per-feature_, and the hints in the `Ctrl+K` panel) use `Ctrl` as their main modifier. Global hotkeys you record yourself keep the exact keys you pressed.

**Wayland:** Because Wayland protocol security blocks background hotkey grabs, assign your shortcut inside your desktop compositor settings to run `asyar-summon` (for example, `bind = $mainMod, SPACE, exec, asyar-summon` in `~/.config/hypr/hyprland.conf`, `hl.unbind("SUPER + SPACE")` + `o.bind("SUPER + SPACE", "Asyar", "asyar-summon")` in `~/.config/hypr/bindings.lua` on Omarchy v4+, or `unbind = SUPER, SPACE` + `bindd = SUPER, SPACE, Asyar, exec, asyar-summon` in `~/.config/hypr/bindings.conf` on Omarchy v3). Executing `asyar-summon` while the daemon is running acts as an instant D-Bus toggle.

## In a view

These shortcuts apply when you have drilled into a result that opens a full view (for example, Clipboard History, Snippets, Window Switcher, or an AI agent conversation).

| Shortcut    | What it does                                                                                |
| ----------- | ------------------------------------------------------------------------------------------- |
| `Backspace` | Go back from the open view (when the search bar is empty), or exit AI mode                  |
| `Esc`       | Follows the Escape behaviour you configured: step backwards, hide window, or reset launcher |
| `Ctrl+K`    | Toggle the action panel for the selected item in the view                                   |

You can change what `Esc` does when a view is open in **Settings → Advanced → Escape Key**:

- **Step Backwards** — clears the search first, then pops the view, then hides Asyar (the default).
- **Hide Window** — hides Asyar immediately without clearing state.
- **Reset Launcher** — hides Asyar and resets all state so the next open starts fresh.

## Per-feature

Built-in features declare Raycast-aligned shortcuts while their view is active. Every shortcut is shown next to its action in the `Ctrl+K` panel and pressing it runs that action cleanly through the central dispatcher. Shortcuts do nothing while the `Ctrl+K` panel or a dialog is open, and text-editing keys such as `Ctrl+A`, `Ctrl+C`, `Ctrl+V`, `Ctrl+Z`, and `Ctrl+Backspace` remain reserved for editing. Destructive actions (`Ctrl+X` for single-item delete, `Ctrl+Shift+X` for bulk clear) always require confirmation (and `Ctrl+X` cuts text whenever text is selected in the focused input).

**Root Search**

| Shortcut       | What it does                                            |
| -------------- | ------------------------------------------------------- |
| `Ctrl+Shift+F` | Add / Remove favorite (Apps & Commands)                 |
| `Ctrl+.`       | Pin / Unpin selected file                               |
| `Ctrl+R`       | Reveal selected application or file in the file manager |
| `Ctrl+Shift+C` | Copy Path (Apps/Files), Deeplink (Commands), or URL     |
| `Ctrl+Shift+E` | View all commands of the selected extension             |
| `Ctrl+Shift+,` | Configure selected extension settings                   |
| `Ctrl+X`       | Move selected file to Trash (with confirm)              |

**Notes**

| Shortcut       | What it does                  |
| -------------- | ----------------------------- |
| `Ctrl+N`       | Create a note                 |
| `Ctrl+.`       | Pin / Unpin the selected note |
| `Ctrl+D`       | Duplicate the selected note   |
| `Ctrl+Shift+C` | Copy note as Markdown         |
| `Ctrl+X`       | Delete the selected note      |

**Snippets**

| Shortcut       | What it does                              |
| -------------- | ----------------------------------------- |
| `Enter`        | Paste the selected snippet                |
| `Ctrl+Enter`   | Save form / submit changes in edit views  |
| `Ctrl+N`       | Create a new snippet                      |
| `Ctrl+E`       | Edit the selected snippet                 |
| `Ctrl+D`       | Duplicate the selected snippet            |
| `Ctrl+Shift+P` | Insert dynamic placeholder (when editing) |
| `Ctrl+Shift+C` | Copy snippet expansion                    |
| `Ctrl+.`       | Pin / Unpin the selected snippet          |
| `Ctrl+X`       | Delete the selected snippet               |
| `Ctrl+Shift+X` | Clear all snippets                        |

**Clipboard History**

| Shortcut            | What it does                               |
| ------------------- | ------------------------------------------ |
| `Ctrl+P`            | Filter by type, source app, or date        |
| `Enter`             | Paste selected item (or merge & paste)     |
| `Ctrl+Enter`        | Copy selected item to clipboard            |
| `Ctrl+Shift+Enter`  | Paste as plain text                        |
| `Ctrl+↑` / `Ctrl+↓` | Extend selection for merged paste          |
| `Ctrl+.`            | Pin / Unpin the selected item              |
| `Ctrl+Shift+E`      | Rename selected item                       |
| `Ctrl+O`            | Open URL item in browser                   |
| `Ctrl+S`            | Save text item as snippet                  |
| `Ctrl+I`            | Ask AI about selected text                 |
| `Ctrl+X`            | Delete selected item (with confirmation)   |
| `Ctrl+Shift+X`      | Clear clipboard history (non-pinned items) |

**AI & Agents**

| Shortcut       | What it does                                          |
| -------------- | ----------------------------------------------------- |
| `Ctrl+N`       | Create a new agent (Agents view) / Start a new thread |
| `Ctrl+E`       | Edit the selected agent (Agents view)                 |
| `Ctrl+X`       | Delete selected agent or thread (with confirmation)   |
| `Ctrl+.`       | Pin / Unpin active thread (Chat view)                 |
| `Ctrl+Shift+C` | Copy the assistant's last reply (Chat view)           |
| `Enter`        | Send user message or follow-up prompt (Chat view)     |
| `Enter`        | Open agent run in chat (Recent Runs view)             |
| `Ctrl+Shift+X` | Clear recent runs (Recent Runs view)                  |

**Window Management**

| Shortcut       | What it does                                    |
| -------------- | ----------------------------------------------- |
| `Enter`        | Switch to selected window / Apply saved layout  |
| `Ctrl+W`       | Close the selected window                       |
| `Ctrl+Shift+C` | Copy the window title                           |
| `Ctrl+N`       | Save the current window layout (Manage Layouts) |
| `Ctrl+Shift+E` | Rename the selected layout                      |
| `Ctrl+X`       | Delete the selected layout (with confirmation)  |
| `Ctrl+K`       | Open actions for the selected window or layout  |

**File Search**

| Shortcut         | What it does                                                               |
| ---------------- | -------------------------------------------------------------------------- |
| `Enter`          | Open selected file (File Search view)                                      |
| `Ctrl+Y`         | Open selected file in its default app from main search (preserves `Space`) |
| `Ctrl+R`         | Reveal in file manager                                                     |
| `Ctrl+Shift+C`   | Copy Path                                                                  |
| `Ctrl+Alt+C`     | Copy Name                                                                  |
| `Ctrl+T`         | Open in Terminal                                                           |
| `Ctrl+.`         | Toggle Pin                                                                 |
| `Tab` / `Ctrl+I` | Send to Asyar AI                                                           |
| `Ctrl+Shift+F`   | Search Everywhere (deep `plocate` search)                                  |
| `Ctrl+X`         | Move to Trash (with confirmation)                                          |
| `Ctrl+Shift+X`   | Clear File Search history (File Search view)                               |

**Portals, Scripts, MCP, Store, Shortcuts & Help**

| Feature         | Shortcut       | What it does                                |
| --------------- | -------------- | ------------------------------------------- |
| Portals         | `Ctrl+Enter`   | Save portal form                            |
| Portals         | `Ctrl+N`       | Create a new portal                         |
| Portals         | `Ctrl+E`       | Edit selected portal                        |
| Portals         | `Ctrl+D`       | Duplicate selected portal                   |
| Portals         | `Ctrl+X`       | Delete selected portal (with confirmation)  |
| Script Commands | `Enter`        | Run selected script (or make executable)    |
| Script Commands | `Ctrl+R`       | Reveal selected script in file manager      |
| Script Commands | `Ctrl+Shift+C` | Copy selected script path                   |
| MCP Servers     | `Ctrl+N`       | Install MCP server                          |
| MCP Servers     | `Ctrl+T`       | Test MCP server connection (Install form)   |
| MCP Servers     | `Ctrl+R`       | Refresh MCP server status                   |
| MCP Permissions | `Ctrl+X`       | Revoke selected tool permission             |
| Store           | `Enter`        | Install / Update / Run selected extension   |
| Store           | `Ctrl+Shift+,` | Open extension preferences (Detail view)    |
| Store           | `Ctrl+X`       | Uninstall extension (Detail view)           |
| Store           | `Ctrl+O`       | Open extension page in web browser          |
| Shortcuts       | `Ctrl+E`       | Change global shortcut                      |
| Shortcuts       | `Ctrl+X`       | Remove global shortcut (with confirmation)  |
| AI Builder      | `Enter`        | Open created extension (Created Extensions) |
| AI Builder      | `Ctrl+O`       | Open created extension directory in editor  |
| Help            | `Ctrl+O`       | Open User Guide                             |

For all other features, use `Ctrl+K` to open the action panel — every available action for the selected item is listed there.

## Related

- [The Basics](./the-basics.md)
- [Getting Started](./getting-started.md)
- [Settings](./settings.md)
- [Window Management](./features/window-management.md)
