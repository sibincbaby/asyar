# asyar-ui/1: native UI for command-line tools

Status: **draft**, not implemented.

`--json` lets AI agents drive a CLI. `asyar-ui` lets the launcher put a native
GPUI interface on the same CLI. The tool keeps every piece of logic,
authentication and network access; it answers with a description of _what_ to
show, and the launcher decides _how_, with one set of native components that
every tool shares.

```
launcher ──stdin: request──▶  mytool ui <route>  ──▶ the tool's own backend
         ◀─stdout: reply────
```

## Principles

1. **Tools describe meaning, the launcher owns the look.** Replies never carry
   colours, sizes, fonts or layout. A tag says `"tone": "success"`, not
   "green". Theming (light, dark, glass), virtualization, keyboard handling and
   animation are the launcher's.
2. **A closed set of views.** Each view type maps to one reusable GPUI
   component. New capabilities arrive as new view types or fields, never as
   tool-drawn UI.
3. **Speed is part of the contract.** Filtering mode, detail prefetch,
   optimistic patches and cache lifetimes are declared in the reply, so the
   launcher never blocks typing on a process.
4. **Tool output is untrusted.** It relays remote data; the launcher validates
   it (see [Security](#security)).

## Transport

A tool supports the protocol when `mytool ui manifest` succeeds. Every other
call is one process run:

```
mytool ui <route>          # request JSON on stdin, reply JSON on stdout
```

- The request goes on stdin, so free text (a comment, a search query) never
  touches the command line or the process list.
- The process has no TTY, stdin closes after the request, and the launcher
  kills it on timeout (default 30 s) or when its result is no longer needed
  (the user moved on).
- Exit codes follow the tool's `--json` conventions; the launcher reads the
  reply whatever the exit code.

_Later (not in v1):_ `mytool ui serve --stdio` keeps one process alive and
exchanges the same messages as JSON lines, adding an `"id"` to match replies
to requests. Messages are shaped so nothing else changes.

### Request

```json
{
  "schema": "asyar-ui/1",
  "accepts": ["asyar-ui/1"],
  "route": "tasks",
  "params": { "state": "open" },
  "query": "login",
  "host": { "name": "asyar-gpui", "version": "0.1.0" }
}
```

| Field     | Meaning                                                            |
| --------- | ------------------------------------------------------------------ |
| `accepts` | Protocol versions the launcher can render, newest first.           |
| `route`   | Which screen or action. Routes are the tool's own names.           |
| `params`  | Arguments from the manifest, a previous view or a submitted input. |
| `query`   | The search text, only for lists with `"filtering": "remote"`.      |

### Reply

Same envelope as `--json`, so a tool has one success path and one error path:

```json
{ "schema": "asyar-ui/1", "ok": true, "data": { "view": { … } } }
```

```json
{
  "schema": "asyar-ui/1",
  "ok": false,
  "error": { "code": "AUTH_MISSING", "message": "Not signed in", "hint": "Run `mytool auth login`" }
}
```

`data` holds exactly one of `view` (a screen), `result` (an action outcome) or
`manifest`.

## Manifest

`mytool ui manifest` (empty request) describes the tool and what appears in
the launcher's root search:

```json
{
  "manifest": {
    "id": "mytool",
    "title": "My Tool",
    "icon": "checklist",
    "version": "2.3.0",
    "entries": [
      {
        "route": "tasks",
        "title": "My Tool",
        "subtitle": "Tasks assigned to me",
        "keywords": ["todo"]
      },
      { "route": "inbox", "title": "My Tool Inbox", "badge_route": "inbox-count" }
    ]
  }
}
```

- Each entry becomes one root-search row; Enter opens its route.
- `icon` names a launcher icon from a fixed set (no image URLs in v1).
- `badge_route`, optional: a cheap route returning `{"result": {"badge": "14"}}`,
  shown on the row and cached.

The launcher caches the manifest until the tool binary changes.

## Views

Every view has `type` and optional `title`, `actions` (screen-level) and
`cache_secs` (how long the launcher may show it again before re-running the
route; default 0, meaning "show the cached copy, refresh in the background").

### `list`

```json
{
  "type": "list",
  "title": "Tasks",
  "search": { "placeholder": "Filter tasks…", "filtering": "local" },
  "filters": [
    {
      "id": "state", "title": "State", "value": "open",
      "options": [
        { "value": "open", "title": "Open" },
        { "value": "closed", "title": "Closed" },
        { "value": "all", "title": "All" }
      ]
    }
  ],
  "detail": { "route": "task", "param": "id", "debounce_ms": 100 },
  "sections": [
    {
      "title": "Today",
      "items": [
        {
          "id": "1042",
          "title": "Fix login redirect",
          "subtitle": "#1042 · Web app",
          "icon": "bug",
          "accessories": [
            { "tag": "In progress", "tone": "info" },
            { "text": "4m", "tooltip": "Updated 2026-11-24 10:16" }
          ],
          "keywords": ["auth"],
          "fields": [{ "label": "Assignee", "value": "You" }],
          "actions": [ … ]
        }
      ]
    }
  ]
}
```

- `search.filtering`: `local` filters loaded items in memory on every keystroke;
  `remote` re-runs the route with `query` after typing pauses (default 150 ms),
  cancelling the previous run.
- `filters`: dropdowns in the header. Changing one re-runs the route with
  `params[filter.id] = option.value`.
- `detail`: when present the list shows a detail pane. When the selection
  rests for `debounce_ms`, the launcher runs `route` with
  `params[param] = item.id`, cancels a superseded run, and caches each result.
  Until it arrives, the pane shows the item's own `title`, `accessories` and
  `fields`, so it is never empty.
- `accessories`: right-aligned `text`, `tag` (with `tone`) or `date` (ISO 8601,
  formatted by the launcher).
- `tone`: `neutral`, `info`, `success`, `warning`, `danger`.

### `detail`

```json
{
  "type": "detail",
  "title": "Fix login redirect",
  "tags": [
    { "tag": "Bug", "tone": "danger" },
    { "tag": "In progress", "tone": "info" }
  ],
  "fields": [
    { "label": "Assignee", "value": "You" },
    { "label": "Done", "progress": 0.6 },
    { "label": "Web", "link": "https://tracker.example/tasks/1042" }
  ],
  "markdown": "## Steps\n1. Sign in\n2. …",
  "sections": [
    {
      "title": "Activity",
      "timeline": [
        { "at": "2026-11-24T10:16:52Z", "who": "Asha", "text": "Changed status to In progress" },
        { "at": "2026-11-24T11:02:00Z", "who": "You", "markdown": "Fixed in `auth.rs`" }
      ]
    },
    { "title": "Subtasks", "items": [ { "id": "1043", "title": "Add test", "route": "task" } ] }
  ],
  "actions": [ … ]
}
```

- `fields`: label with one of `value`, `progress` (0–1), `link`, or `tags`.
- `markdown`: CommonMark plus tables. No raw HTML, no images in v1.
- `sections`: `timeline` (events, newest last) or `items` (rows that open
  `route` with their `id`).

## Actions

Actions appear in the footer (first two) and the Ctrl+K action panel (all).
Shortcuts are hints; the launcher drops ones that clash with its own keys or
with text editing.

```json
{
  "id": "set-status",
  "title": "Set Status",
  "shortcut": "s",
  "safety": "write",
  "kind": "pick",
  "pick": { "options_route": "statuses", "cache_secs": 600 },
  "run": { "route": "task-update", "params": { "id": "1042" }, "input_param": "status" },
  "optimistic": { "accessories": [{ "tag": "{label}", "tone": "info" }] }
}
```

| `kind`  | What happens                                                                                    |
| ------- | ----------------------------------------------------------------------------------------------- |
| `open`  | Opens `url` in the browser (`http`/`https` only).                                               |
| `copy`  | Copies `text`.                                                                                  |
| `push`  | Opens another screen: `route` with `params`. Esc goes back.                                     |
| `run`   | Runs `run.route` with `run.params`.                                                             |
| `pick`  | Shows a picker filled from `pick.options` or `pick.options_route`, then `run`s with the choice. |
| `input` | Shows a one-line (or `multiline`) text field with `placeholder`, then `run`s with the text.     |

- `safety`: `read`, `write`, `destructive` (the same classes as lwcyl). The
  launcher runs `read` and `write` at once; `destructive` asks for confirmation
  first.
- `run.input_param`: the parameter that receives the picked value or typed
  text.
- `optimistic`: changes applied to the current item immediately (`{label}` is
  the picked option's title); reverted if the action fails.

An action route replies with a `result`:

```json
{
  "result": {
    "toast": "Status set to Resolved",
    "refresh": true,
    "patch": { "accessories": [{ "tag": "Resolved", "tone": "success" }] },
    "pop": false
  }
}
```

`refresh` re-runs the current view; `patch` replaces the optimistic change
with the confirmed one; `pop` goes back one screen. On `ok: false` the
launcher reverts the optimistic change and shows `error.message`.

Picker options (`options_route`) reply with
`{"result": {"options": [{"value": "5", "title": "Resolved", "tone": "success"}]}}`.

## Versioning

- The schema id is `asyar-ui/<major>`. Within a major version, changes only
  add optional fields or new view and action kinds.
- Launchers ignore unknown fields. An unknown view `type` or action `kind`
  shows a "needs a newer launcher" placeholder instead of failing.
- The request's `accepts` lists versions the launcher renders; the reply's
  `schema` names the one used. A tool that only speaks a newer major replies
  `ok: false` with code `UNSUPPORTED_SCHEMA`.
- A JSON Schema generated from the reference Rust types is published with each
  version, so tools in any language (and AI agents writing them) can validate
  their output. Reply fixtures from every released version stay in the
  launcher's tests.

## Security

The launcher treats every reply as untrusted:

- Links open only for `http` and `https`.
- Markdown renders without raw HTML; images are not fetched in v1.
- Text is capped (titles 500 characters, Markdown 200 KB, 5,000 items a list).
- Routes and params pass to the tool on stdin, never through a shell.
- `destructive` actions always confirm; a tool can't mark them otherwise.
- A tool is trusted only after the user installs its manifest; the launcher
  never runs programs a reply names.

## Fallback

Tools without `ui` support keep working through the `commands --json` catalog:
their read-only commands appear as plain searchable lists.

## Reference implementation (planned)

- `asyar-ui-protocol`: Rust types (serde) for every message, the JSON Schema,
  and fixtures. Tools depend on it to emit replies; the launcher to parse them.
- Launcher components, one per protocol type: list with detail pane, detail,
  Markdown, timeline, tag, action panel, picker, text input, toast. Candidate
  base: [`gpui-kit`](https://github.com/longbridge/gpui-kit) (Apache-2.0)
  already ships `VirtualList`, `TextView` (Markdown), `Select`/`Combobox`,
  `Menu`, command palette, `Notification`, `Tag`, `Kbd`, `Tabs` and a theme
  system. It pins `gpui-pre =0.3.14` (a Zed GPUI snapshot), not `gpui-ce`, so
  adopting it moves the launcher to that GPUI.
