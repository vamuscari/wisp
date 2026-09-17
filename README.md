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
- OpenCode `1.18.31` for optional session tracking

Clickable right-status providers additionally require a WezTerm build where
`wezterm.format` accepts `Hyperlink` and `EndHyperlink` items and linked status
cells dispatch `open-uri`. Wisp probes this capability when the adapter loads;
other supported WezTerm builds render the same status without click targets.

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

Deployment schemas are strict and are never migrated. If an installed Wisp
rejects an older `active.json`, explicitly discard only that incompatible
pointer while deploying the new bundle:

```sh
wisp deploy --replace-incompatible
```

The replacement starts a new deployment history with no previous bundle. It
does not bypass malformed state that already claims the current schema.

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

Pruning retains the active and previous bundles. On Windows, stale bundles
locked by running processes are retained and retried by a later prune instead
of failing the whole operation. Set `WISP_DEPLOY_ROOT` to override the platform
data directory and `WISP_WEZTERM_CONFIG_DIR` to override the WezTerm
configuration directory.

## Configuration

The default configuration path is `$XDG_CONFIG_HOME/wisp/config.toml`. Without
`XDG_CONFIG_HOME`, Wisp uses the platform configuration directory, including
`~/.config/wisp/config.toml` on Linux, `~/Library/Application Support/wisp/config.toml`
on macOS, and `%APPDATA%\wisp\config.toml` on Windows.

Set `WISP_CONFIG_FILE` or pass the global `--config <path>` option to use a
different file.

