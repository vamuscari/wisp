# Wisp

Wisp is a standalone terminal picker for projects, host windows, and files. A
Rust core owns configuration, local discovery, typed filesystem entries, lazy
navigation, and a persistent cache. The same Ratatui interface runs directly,
in a temporary WezTerm tab, or in a Neovim floating terminal.

Host integrations are intentionally thin. They receive a versioned selection
and apply host-specific behavior; neither adapter discovers projects or embeds
its own picker UI.

## Requirements

- The `wisp` executable on `PATH`
- WezTerm `20240127-113634-bbcac864` or newer for the WezTerm adapter
- Neovim `0.10.4` or newer for the Neovim adapter

Project discovery is local to the machine running `wisp`. A configured named
WezTerm domain may point at a same-host mux server, but remote project paths are
not supported.

## Install The Binary

Download an archive for Linux, macOS, or Windows from
[GitHub Releases](https://github.com/vamuscari/wisp/releases), then place
`wisp` (or `wisp.exe`) on `PATH`.

To install from source with Rust 1.85 or newer:

```sh
cargo install --git https://github.com/vamuscari/wisp --locked wisp-cli
```

From a local checkout:

```sh
cargo install --path crates/wisp-cli --locked
```

Installing the WezTerm or Neovim adapter does not install the executable.

## Configuration

The default configuration path is `$XDG_CONFIG_HOME/wisp/config.toml`. Without
`XDG_CONFIG_HOME`, Wisp uses the platform configuration directory, including
`~/.config/wisp/config.toml` on Linux, `~/Library/Application Support/wisp/config.toml`
on macOS, and `%APPDATA%\wisp\config.toml` on Windows.

Set `WISP_CONFIG_FILE` or pass the global `--config <path>` option to use a
different file.

```toml
version = 1
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

Validate configuration without starting the UI:

```sh
wisp config validate
```

## Commands

Running `wisp` without a subcommand is equivalent to `wisp pick`.

```text
wisp pick
wisp pick --result-file <path> --host-context-file <path> --initial-view projects|windows
wisp projects --json
wisp refresh
wisp cache clear
wisp config validate
wisp open <selection-json>
```

`pick` writes a versioned selection envelope to stdout after restoring the
terminal. Embedded integrations use `--result-file`; Wisp writes that file by
atomic same-directory replacement. Cancellation is a successful `cancelled`
result. Handled errors produce an `error` result and a nonzero process status.

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
| `Enter` | Select a project, window, or file; enter a directory |
| `w` | Show Windows and focus the detail pane |
| `f` | Show Files and focus the detail pane |
| `x` | Close the selected open project from the Projects pane and exit |
| `/` | Enter fuzzy search for the focused pane |
| `Backspace` | Go to the parent directory; at the project root focus Projects |
| `Ctrl-R` | Force-refresh projects or the active directory listing |
| `Esc`, `q`, `Ctrl-C` | Cancel |

In search mode, printable characters update the focused pane's query,
`Backspace` edits it, `Esc` returns to normal mode while retaining the query,
and `Enter` selects the current match. Project and detail queries are
independent.

When a host supplies context, projects are grouped by status:
`◆` current, `●` open, then `○` new. Current and open projects appear before
new projects. The indicators use green, cyan, and muted ANSI colors from the
active terminal theme rather than fixed RGB values.

Projects remain in the left pane. The right pane shows either host windows or
the selected project's files. Pressing `x` on a current or open project returns
a host action rather than terminating processes directly. The WezTerm adapter
applies it by closing every pane in that project's workspace; `x` has no effect
for new or standalone projects.

File browsing lists only the current directory. Child directories are read
when entered rather than indexed recursively.

## WezTerm

The WezTerm adapter can be installed as a normal plugin. The executable remains
a separate requirement.

```lua
local wezterm = require "wezterm"
local config = wezterm.config_builder()
local wisp = wezterm.plugin.require "https://github.com/vamuscari/wisp"

wisp.apply_to_config(config, {
  spawn_domain = { DomainName = "local" },
})

config.keys = config.keys or {}
table.insert(config.keys, { key = "s", mods = "LEADER", action = wisp.project_picker_action() })
table.insert(config.keys, { key = "w", mods = "LEADER", action = wisp.window_picker_action() })

return config
```

`apply_to_config` installs no binding unless the optional `picker_binding` is
present; that convenience option binds the project-focused picker. Roots, fixed
projects, cache settings, and openers belong in shared TOML, not in the Lua
options.

The picker actions query `wisp projects --json`, snapshot project tabs and
`current`, `open`, and `new` labels into host context, and launch `wisp pick` as
the sole process in a temporary tab. The project action starts with Projects
focused; the window action starts on the active tab of the current project. A
completed result closes the owned picker tab and applies the selection through
the original window and pane.

### WezTerm Options

| Option | Default | Purpose |
| --- | --- | --- |
| `wisp_path` | `"wisp"` | Executable name or absolute path |
| `config_file` | platform default | Shared TOML override |
| `picker_binding` | none | Optional key assignment for the project picker |
| `spawn_domain` | `{ DomainName = "local" }` | Named same-host domain for projects |
| `picker_domain` | `spawn_domain` | Named domain for the temporary picker tab |
| `workspace_prefix` | `"wisp:"` | Prefix for generated `group/name` workspaces |
| `workspace_for_project` | none | Callback returning a workspace name |
| `domain_for_project` | none | Callback returning `{ DomainName = name }` |
| `poll_interval_seconds` | `0.05` | Atomic result polling interval |
| `picker_timeout_seconds` | `3600` | Missing-result timeout |

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
wisp.refresh_cache_action()
wisp.switch_to_project_action "dotfiles"
wisp.new_tab_action()
wisp.split_pane_action("Right", false)
```

Project workspaces and project-aware tabs/splits set `WISP_PROJECT_DIR` and
`WISP_PROJECT_NAME`. File selections become the initial process in a new
workspace or a new tab in an existing workspace. Window selections activate
the exact tab captured when the picker launched. Closing a project terminates
all panes in the selected project workspace through `wezterm cli kill-pane`.

For local adapter development, load only the WezTerm module:

```lua
local wisp = dofile(wezterm.home_dir .. "/Repos/wisp/plugin/init.lua")
```

## Neovim

Add only the checkout's `nvim/` directory to `runtimepath`. The repository root
also contains WezTerm's `plugin/init.lua` and must not be added.

```lua
vim.opt.runtimepath:prepend(vim.fn.expand "~/Repos/wisp/nvim")

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

Setup options are `wisp_path`, `config_file`, `command`, `keymap`,
`keymap_options`, `width`, `height`, and `border`. See `:help wisp` for the
compact reference.

## Protocol

Selection protocol version 2 embeds the owning project and resolved opener:

```json
{
  "protocol_version": 2,
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

Host-managed project closure uses the same envelope with a
`"kind": "close_project"` selection containing the project. Host window
selection uses `"kind": "host_item"` with the project and an opaque `"id"`.
Standalone Wisp cannot produce either selection without host context, and
`wisp open` does not execute them.

Host context is a separate versioned input keyed by project ID. Labels control
project status and items describe host-owned windows:

```json
{
  "protocol_version": 2,
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
      ]
    },
    "dotfiles": {
      "labels": ["new"]
    }
  }
}
```

An omitted `items` field is equivalent to an empty list. Host item IDs are
opaque to the Rust picker. Adapters reject protocol versions other than 2
rather than attempting compatibility. Canonical examples live in
[`tests/fixtures`](tests/fixtures).

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
server exits or the host restarts.

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
