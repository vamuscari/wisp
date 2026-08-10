# Wisp

Wisp is a project and file picker for [WezTerm](https://wezterm.org/). It finds
projects under local roots, opens each project in a stable workspace, and lets
you browse files lazily without shelling out to platform-specific tools.

## Requirements

Wisp requires WezTerm `20240127-113634-bbcac864` or newer. This is the first
release that supports the `InputSelector.fuzzy_description` and URL behavior
used by Wisp.

## Installation

```lua
local wezterm = require "wezterm"
local config = wezterm.config_builder()
local wisp = wezterm.plugin.require "https://github.com/vamuscari/wisp"

wisp.apply_to_config(config, {
  roots = {
    "~/Repos",
    { path = "~/work", group = "Work" },
  },
  projects = {
    {
      id = "dotfiles",
      group = "Home",
      name = "Dotfiles",
      path = "~/.config",
    },
  },
  picker_binding = { key = "f", mods = "LEADER" },
  cache_ttl_seconds = 60,
  open_file = { "nvim" },
})

return config
```

Wisp installs only `picker_binding` when that option is present. It does not
ship personal roots, key assignments, an editor, a theme, or mux policy.

## Picker Flow

The first fuzzy selector contains projects only. Selecting a project opens an
action menu:

- `Open workspace` creates or switches to that project's workspace.
- `Browse files` opens a fuzzy listing of the project root.

File browsing is hierarchical. Select a directory to enter it, select `..` to
return to its parent, or select `Project actions` at the project root. Selecting
a non-directory passes its absolute path to the configured file opener.
Cancellation at every level is a no-op.

## Options

### `roots`

An array of local directories whose immediate child directories are projects.
A root can be a string or a table:

```lua
roots = {
  "~/Repos",
  {
    path = "~/work",
    group = "Work",
    domain = { DomainName = "unix" },
  },
}
```

`group` defaults to the root directory name. `domain` accepts a
`{ DomainName = name }` table and overrides the spawn domain for projects
discovered under that root.

### `projects`

An array of fixed local projects. Only `path` is required.

```lua
projects = {
  {
    id = "wisp",
    path = "~/Repos/wisp",
    group = "Plugins",
    name = "wisp",
    display_name = "Wisp",
    workspace = "wisp:Plugins/wisp",
    domain = { DomainName = "local" },
  },
}
```

`id` defaults to the native project path and is used by
`switch_to_project_action`. `name` defaults to the path basename,
`display_name` defaults to `name`, `group` defaults to `Projects`, and
`workspace` defaults to `wisp:group/name`. Duplicate normalized paths are
coalesced. Distinct projects may not share an ID or workspace.

### `spawn_domain`

The default named domain for project processes. It defaults to the explicit
local domain:

```lua
spawn_domain = { DomainName = "local" }
```

Set a stable named domain when projects should run in a same-host mux server:

```lua
spawn_domain = { DomainName = "unix" }
```

Only `{ DomainName = name }` is accepted for project domains. Wisp intentionally
rejects `DefaultDomain`, `CurrentPaneDomain`, and domain IDs so a known project
cannot silently change hosts. Discovery and file browsing always run on the
local host, so remote project paths are not supported by the initial release.

### `cache_ttl_seconds`

Filesystem listings are cached independently for 60 seconds by default. Root
and project listings are loaded during discovery; deeper directories are read
only when entered. Every immediate entry is retained in the cache, including
files.

Set the TTL to `0` to disable reuse. Use `refresh_cache_action()` to clear all
entries and silently preload roots and fixed projects.

### `open_file`

An argv array or function that returns argv. Wisp appends the selected path to
an array:

```lua
open_file = { "nvim", "--clean" }
```

A function receives the project and selected path and must return complete
argv:

```lua
open_file = function(project, path)
  return { "nvim", "+cd " .. project.path, path }
end
```

Wisp does not invoke a shell. For a new workspace, the file command is its
initial process. For an open workspace, Wisp creates a tab in one of that
workspace's mux windows and then switches to it.

## Actions

Wisp exports action constructors so mappings remain in user configuration:

```lua
table.insert(config.keys, {
  key = "R",
  mods = "LEADER|SHIFT",
  action = wisp.refresh_cache_action(),
})

table.insert(config.keys, {
  key = "t",
  mods = "LEADER",
  action = wisp.switch_to_project_action "dotfiles",
})

table.insert(config.keys, {
  key = "c",
  mods = "LEADER",
  action = wisp.new_tab_action(),
})

table.insert(config.keys, {
  key = "|",
  mods = "LEADER|SHIFT",
  action = wisp.split_pane_action("Right", false),
})
```

Available constructors:

- `project_picker_action()`
- `refresh_cache_action()`
- `switch_to_project_action(project_id)`
- `new_tab_action()`
- `split_pane_action(direction, top_level)`

Project-aware tabs and splits preserve a known local pane working directory
and set `WISP_PROJECT_DIR` and `WISP_PROJECT_NAME`. Unknown workspaces fall back
to `CurrentPaneDomain` without project metadata.

## Updating

`wezterm.plugin.require` does not update a previously cloned plugin. Run this
from WezTerm's Debug Overlay, then reload the configuration:

```lua
wezterm.plugin.update_all()
wezterm.reload_configuration()
```

For local development, use a file URL:

```lua
local wisp = wezterm.plugin.require "file:///absolute/path/to/wisp"
```

Local plugin changes must also be synchronized with `update_all()` before a
configuration reload.

## Compatibility And Limits

CI runs the Lua suite on Linux, macOS, and Windows and parses the integration
fixture with the minimum supported WezTerm AppImage. Wisp uses documented
WezTerm and Lua APIs without operating-system commands so that BSD remains a
compatibility target, but BSD is not yet continuously verified.

Workspace names belong to the mux globally. The `wisp:` prefix reduces
collisions, but an unrelated pre-existing workspace with the same name still
wins because WezTerm applies `SwitchToWorkspace.spawn` only when creating a
workspace.

Wisp organizes live mux processes; it does not restore commands after the mux
server exits or the host restarts.

## Development

Run the test suite and formatter from the repository root:

```sh
lua tests/run.lua
stylua --check .
```

## License

[MIT](LICENSE)