```toml
version = 7
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
native paths are coalesced, while duplicate explicit IDs are rejected. Root and
fixed-project paths must be absolute after optional `~` expansion.

Openers are argv arrays, never shell strings. Supported placeholders are:

- `{path}`
- `{project.path}`
- `{project.id}`
- `{project.name}`
- `{project.group}`

`openers.file` is included in file selections. An optional `openers.project`
is included in project selections. The picker itself never executes either.

The optional `[vcs.icons]` table customizes Git markers for current and open
projects. Omitted keys use these defaults:

```toml
[vcs.icons]
clean = "✓"
dirty = "✗"
untracked = "?"
modified = "!"
staged = "+"
conflicted = "×"
ahead = "⇡"
behind = "⇣"
diverged = "⇕"
stashed = "*"
```

Each value must be a non-empty string or `false`; `false` disables that marker.
When `diverged` is disabled, Wisp renders separate `ahead` and `behind` markers.

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
wisp pick --result-file <path> --host-context-file <path> \
  [--active-project-path <path>] [--active-file <path>] \
  [--file-open-target window|right-pane|bottom-pane] \
  [--file-preview-state-file <path>] [--file-preview] \
  --initial-view projects|windows|sessions [--disable-sessions]
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
Adapters use `--active-project-path` and `--active-file` to identify the
originating editor context. Wisp accepts the file only when it is inside the
resolved active project. It prefers a matching explicit project, then a mapped
host-current project, then the deepest project containing the active file.

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
| `Left` / `Right`, `h` / `l`, `Tab` | Move through the Project, Window, Pane, or directory hierarchy |
| `Enter` | Open an unopened project with no host windows; otherwise descend into a project, window, or directory, or select the focused item |
| `Ctrl-T` | Open the selected file in a new Window, even when it is already visible |
| `Ctrl-V` | Open the selected file in a new right Pane, even when it is already visible |
| `Ctrl-X` | Open the selected file in a new bottom Pane, even when it is already visible |
| `o` | Jump directly to the selected project or host workspace without selecting a window or pane |
| `w` | Show Windows and focus the detail pane |
| `f` | Show Files and focus the detail pane |
| `s` | Show OpenCode Sessions and focus the detail pane |
| `x` | Close the selected open project or host workspace from the Projects pane and exit |
| `p` | Toggle terminal-text Preview in Windows or live file Preview in Files |
| `?` | Toggle the Commands pane |
| `/` | Enter fuzzy search for the focused pane |
| `Backspace` | Go to the parent directory; at the project root focus Projects |
| `Ctrl-R` | Force-refresh projects and open-project Git, or the active detail listing; Windows also refreshes a visible Preview |
| `Esc` | Close Commands when visible; otherwise cancel |
| `q`, `Ctrl-C` | Cancel |

In search mode, printable characters, including `p`, `?`, and `o`, update the
focused pane's query, `Backspace` edits it, `Esc` returns to normal mode while
retaining the query, and `Enter` selects the current match. Project and detail
queries are independent.

In Windows mode, `Enter` on a configured project that is not host-open and has
no host windows switches directly to its workspace. WezTerm creates one
default-shell window at the project directory when that workspace does not yet
exist. `o` always bypasses window and pane selection for direct project or host
workspace activation.

Projects and live host-only workspaces are grouped by status: `◆` current, `●`
open, then `○` new. Editor context can make a discovered project current for
selection and display without making it host-open. Host-only workspaces are
present only while open, so only discovered projects can appear as new. The
indicators use green, cyan, and muted ANSI colors from the active terminal theme
rather than fixed RGB values.

The current project row includes the active Neovim file as a project-relative
path when the current buffer is a normal file. Every current or open configured
project also includes the Git branch, clean or dirty state, and nonzero counts
for untracked, modified, staged, conflicted, upstream, and stashed states when
`git` can inspect it. Combined upstream divergence renders as `ahead/behind`.
This metadata stays inline in Projects; each Git summary is anchored to the
right edge while the active file path uses the remaining space and truncates
from the left when needed. Up to four Git inspections run after the picker opens
and rows update as they finish. The Projects pane uses a capped adaptive width
so most horizontal space remains available to the detail pane.

Projects and live host-only workspaces remain in the left pane. Windows mode
shows separate Window and Pane columns so selecting a split activates its exact
pane. Files mode uses retained Miller columns: highlighting a directory reads
only its immediate children, Enter or Right focuses that child column, and
Backspace or Left returns to an already loaded parent. Wide layouts show up to
four hierarchy levels including Projects, medium layouts show three, and narrow
layouts recycle the focused directory while retaining the full breadcrumb.
Directory reads use a debounced background worker, and request IDs discard
results superseded by a newer highlight.
Files already visible in a Neovim window have a `◆` marker. `Enter` revalidates
and reuses the best marked project target when possible. The three Ctrl keys
always create a duplicate in their explicit target and do nothing on a
directory.
There is no application title bar. A white-bordered utility bar at the bottom
shows the current mode and view, becomes the focused query input during search,
and reports status errors. Command hints live in the `?` Commands pane, which
uses the Preview region and restores its previous state when closed.
Files and OpenCode sessions are unavailable for a host-only workspace. Pressing
`x` on a host-current or host-open row returns a host action rather than
terminating processes directly. The WezTerm adapter applies it by closing every
pane in the exact workspace; `x` has no effect for new, standalone, or
editor-active-only projects.

File browsing has unlimited logical depth without recursive indexing. Only the
highlighted directory's immediate children are loaded.

Session rows show the OpenCode agent and whether the session is a root or child.
States are ordered by waiting for a question or permission, retrying, running,
idle, then error. OpenCode has no terminal completed state: an idle session can
receive another prompt later. Pending questions take display precedence over
permissions, and both counts are shown when both exist. Error events remain
visible until that session starts running or retrying again.

## WezTerm

`wisp deploy` installs the stable WezTerm bootstrap as `wisp/init.lua` beside
the active WezTerm configuration. Resolution prefers
`WISP_WEZTERM_CONFIG_DIR`, then the parent of `WEZTERM_CONFIG_FILE`, an existing
`$HOME/.wezterm.lua`, `$XDG_CONFIG_HOME/wezterm` when set, an existing
`$HOME/.config/wezterm/wezterm.lua`, and finally `$HOME` for the recommended
`.wezterm.lua` layout. Portable mode and `--config-file` launches should set
`WISP_WEZTERM_CONFIG_DIR` explicitly.

```lua
local wezterm = require "wezterm"
local config = wezterm.config_builder()
local wisp = dofile(wezterm.config_dir .. "/wisp/init.lua")

wisp.apply_to_config(config, {
  spawn_domain = { DomainName = "local" },
  opencode_tab_colors = true,
  status_items = {
    { name = "opencode", action = "sessions" },
    { name = "directory", action = "projects" },
  },
  popup = { direction = "Bottom", size = 0.65 },
  window_preview = true,
  file_open = { default = "window" },
  file_preview = {
    command = { "nvim" },
    direction = "Right",
    size = 0.5,
  },
})

