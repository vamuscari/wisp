# Wisp

Wisp is a standalone terminal picker for projects, host windows, files, and
OpenCode sessions. A Rust core owns configuration, local discovery, typed
filesystem entries, lazy
navigation, and a persistent cache. The same Ratatui interface runs directly,
in a temporary WezTerm tab, or in a Neovim floating terminal.

Host integrations are intentionally thin. They receive a versioned selection
and apply host-specific behavior; neither adapter discovers projects or embeds
its own picker UI.

## Requirements

- The `wisp` executable on `PATH`
- WezTerm `20240127-113634-bbcac864` or newer for the WezTerm adapter
- Neovim `0.10.4` or newer for the Neovim adapter
- OpenCode `1.18.15` for optional session tracking

Project discovery is local to the machine running `wisp`. A configured named
WezTerm domain may point at a same-host mux server, but remote project paths are
not supported.

## Install

Download an archive for Linux, macOS, or Windows from
[GitHub Releases](https://github.com/vamuscari/wisp/releases), place `wisp`
(or `wisp.exe`) on `PATH`, then deploy the executable and both host adapters as
one versioned bundle:

```sh
wisp deploy
```

To track ordinary OpenCode TUI launches in addition to a shared server, install
the plugin loader from the active Wisp bundle, then restart OpenCode:

```sh
wisp opencode install
```

To install from source with Rust 1.85 or newer:

```sh
cargo install --git https://github.com/vamuscari/wisp --locked wisp-cli
wisp deploy
```

From a local checkout:

```sh
cargo run --release --locked -p wisp-cli -- deploy
```

`wisp deploy` copies the running executable, `wezterm/`, `nvim/`, and the
OpenCode plugin into one content-addressed deployment and atomically switches
`active.json`. Host
loaders always use the executable from that same bundle. Inspect or validate
the active deployment with:

```sh
wisp deploy status
wisp deploy status --json
wisp deploy verify
wisp deploy prune
```

Pruning retains the active and previous bundles. Set `WISP_DEPLOY_ROOT` to
override the platform data directory and `WISP_WEZTERM_CONFIG_DIR` to override
the WezTerm configuration directory.

## Configuration

The default configuration path is `$XDG_CONFIG_HOME/wisp/config.toml`. Without
`XDG_CONFIG_HOME`, Wisp uses the platform configuration directory, including
`~/.config/wisp/config.toml` on Linux, `~/Library/Application Support/wisp/config.toml`
on macOS, and `%APPDATA%\wisp\config.toml` on Windows.

Set `WISP_CONFIG_FILE` or pass the global `--config <path>` option to use a
different file.

```toml
version = 3
cache_ttl_seconds = 60
follow_symlinks = false

[[roots]]
path = "~/Repos"
group = "Repos"

[[roots]]
path = "~/work"
group = "Work"

[[projects]]
id = "dotfiles"
path = "~/.config"
group = "Home"
name = "dotfiles"
display_name = "Dotfiles"

[openers]
file = ["nvim", "{path}"]

[opencode]
server_url = "http://127.0.0.1:4096"
command = ["opencode"]
session_limit = 100
```

Each immediate directory under a root becomes a project. Fixed projects need
only `path`; `id`, `group`, `name`, and `display_name` are optional. Repeated
native paths are coalesced, while duplicate explicit IDs are rejected.

Openers are argv arrays, never shell strings. Supported placeholders are:

- `{path}`
- `{project.path}`
- `{project.id}`
- `{project.name}`
- `{project.group}`

`openers.file` is included in file selections. An optional `openers.project`
is included in project selections. The picker itself never executes either.

The optional `opencode` table enables session mode. `server_url` must be a
loopback HTTP URL. `command` is an argv prefix and defaults to `["opencode"]`;
`session_limit` defaults to 100. Wisp checks the server's exact supported
OpenCode version before reading session data. If the server uses Basic Auth,
set `OPENCODE_SERVER_PASSWORD` and optionally `OPENCODE_SERVER_USERNAME` in the
environment that launches Wisp.

Validate configuration without starting the UI:

```sh
wisp config validate
```

## Commands

Running `wisp` without a subcommand is equivalent to `wisp pick`.

```text
wisp pick
wisp pick --result-file <path> --host-context-file <path> --initial-view projects|windows|sessions [--disable-sessions]
wisp projects --json
wisp refresh
wisp cache clear
wisp config validate
wisp deploy
wisp deploy verify
wisp deploy status --json
wisp deploy prune
wisp opencode install
wisp opencode status --json
wisp open <selection-json>
```

`pick` writes a versioned selection envelope to stdout after restoring the
terminal. Embedded integrations use `--result-file`; Wisp writes that file by
atomic same-directory replacement. Cancellation is a successful `cancelled`
result. Handled errors produce an `error` result and a nonzero process status.
`projects --json` also returns a protocol-versioned envelope rather than a raw
project array. Embedded hosts that cannot apply OpenCode selections pass
`--disable-sessions`, which makes session mode unavailable in that picker.

`open` is the only command that executes a resolved opener. It launches argv
directly without a shell. For example:

```sh
wisp pick --result-file /tmp/wisp-selection.json
wisp open "$(cat /tmp/wisp-selection.json)"
```

## Picker Keys

| Key | Action |
| --- | --- |
| `Up` / `Down`, `j` / `k` | Move in the focused pane |
| `Left` / `Right`, `h` / `l`, `Tab` | Change pane focus |
| `Enter` | Select a project, host workspace, window, file, or session; enter a directory |
| `w` | Show Windows and focus the detail pane |
| `f` | Show Files and focus the detail pane |
| `s` | Show OpenCode Sessions and focus the detail pane |
| `x` | Close the selected open project or host workspace from the Projects pane and exit |
| `/` | Enter fuzzy search for the focused pane |
| `Backspace` | Go to the parent directory; at the project root focus Projects |
| `Ctrl-R` | Force-refresh projects or the active detail listing |
| `Esc`, `q`, `Ctrl-C` | Cancel |

In search mode, printable characters update the focused pane's query,
`Backspace` edits it, `Esc` returns to normal mode while retaining the query,
and `Enter` selects the current match. Project and detail queries are
independent.

When a host supplies context, projects and live host-only workspaces are grouped
by status: `◆` current, `●` open, then `○` new. Host-only workspaces are present
only while open, so only discovered projects can appear as new. The indicators
use green, cyan, and muted ANSI colors from the active terminal theme rather
than fixed RGB values.

Projects and live host-only workspaces remain in the left pane. The right pane
shows host windows, the selected project's files, or its OpenCode sessions.
Files and OpenCode sessions are unavailable for a host-only workspace. Pressing
`x` on a current or open row returns a host action rather than terminating
processes directly. The WezTerm adapter applies it by closing every pane in the
exact workspace; `x` has no effect for new or standalone projects.

File browsing lists only the current directory. Child directories are read
when entered rather than indexed recursively.

Session rows show the OpenCode agent and whether the session is a root or child.
States are ordered by waiting for a question or permission, retrying, running,
idle, then error. OpenCode has no terminal completed state: an idle session can
receive another prompt later. Pending questions take display precedence over
permissions, and both counts are shown when both exist. Error events remain
visible until that session starts running or retrying again.

## WezTerm

`wisp deploy` installs the stable WezTerm bootstrap as
`wisp/init.lua` under `wezterm.config_dir`.

```lua
local wezterm = require "wezterm"
local config = wezterm.config_builder()
local wisp = dofile(wezterm.config_dir .. "/wisp/init.lua")

wisp.apply_to_config(config, {
  spawn_domain = { DomainName = "local" },
})

config.keys = config.keys or {}
table.insert(config.keys, { key = "s", mods = "LEADER", action = wisp.project_picker_action() })
table.insert(config.keys, { key = "w", mods = "LEADER", action = wisp.window_picker_action() })
table.insert(config.keys, { key = "o", mods = "LEADER", action = wisp.opencode_picker_action() })

return config
```

`apply_to_config` installs no binding unless the optional `picker_binding` is
present; that convenience option binds the project-focused picker. Roots, fixed
projects, cache settings, and openers belong in shared TOML, not in the Lua
options.

By default, the adapter owns WezTerm's right status area. It shows the full
workspace name followed by `wait`, `run`, `retry`, `idle`, and `err` counts for
fresh OpenCode plugin registrations. Counts refresh every two seconds and retain
their last valid values across transient failures. Set `status_bar = false` to
leave the right status area untouched.

The picker actions query `wisp projects --json`, snapshot every live workspace
and its tabs, map configured project workspaces to `current`, `open`, and `new`
labels, and include unmatched workspaces as host-only rows. They then launch
`wisp pick` as the sole process in a temporary tab. The project action starts
with Projects focused; the window action starts on the active tab of the current
workspace. A completed result closes the owned picker tab and applies the
selection through the original window and pane.

### WezTerm Options

| Option | Default | Purpose |
| --- | --- | --- |
| `config_file` | platform default | Shared TOML override |
| `picker_binding` | none | Optional key assignment for the project picker |
| `spawn_domain` | `{ DomainName = "local" }` | Named same-host domain for projects |
| `picker_domain` | `spawn_domain` | Named domain for the temporary picker tab |
| `workspace_prefix` | `"wisp:"` | Prefix for generated `group/name` workspaces |
| `workspace_for_project` | none | Callback returning a workspace name |
| `domain_for_project` | none | Callback returning `{ DomainName = name }` |
| `poll_interval_seconds` | `0.05` | Atomic result polling interval |
| `picker_timeout_seconds` | `3600` | Missing-result timeout |
| `status_bar` | `true` | Install the workspace and OpenCode right-status renderer |
| `status_interval_seconds` | `2` | Minimum interval between OpenCode status queries |
| `status_colors` | built-in OldBook palette | Partial status color override table |

`status_colors` accepts only `foreground`, `workspace_background`,
`active_workspace_background`, `waiting_background`, `running_background`,
`retrying_background`, `idle_background`, and `error_background`.

Mux workspace names and domains remain host policy:

```lua
wisp.apply_to_config(config, {
  spawn_domain = { DomainName = "unix" },
  workspace_for_project = function(project)
    return "project:" .. project.id
  end,
})
```

### WezTerm Actions

The adapter exports action constructors for user-owned mappings:

```lua
wisp.project_picker_action()
wisp.window_picker_action()
wisp.opencode_picker_action()
wisp.refresh_cache_action()
wisp.switch_to_project_action "dotfiles"
wisp.new_tab_action()
wisp.split_pane_action("Right", false)
```

Project workspaces and project-aware tabs/splits set `WISP_PROJECT_DIR` and
`WISP_PROJECT_NAME`. File selections launch `wisp open` as the initial process
in a new workspace or a new tab in an existing workspace; the adapter never
executes opener argv itself. Window selections activate the exact tab captured
when the picker launched. Host-only workspace selections use WezTerm's
existing-workspace API, so a stale selection cannot recreate a closed
workspace. Closing a project or host-only workspace terminates all panes in the
selected workspace through `wezterm cli kill-pane`.

## OpenCode

Phase one uses a user-managed OpenCode server on the loopback URL configured in
Wisp TOML:

```sh
opencode serve --hostname 127.0.0.1 --port 4096
opencode attach http://127.0.0.1:4096 --dir "$PWD"
```

Wisp reads sessions, live status, pending permissions, and pending questions
from the server API. It listens to the global event stream for immediate
refreshes and retains session error events that are not represented by the
status endpoint. It also performs a periodic resnapshot so registry changes and
reconnections converge. The OpenCode server remains user-managed; Wisp does
not supervise it.

`wisp opencode install` adds an opt-in global plugin loader under
`~/.config/opencode/plugins/`. The plugin verifies the exact supported version
through OpenCode's in-process SDK transport, so ordinary TUIs do not need to
listen on a TCP port. It records selected-session activity and errors, and
aggregates pending permissions and questions across that session's recursive
subagent tree. Versioned atomic files are renewed every 30 seconds, when the
plugin also reconciles missed pending-request events. Wisp expires registrations
after 90 seconds. The plugin and shared server are aggregated, with duplicate
live session IDs surfaced as conflicts rather than assigned to an arbitrary
server.

`wisp opencode status --json` reports fresh plugin registrations, not historical
picker sessions. A launch counts as idle until OpenCode emits its first selected
session event. Pending questions or permissions then take precedence over
activity; retrying, running, persisted session errors, and idle are otherwise
counted separately. Status reads the event-backed registry without contacting
each launch's server URL, which is necessary because an ordinary TUI's API is
private to its process. Conflicting live registrations count as errors. The
command does not require an `[opencode]` shared-server configuration.

Selecting a session first tries its recorded WezTerm tab or pane. OpenCode
1.18.15 does not expose later in-TUI session switches to v1 plugins, so this
focus mapping is best effort and can become stale after switching sessions
inside OpenCode. If the recorded target is missing, Wisp opens a new project tab
and runs the resolved `opencode attach ... --session ...` argv through
`wisp open`. Wisp uses only recorded opaque IDs, never searches for a pane by
title or path, and never invokes a shell.

## Neovim

Add the stable runtime installed by `wisp deploy`, not a repository checkout:

```lua
local wisp_root = vim.env.WISP_DEPLOY_ROOT
  or (vim.fs.dirname(vim.fn.stdpath "data") .. "/wisp")
vim.opt.runtimepath:prepend(wisp_root .. "/nvim")

require("wisp").setup {
  keymap = "<leader>wp",
}
```

`:Wisp` opens `wisp pick` in a centered floating terminal. Results are applied
to the tab that launched the picker, even if another tab becomes active:

- Project selection sets tab-local cwd with `:tcd`.
- File selection sets tab-local cwd and edits the file.
- `vim.t.wisp_project_dir` and `vim.t.wisp_project_name` store project metadata.
- Initial metadata is seeded from `WISP_PROJECT_DIR` and `WISP_PROJECT_NAME`.

The Neovim adapter disables OpenCode session mode because this first release
implements session focus and attach behavior only in the WezTerm adapter.

Setup options are `config_file`, `command`, `keymap`, `keymap_options`,
`width`, `height`, and `border`. See `:help wisp` for the compact reference.

## Protocol

Project listing uses a versioned envelope so separately installed adapters and
executables reject mismatched schemas:

```json
{
  "protocol_version": 3,
  "projects": [
    {
      "id": "api",
      "path": "/home/user/Repos/api",
      "group": "Repos",
      "name": "api",
      "display_name": "API"
    }
  ]
}
```

Selection protocol version 3 embeds the owning project and resolved opener:

```json
{
  "protocol_version": 3,
  "status": "selected",
  "selection": {
    "kind": "file",
    "project": {
      "id": "api",
      "path": "/home/user/Repos/api",
      "group": "Repos",
      "name": "api",
      "display_name": "API"
    },
    "path": "/home/user/Repos/api/src/main.rs",
    "opener": ["nvim", "/home/user/Repos/api/src/main.rs"]
  }
}
```

OpenCode status uses a separate strict envelope consumed by the WezTerm status
renderer:

```json
{
  "protocol_version": 3,
  "sessions": {
    "waiting": 1,
    "running": 2,
    "retrying": 0,
    "idle": 3,
    "error": 0
  }
}
```

Host-managed project closure uses the same envelope with a
`"kind": "close_project"` selection containing the project. Host window
selection uses `"kind": "host_item"` with the project and an opaque `"id"`.
Host-only rows use `"workspace"`, `"workspace_item"`, and `"close_workspace"`
selections carrying the exact `"workspace"` name; workspace items also carry an
opaque `"id"`. Standalone Wisp cannot produce these selections without host
context, and `wisp open` does not execute them.

OpenCode session selection uses `"kind": "open_code_session"` with the owning
project, session ID, resolved attach argv, and an optional opaque
`host_item_id`. Unlike host-window actions, `wisp open` executes the attach argv
directly when the host cannot focus the exact target.

Host context is a separate versioned input. Project entries are keyed by project
ID, while live host-only entries are keyed by exact workspace name. Project
labels control status and both entry types can describe host-owned windows:

```json
{
  "protocol_version": 3,
  "projects": {
    "api": {
      "labels": ["current", "open"],
      "items": [
        {
          "id": "17",
          "label": "nvim",
          "detail": "src/main.rs",
          "active": true
        }
      ],
      "session_items": {
        "ses_123": "tab:17"
      }
    },
    "dotfiles": {
      "labels": ["new"]
    }
  },
  "workspaces": {
    "default": {
      "current": false,
      "items": [
        {
          "id": "29",
          "label": "shell"
        }
      ]
    }
  }
}
```

The `workspaces` map is required; each key exists only while that workspace is
open, and `current` selects the current row. Omitted `items` and `session_items`
fields are empty. Host item IDs are opaque to the Rust picker. Adapters reject
protocol versions other than 3 rather than attempting compatibility. Canonical
examples live in [`tests/fixtures`](tests/fixtures).

## Cache And Limits

Wisp stores versioned JSON under the platform cache directory. Every record
contains a native path, normalized identity, scan time, and typed immediate
entries. Writes are locked and atomically replaced. TTL expiry, config changes,
schema changes, `Ctrl-R`, `wisp refresh`, and `wisp cache clear` invalidate the
appropriate records. There is no daemon.

Directory symlinks are traversed only when `follow_symlinks = true`. Native
paths remain in results; normalized keys are used only for identity and
deduplication.

Wisp organizes live mux processes. It does not restore commands after a mux
server exits or the host restarts, and Wisp itself runs no daemon.

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
lua tests/run.lua
stylua --check .
```

CI covers Rust and Lua on Linux, macOS, and Windows, parses the WezTerm fixture
at the minimum supported version, and loads the adapter in Neovim 0.10.4.
Version tags matching `v*` publish binary archives and SHA-256 checksums.

## License

[MIT](LICENSE)
