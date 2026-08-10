# Standalone Architecture Plan

## Goal

Separate Wisp's project and filesystem logic from its user interface and host
actions. The same standalone terminal UI must run directly, from WezTerm, and
from Neovim without embedding either host in the core or UI.

## Decisions

- Implement the core and standalone UI in Rust using Ratatui.
- Store shared roots, fixed projects, cache policy, and opener argv in Wisp
  TOML under the platform configuration directory.
- Return a versioned structured selection by default rather than executing a
  host action.
- Keep file search scoped to the current directory and navigate lazily.
- Persist a versioned per-directory cache on disk; do not add a daemon.
- Use atomic result files for embedded host integrations.
- Run the Neovim UI in a floating terminal and apply selections to the
  originating tab with tab-local cwd.
- Run the WezTerm UI in a temporary tab and provide optional host context with
  project labels and host-owned window items.
- Bundle the Neovim adapter under `nvim/`; consumers add only that subdirectory
  to runtimepath because WezTerm requires the repository-root `plugin/init.lua`.
- Require the `wisp` executable on `PATH`; adapters do not download or build it.

## Boundaries

```text
wisp.toml
    |
wisp-core  <-> versioned disk cache
    |
wisp-tui
    |
wisp CLI -> SelectionEnvelope
               |
       +-------+--------+
       |                |
 WezTerm adapter   Neovim adapter
```

`wisp-core` owns configuration, path identity, discovery, typed entries,
deduplication, cache persistence, navigation state, opener expansion, and
protocol models. It has no terminal, WezTerm, or Neovim dependency.

`wisp-tui` owns Ratatui rendering, key handling, fuzzy filtering, and screen
transitions. It receives core models and returns a selection. It does not read
configuration, access host APIs, or serialize host protocols.

`wisp-cli` loads configuration and cache state, runs the TUI, and writes a
selection to stdout or an atomic result file. It also provides noninteractive
project, refresh, cache, and validation commands.

The Lua adapters launch the executable, provide optional host context, validate
the result protocol, and translate selections into host actions.

## Repository Layout

```text
Cargo.toml
crates/
  wisp-core/
  wisp-tui/
  wisp-cli/
plugin/
  init.lua
nvim/
  lua/wisp/init.lua
  doc/wisp.txt
```

## Configuration

The default path is `$XDG_CONFIG_HOME/wisp/config.toml`, with platform-native
fallbacks provided by the Rust `directories` crate.

```toml
version = 1
cache_ttl_seconds = 60
follow_symlinks = false

[[roots]]
path = "~/Repos"
group = "Repos"

[[projects]]
id = "artifacts"
path = "~/Artifacts"
group = "Home"
name = "Artifacts"

[openers]
file = ["nvim", "{path}"]
```

Openers are argv arrays, never shell command strings. Supported placeholders
are `{path}`, `{project.path}`, `{project.id}`, `{project.name}`, and
`{project.group}`.

WezTerm workspace names, domains, and bindings remain adapter configuration.
Neovim mappings and float dimensions remain adapter configuration.

## Selection Protocol

Every response uses protocol version 2. Selected files include their owning
project and the resolved opener argv.

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
      "display_name": "api"
    },
    "path": "/home/user/Repos/api/src/main.rs",
    "opener": ["nvim", "/home/user/Repos/api/src/main.rs"]
  }
}
```

Cancellation returns a `cancelled` envelope with process status 0. Handled
errors return an `error` envelope and a nonzero process status. Embedded mode
writes the envelope to a uniquely named file through same-directory atomic
replacement.

Host context is a separate versioned JSON input keyed by project ID. Each
project has presentation labels and generic host items with opaque IDs, labels,
optional details, and active state. The TUI recognizes `current`, `open`, and
`new`, groups current and open projects before new projects, and renders
theme-colored status indicators. A `host_item` result returns the owning
project and opaque item ID to the host adapter.

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
    }
  }
}
```

## Commands