config.keys = config.keys or {}
table.insert(config.keys, { key = "s", mods = "LEADER", action = wisp.project_picker_action() })
table.insert(config.keys, { key = "w", mods = "LEADER", action = wisp.window_picker_action() })
table.insert(config.keys, { key = "o", mods = "LEADER", action = wisp.opencode_picker_action() })
table.insert(config.keys, { key = "p", mods = "LEADER", action = wisp.popup_action "projects" })

return config
```

A complete configuration with a leader key and additional Wisp actions is in
[`examples/wezterm.lua`](examples/wezterm.lua).

`apply_to_config` installs no binding unless the optional `picker_binding` is
present; that convenience option binds the project-focused picker. Roots, fixed
projects, cache settings, and openers belong in shared TOML, not in the Lua
options.

By default, the adapter owns WezTerm's right status area and renders the bundled
`opencode` provider followed by `directory`. `opencode` shows `OC`, idle and
running counts, and optional waiting and failure counts. Failure is the sum of
retrying and error registrations. Waiting and failure are hidden at zero; each
flashes three times when it becomes nonzero, then remains solid. Counts refresh
every two seconds and retain their last valid values across transient failures.
`directory` shows the short workspace name. On a capable WezTerm build, clicking
the default providers opens Sessions or Projects in Wisp's popup. Set
`status_bar = false` to leave the right status area untouched.

Set `opencode_tab_colors = true` to color each tab containing a fresh OpenCode
pane. Wisp inspects every pane snapshot in that tab and uses the most urgent
state in `waiting > failure > running > idle` order. Those states use
`waiting_background`, `failure_background`, `running_background`, and
`idle_background` from `status_colors`; colors are steady even when the matching
right-status counts flash. Active colored tabs use bold titles. Explicit tab
titles are preserved, with the active pane title as fallback, and unaffected
tabs retain WezTerm's configured active, inactive, and hover formatting.

The bundled OpenCode plugin publishes pane-local state on events and renews it
every 30 seconds. Wisp ignores malformed values, future timestamps, and state
older than 90 seconds, while normal plugin shutdown clears the state
immediately. The option defaults to `false`. WezTerm executes only its first
`format-tab-title` handler, so enabling this option gives Wisp ownership of that
event and any other tab-title customization must be incorporated into Wisp.
The option requires `status_bar = true`: Wisp varies a zero-width status format
attribute on each update so WezTerm recomputes tab freshness without changing
the visible right status.

The picker actions query `wisp projects --json`, snapshot every live workspace,
tab, and pane, map configured project workspaces to `current`, `open`, and `new`
labels, and include unmatched workspaces as host-only rows. The standard
project, window, and OpenCode actions launch `wisp pick` as the sole process in
a temporary tab. `popup_action` and status-provider clicks launch the same exact
argv in a top-level split. Only one Wisp popup is active per GUI window; opening
another replaces it, and cancellation or selection closes only the owned split.
The project action starts with Projects focused; the window action starts on the
active tab and pane of the current workspace. A completed result applies the
selection through the original window and pane. `single_pane_behavior = "show"`
keeps the Pane column visible for one-pane windows; `"activate"` makes Enter on
such a window activate its sole pane immediately.

Window Preview is available on demand through `p` and starts hidden by default.
Set `window_preview = true` to start each picker with Preview visible. Hovering a
Windows row while Preview is visible changes its target without changing
selection; keyboard navigation previews the highlighted row. Wisp reads no pane
text while Preview is hidden or Commands is visible. It runs a bounded
`wezterm cli get-text` request only when Preview is shown, its target changes, or
`Ctrl-R` is pressed. Preview text remains in picker memory, is never logged or
cached, and is discarded when the target changes, Preview is hidden, or the
picker exits.

File Preview is disabled unless `file_preview` is configured. It starts visible
when Files mode is entered, follows the highlighted file in one owned Neovim
split, and closes when Files mode is left or `p` toggles it off. The command is
an argv array launched directly without a shell; use an absolute executable
path when the mux server's `PATH` does not include Neovim, and add user-owned
startup flags such as `--clean` when desired. Preview reads at most 1 MiB or
10,000 lines,
rejects binary NUL content, and uses an isolated read-only scratch buffer with
normal filetype detection and the active color scheme. When a matching live
editor view exists, the preview restores its captured cursor and viewport and
shows the source line range and dimensions.

When a pane is running Neovim with Wisp configured, the adapter reads its
strict `WISP_NVIM_STATE` payload before launching the picker. The state contains
every normal file window in the displayed Neovim tab, including viewport
metadata. A known non-Neovim foreground process rejects stale values; mux panes
where process inspection is unavailable use pane state directly. In an
unmanaged workspace, the active view can still identify its containing Wisp
project.

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
| `status_bar` | `true` | Install Wisp's right-status renderer |
| `status_items` | `opencode`, `directory` | Ordered bundled status providers and optional picker actions |
| `status_interval_seconds` | `2` | Minimum interval between OpenCode status queries |
| `status_colors` | built-in OldBook palette | Partial semantic status color table |
| `opencode_tab_colors` | `false` | Color tab backgrounds from fresh pane-local OpenCode state |
| `popup` | `{ direction = "Bottom", size = 0.65 }` | Top-level popup split placement and size |
| `window_preview` | `false` | Start with window Preview visible instead of on demand |
| `file_open` | `{ default = "window" }` | Default file target: `window`, `right_pane`, or `bottom_pane` |
| `file_preview` | none | Live preview command, split direction, and positive split size |
| `single_pane_behavior` | `"show"` | Show a one-pane Window's Pane column, or use `"activate"` to select it immediately |

`status_colors` accepts only `foreground`, `opencode_background`,
`workspace_background`, `active_workspace_background`, `idle_background`,
`running_background`, `waiting_background`, and `failure_background`. A main
WezTerm theme can derive these semantic roles from its selected scheme and pass
the resulting Lua table directly to Wisp.

`status_items` is a strict array of bundled providers. Available names are
`opencode` and `directory`; available actions are `projects`, `windows`, and
`sessions`. Array order controls render order, omitting an action makes that
provider display-only, and an empty array renders no Wisp status cells. Wisp
does not load provider modules or execute user-supplied commands.

`popup.direction` accepts `Top`, `Bottom`, `Left`, or `Right`. A `popup.size`
below `1` is a fraction of the available space; a value of `1` or greater is a
cell count, matching `pane:split` semantics.

`file_preview.command` must be a dense non-empty argv array.
`file_preview.direction` accepts the same four directions as `popup`, and
`file_preview.size` follows the same positive fraction-or-cell split semantics.

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
wisp.popup_action "projects" -- also accepts "windows" or "sessions"
wisp.refresh_cache_action()
wisp.switch_to_project_action "dotfiles"
wisp.new_tab_action()
wisp.split_pane_action("Right", false)
```

