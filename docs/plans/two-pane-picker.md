# Two-Pane Picker Plan

## Goal

Give Wisp a tmux-style project and window chooser while preserving standalone
file browsing and the existing one-shot host integration model.

In WezTerm, Wisp projects map to workspaces and tmux-style windows map to
WezTerm tabs. The selected project's tabs or files appear beside the project
list instead of behind a project-action screen.

## Approved Decisions

- Keep projects visible in the left pane.
- Give the right pane two modes: WezTerm tabs (`Windows`) and lazy filesystem
  entries (`Files`).
- Make `leader+s` open project-focused Wisp.
- Make `leader+w` open window-focused Wisp for the current project.
- Remove the existing `leader+f` Wisp binding.
- Use `/` to enter search mode so normal-mode `x`, `w`, and `f` remain direct
  commands.
- Make `x` close the selected open project and exit Wisp.
- Keep the picker one-shot. Do not add a daemon, interactive IPC, or an
  in-place close-and-refresh loop.
- Leave the live Neovim configuration unchanged.

## Layout

At normal terminal widths, render Projects on the left and the selected
project's Windows or Files on the right. Projects use less horizontal space
than the detail pane. The focused pane uses the terminal accent color for its
border and the selected row keeps the existing reversed, bold treatment.

Below the minimum useful two-column width, stack Projects above the detail
pane. Both layouts retain the current header, status area, project status
icons, and terminal ANSI palette.

The right pane has explicit empty states:

- `Project is not open` when Windows mode targets a closed project.
- `No windows` or `No matching windows` for an open project with no visible
  host items.
- `No files` or `No matching files` for an empty or filtered directory.

## Interaction

### Normal Mode

| Key | Action |
| --- | --- |
| `Up` / `Down`, `j` / `k` | Move in the focused pane |
| `Left` / `h`, `Right` / `l`, `Tab` | Change pane focus |
| `Enter` | Select a project, window, or file; enter a directory |
| `w` | Show Windows and focus the right pane |
| `f` | Show Files and focus the right pane |
| `x` | Close the focused open project from the project pane |
| `/` | Enter search mode for the focused pane |
| `Backspace` | Go to the parent directory; at the project root focus Projects |
| `Esc`, `q`, `Ctrl-C` | Cancel |
| `Ctrl-R` | Refresh the active project or filesystem listing |

Project and detail queries are independent. Changing the selected project,
right-pane mode, or directory clears the affected detail query and cursor.

### Search Mode

Printable characters, including `x`, `w`, and `f`, update the focused pane's
fuzzy query. `Backspace` edits it. `Esc` returns to normal mode while retaining
the query. `Enter` selects the current match.

## Selection Semantics

- Selecting a project switches to or creates its workspace.
- Selecting a host window activates the exact WezTerm tab captured in the
  host snapshot.
- Selecting a file returns the existing file selection and resolved opener.
- Selecting a directory reads that directory lazily and stays in Files mode.
- Closing a project returns a host action. The WezTerm adapter closes the owned
  picker tab before terminating all remaining panes in the project workspace.

## Protocol V2

Replace the presentation-only host annotations with a versioned host context.
Each project context contains labels and generic host items. Host IDs are
opaque strings to Rust; the WezTerm adapter uses stringified mux tab IDs.

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

The context rejects empty project IDs, labels, item IDs, and item labels. Item
IDs must be unique within a project. `detail` is optional and `active` defaults
to false.

Add a `host_item` selection containing the owning project and opaque item ID.
All result envelopes move to protocol version 2. Adapters reject other
versions rather than guessing at compatibility.

## CLI

Embedded launchers use:

```text
wisp pick --result-file <path> --host-context-file <path> \
  --initial-view projects|windows
```

`projects` starts on the current project with the project pane focused.
`windows` starts on the current project and active host item with the detail
pane focused. If no configured project owns the current workspace, the picker
falls back to project focus and reports that the current workspace is
unmanaged.

Standalone Wisp has no host items. It starts project-focused and still exposes
Files mode for every project.

## WezTerm Adapter

Build the host context from `wezterm.mux.all_windows()`:

1. Match mux windows to configured project workspace names.
2. Enumerate `tabs_with_info()` in index order.
3. Record `tab:tab_id()` as the opaque ID.
4. Prefer an explicit tab title; otherwise use the active pane title or
   foreground process basename.
5. Use the active pane's project-relative cwd as optional detail when
   available.
6. Mark the tab active only when its workspace and tab are active.
7. Exclude the temporary picker tab from project host items.

For a `host_item` result, resolve the tab with `wezterm.mux.get_tab`, verify
that it still belongs to the selected project's workspace, activate it, and
switch the original GUI window to that workspace. A stale or moved tab logs an
error and performs no host action.

Export `window_picker_action()` beside `project_picker_action()`. Both actions
use the same picker lifecycle and differ only in their initial view.

## Responsive And Stale-State Rules

- Windows are a launch-time snapshot. Reopen Wisp to refresh them.
- `Ctrl-R` refreshes projects or the active filesystem directory, not host
  items.
- Host item activation validates both ID existence and workspace ownership.
- Project closure scopes pane termination to the selected workspace and
  excludes the picker pane.
- A narrow terminal changes layout only; focus and key behavior remain the
  same.

## Implementation Order

1. Add protocol v2 fixtures and failing core/CLI contract tests.
2. Implement host context models, `host_item`, and initial-view CLI plumbing.
3. Add failing TUI state, interaction, and responsive rendering tests.
4. Replace the action screen with the two-pane TUI and lazy Files mode.
5. Add failing WezTerm context, activation, stale-ID, and launcher tests.
6. Implement the WezTerm host snapshot and window action.
7. Update Artifacts bindings, README, and architecture documentation.
8. Format and run all Rust, Lua, and WezTerm validation checks.
9. Install the tested binary and cut over the live WezTerm configuration with
   the matching plugin protocol.

## Verification

- Protocol fixtures deserialize and invalid versions/data are rejected.
- Project status ordering and standalone behavior remain intact.
- Pane focus, right modes, separate queries, direct close, and lazy directory
  traversal have state-machine tests.
- Both wide and narrow TestBackend layouts render useful content.
- WezTerm tests cover metadata generation, active tab selection, stale IDs,
  moved tabs, closure scope, and both launcher actions.
- Rustfmt, locked workspace tests, strict Clippy, StyLua, Lua tests, and real
  WezTerm configuration parsing pass.
- Manual verification covers `leader+s`, `leader+w`, tab activation, file
  opening, unmanaged-workspace fallback, and disposable-workspace closure.