```text
wisp pick
wisp pick --result-file <path> --host-context-file <path> --initial-view projects|windows
wisp projects --json
wisp refresh
wisp cache clear
wisp config validate
wisp open <selection-json>
```

Running `wisp` without a subcommand is equivalent to `wisp pick`. Standalone
selection JSON is written to stdout after the alternate screen is restored.

## TUI Flow

1. Keep projects visible on the left, ordered by optional status indicators.
2. Show host windows or lazy filesystem entries for the selected project on
   the right; stack the panes when the terminal is narrow.
3. Selecting a project returns a project selection. Selecting a host window
   returns a `host_item` selection.
4. `w` shows Windows, `f` shows Files, and `/` enters fuzzy search for the
   focused pane.
5. Selecting a directory enters it; selecting a file returns it.
6. `x` on an open project returns a `close_project` selection and exits.
7. Backspace navigates to the parent directory and returns focus to Projects at
   the project root. Escape, `q`, or Ctrl-C cancels.
8. Ctrl-R refreshes projects or the active filesystem listing. Host windows
   remain a launch-time snapshot.

## Cache

The cache is stored under the platform cache directory as versioned JSON. Each
directory record contains the native path, normalized key, scan timestamp, and
typed entries. Writes are locked and atomically replaced. Cache records are
invalidated by TTL expiry, configuration fingerprint changes, explicit
refresh, or cache schema changes.

Rust filesystem metadata classifies directories, files, symlinks, and other
entries. Native paths are retained for results; normalized comparison keys are
used for deduplication. Directories below a project are loaded only when the
user navigates into them.

## WezTerm Adapter

The WezTerm adapter launches `wisp pick` in a temporary tab using an explicit
same-host domain. It snapshots matching workspace tabs into a temporary host
context file, polls the atomic result file with `wezterm.time.call_after`, and
retains the original window and pane for the resulting action.

Project results create or switch workspaces. File results create the project
workspace with the resolved opener or spawn a new tab in an existing workspace.
Host-item results validate that the captured tab still belongs to the selected
project workspace before activating it. Close-project results terminate every
pane in that project's workspace. The adapter queries `wisp projects --json`
for direct-project, project-aware tab, and split actions. It contains no path
discovery, cache, or picker UI.

## Neovim Adapter

Consumers add only `<checkout>/nvim` to runtimepath and call
`require("wisp").setup()`. The adapter exposes `:Wisp` and optional mappings.

The command opens a centered floating PTY terminal running `wisp pick` with an
atomic result path. On exit, it closes the float and applies the result to the
originating tab. Project results set tab-local cwd. File results set tab-local
cwd and edit the file in the originating tab. Metadata is stored in
`vim.t.wisp_project_dir` and `vim.t.wisp_project_name`; the initial tab may be
seeded from `WISP_PROJECT_DIR` and `WISP_PROJECT_NAME`.

## Migration

1. Add protocol fixtures and build the Rust core beside the working Lua plugin.
2. Implement and verify the persistent cache and navigation state machine.
3. Implement the Ratatui UI and standalone CLI.
4. Replace the WezTerm selector and discovery logic with the process adapter.
5. Add the bundled Neovim adapter.
6. Move Artifacts roots and fixed projects into Wisp TOML.
7. Remove superseded Lua discovery/cache/UI code after cross-host parity tests.
8. Add release binaries and installation documentation for Linux, macOS, and
   Windows.

## Completion Criteria

- Standalone Wisp operates with only the binary and TOML config.
- Standalone, WezTerm, and Neovim return the same projects from one config.
- The core has no terminal or host dependency.
- The TUI has no WezTerm, Neovim, filesystem, cache, or output transport code.
- Neovim changes only the originating tab's cwd and opens selected files there.
- WezTerm preserves workspace behavior and supplies project labels and tabs as
  host context.
- No platform-specific shell command is required.
- Rust, Lua, WezTerm, Neovim, shell, formatting, and cross-platform CI checks
  pass.