The three named picker actions use temporary tabs. `popup_action` uses the
configured top-level split and shares its singleton lifecycle with status
provider clicks.

Project workspaces and project-aware tabs/splits set `WISP_PROJECT_DIR` and
`WISP_PROJECT_NAME`. Tabs and splits preserve pane directories after converting
WezTerm file URLs to native drive or UNC paths on Windows. `Enter` first
revalidates the exact captured workspace, Window, Pane, and visible file. A live
target is activated without running an opener; a stale target falls back to the
configured default. Window targets launch `wisp open` in the first project
Window or a new Window in an existing workspace. Pane targets split the active
project Window right or bottom, but create the first Window when the project is
closed. The adapter never executes opener argv itself. Pane
selections validate and activate the exact workspace, tab, and pane captured
when the picker launched. Host-only
workspace selections use WezTerm's existing-workspace API, so a stale selection
cannot recreate a closed workspace. Closing a project or host-only workspace
terminates all panes in the selected workspace through `wezterm cli kill-pane`.

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
1.18.31 does not expose later in-TUI session switches to v1 plugins, so this
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
  file_open = { default = "window" },
  file_preview = { width = 0.5 },
}
```

`:Wisp` opens `wisp pick` in a centered floating terminal. Project and new-file
results are applied relative to the tab that launched the picker, even if
another tab becomes active:

- Project selection sets tab-local cwd with `:tcd`.
- `Enter` focuses a visible matching normal-file window before opening another.
- `window` creates a tab page; `right_pane` and `bottom_pane` create vertical
  and horizontal splits in the originating tab.
- The explicit Ctrl file targets always create their requested tab or split.
- `vim.t.wisp_project_dir` and `vim.t.wisp_project_name` store project metadata.
- Initial metadata is seeded from `WISP_PROJECT_DIR` and `WISP_PROJECT_NAME`.
- The originating normal-file buffer is shown inline on the current project.

When `file_preview` is configured, Files mode starts with a companion scratch
float to the right of the picker within its configured footprint. It uses the
same bounded renderer as WezTerm, retains focus in the picker terminal, and
restores the original picker dimensions when hidden. Selection, cancellation,
and failure stop the watcher and remove the temporary sidecar before applying a
result.

Inside WezTerm, the adapter publishes every normal file shown in the current
Neovim tab as strict protocol-v7 pane state. Unnamed, hidden, terminal,
quickfix, help, and other non-file buffers are omitted. Cursor and scroll-only
updates are debounced; tab, window, and buffer changes publish immediately. The
active view is also passed directly when `:Wisp` opens the picker.

The Neovim adapter disables OpenCode session mode because this first release
implements session focus and attach behavior only in the WezTerm adapter.

Setup options are `config_file`, `command`, `keymap`, `keymap_options`,
`width`, `height`, `border`, `file_open`, and `file_preview`. See `:help wisp`
for the compact reference.

## Protocol

Project listing uses a versioned envelope so separately installed adapters and
executables reject mismatched schemas:

```json
{
  "protocol_version": 7,
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

Selection protocol version 7 embeds the owning project, resolved opener, and
file-placement policy:

```json
{
  "protocol_version": 7,
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
    "opener": ["nvim", "/home/user/Repos/api/src/main.rs"],
    "open_target": "right_pane",
    "reuse_existing": true,
    "host_target": {
      "window_id": "17",
      "pane_id": "42"
    }
  }
}
```

OpenCode status uses a separate strict envelope consumed by the WezTerm status
renderer:

```json
{
  "protocol_version": 7,
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
`"kind": "close_project"` selection containing the project. Exact split
activation uses `"kind": "host_pane"` with the project, opaque `"window_id"`,
and opaque `"pane_id"`. Host-only rows use `"workspace"`, `"workspace_pane"`,
and `"close_workspace"` selections; pane selections carry the exact workspace,
window, and pane identities. Standalone Wisp cannot produce these selections
without host context, and `wisp open` does not execute them.

OpenCode session selection uses `"kind": "open_code_session"` with the owning
project, session ID, resolved attach argv, and an optional opaque
`host_item_id`. Unlike host-window actions, `wisp open` executes the attach argv
directly when the host cannot focus the exact target.

Host context is a separate versioned input. Project entries are keyed by project
ID, while live host-only entries are keyed by exact workspace name. Project
labels control status and both entry types describe host-owned windows with
nested panes. The WezTerm adapter carries every pane so selection and on-demand
Preview target the exact split captured at launch:

```json
{
  "protocol_version": 7,
  "projects": {
    "api": {
      "labels": ["current", "open"],
      "windows": [
        {
          "id": "17",
          "label": "editor",
          "detail": "src/main.rs",
          "active": true,
          "panes": [
            {
              "id": "42",
              "label": "nvim",
              "detail": "src/main.rs",
              "active": true,
              "nvim_views": [
                {
                  "window_id": "1001",
                  "path": "/home/user/Repos/api/src/main.rs",
                  "active": true,
                  "width": 120,
                  "height": 40,
                  "bottomline": 30,
                  "view": {
                    "lnum": 12,
                    "col": 0,
                    "coladd": 0,
                    "curswant": 0,
                    "topline": 4,
                    "topfill": 0,
                    "leftcol": 0,
                    "skipcol": 0
                  }
                }
              ]
            }
          ]
        }
      ],
      "session_items": {
        "ses_123": "pane:42"
      }
    },
    "dotfiles": {
      "labels": ["new"]
    }
  },
  "workspaces": {
    "default": {
      "current": false,
      "windows": [
        {
          "id": "29",
          "label": "shell",
          "panes": [
            {
              "id": "43",
              "label": "zsh"
            }
          ]
        }
      ]
    }
  }
}
```

The `workspaces` map is required; each key exists only while that workspace is
open, and `current` selects the current row. Omitted `windows` and
`session_items` fields are empty, while every window must contain at least one
pane. Window and pane IDs are opaque to the Rust picker. Adapters reject
protocol versions other than 7 rather than attempting compatibility. Canonical
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
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85.0 check --workspace --locked
node --check opencode/wisp.js
node --test tests/opencode_plugin_test.mjs tests/opencode_plugin_process_test.mjs
lua tests/run.lua
stylua --check .
```

CI covers Rust and Lua on Linux, macOS, and Windows, parses the WezTerm fixture
at the minimum supported version, and loads the adapter in Neovim 0.10.4.
Version tags matching `v*` publish binary archives and SHA-256 checksums.

## License

[MIT](LICENSE)
