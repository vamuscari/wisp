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
- Keep the picker one-shot. Do not add a daemon, a custom bidirectional host
  protocol, continuous pane polling, or an in-place close-and-refresh loop.
  Window previews may make bounded `wezterm cli get-text` requests when the
  preview target changes or the user explicitly refreshes it.
- Leave the live Neovim configuration unchanged.

## Implemented Follow-Up: Open-Project Git Status

- Show the Git branch and working-tree summary on every configured project row
  labeled `current` or `open`, rather than only on the active project.
- Reuse the existing VCS icons and right-aligned project-row rendering. The
  active project's file context remains visible alongside its Git summary.
- Collect summaries asynchronously with bounded concurrency so repositories do
  not block picker startup or input. Rows update as results arrive.
- Do not run Git for closed projects. A missing Git executable, a non-repository
  project, or a failed status command leaves that row without a summary.
- Refresh Git summaries for the current open-project snapshot on `Ctrl-R`.
- Keep this internal to the CLI and TUI. Existing host-context labels identify
  open projects, so no protocol, configuration, or cache schema change is
  required.

## Implemented Follow-Up: Window Preview

- In Windows mode, show a preview panel for the window row under the mouse. A
  mouse hover only changes the preview; it never activates the window.
- Keep full keyboard parity: when there is no hovered row, preview the currently
  highlighted window as `j`, `k`, or the arrow keys move it.
- Preview the current plain-text viewport of the tab's active pane, not a
  graphical screenshot. Split panes are represented by the active pane only.
- Add an optional active pane ID to project and host-workspace items. Because
  host context is strict, this requires a synchronized protocol bump across the
  Rust models, both Lua adapters, protocol fixtures, adapter tests, and schema
  assertions. Follow the repository versioning policy for the package and the
  config/cache versions coupled to the protocol.
- Have the WezTerm adapter pass the full WezTerm executable path to the picker.
  When the keyboard or mouse preview target changes, asynchronously run
  `wezterm cli get-text --pane-id <id>` and render its stdout. The pinned
  minimum WezTerm version supports this command, and an explicit pane ID keeps
  the request targeted at the original tab rather than the picker pane.
- Treat each read as a current-buffer snapshot. Do not poll while the target is
  unchanged; `Ctrl-R` explicitly refreshes the current preview. Debounce rapid
  target changes, keep input responsive, and discard responses for targets
  that are no longer selected or hovered.
- Make current-buffer reads explicit through `p`. The Preview pane starts hidden
  by default; the WezTerm `window_preview = true` option makes it initially
  visible. Preview text may contain secrets, so never read it while Preview is
  hidden, cache it, log it, or write it to host context or another temporary
  file. Retain only the current response in picker memory and clear it when the
  target changes, Preview is hidden, or the picker exits.
- Bound preview subprocess concurrency, captured bytes, and rendered physical
  lines. A missing WezTerm executable, failed command, invalid pane ID, or
  oversized response produces `Preview unavailable` without blocking input or
  changing selection behavior.
- Render Projects, Windows, and the requested Preview or Commands pane as three
  regions at wide widths. In the stacked layout, place the auxiliary pane below
  Windows; replace the detail pane when there is not enough room for both.
- Preserve newlines, strip unsupported control characters, truncate each line
  to the preview width, and show explicit `Preview unavailable` and `No output`
  states without changing selection behavior.
- Extend TUI input from key-only events to keyboard and mouse events, enable
  mouse capture only while Preview is visible in the alternate screen, and
  disable it when Preview is hidden and on every normal and error exit path.
- Test adapter pane IDs and executable-path wiring, strict protocol decoding,
  keyboard and mouse target changes, explicit refresh, output limits, stale
  response suppression, command failures, empty previews, wide and stacked
  layouts, and terminal mouse-capture cleanup. Keep minimum-WezTerm parsing in
  the verification matrix.

## Implemented Follow-Up: Status Providers And Popup Surface

- Keep status composition in bundled providers. `opencode` renders aggregate
  session state and `directory` renders the short workspace name; external
  `status_items` controls only inclusion, order, and an optional `projects`,
  `windows`, or `sessions` picker action.
- Do not load arbitrary provider paths or execute status-configured commands.
  Status actions launch the same typed Wisp picker flow as public adapter
  actions.
- Encode clickable cells with `Hyperlink` and `EndHyperlink` format items and
  dispatch the resulting `wisp://status/*` URI through WezTerm's cancellable
  `open-uri` event. Probe support at adapter startup and retain non-clickable
  rendering on WezTerm builds without the status-hyperlink patch.
- Use one reusable top-level split per GUI window for popup pickers. Preserve
  exact argv and named-domain spawning, make direction and size strict options,
  replace an existing Wisp popup before opening another, and make every cleanup
  path idempotent.
- Export `popup_action()` for consumer-owned mappings. Existing project,
  window, and OpenCode actions retain their temporary-tab behavior.

## Implemented Follow-Up: Hierarchical Windows And Files

- Protocol v6 replaces flat host items with Window entries containing exact
  Pane entries. Project-backed and host-only workspaces both navigate through
  Project → Window → Pane and return the captured window and pane IDs.
- `single_pane_behavior = "show" | "activate"` controls whether Enter exposes a
  one-pane Window or activates it immediately. The default is `"show"`.
- Files use adaptive retained Miller columns. Highlighting a directory loads
  only its immediate children; Enter or Right focuses that child, while Left or
  Backspace returns to a retained ancestor. Reads run on a debounced worker;
  request IDs prevent superseded results from replacing the newest column.
  Logical depth is unlimited.
- Wide layouts show Projects plus three directory levels, medium layouts show
  Projects plus two, and narrow layouts stack Projects with one recycled file
  column. The utility bar retains the full focused breadcrumb.
- `o` remains the direct project/workspace activation path; Enter drills into
  the hierarchy before selecting an exact pane or file.

## Layout

At normal terminal widths, render Projects on the left and the selected
project's Windows or Files on the right. Projects use less horizontal space
than the detail pane. The focused pane uses the terminal accent color for its
border and the selected row keeps the existing reversed, bold treatment.

Below the minimum useful two-column width, stack Projects above the detail
pane. Remove the application title bar and use one white-bordered bottom utility
bar for mode/context, search input, and status errors. Command guidance appears
only in the `?` Commands pane. Both layouts retain project status icons and the
terminal ANSI palette.

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
| `s` | Show OpenCode Sessions and focus the right pane |
| `x` | Close the focused open project from the project pane |
| `p` | Toggle the window Preview pane |
| `?` | Toggle the Commands pane |
| `/` | Enter search mode for the focused pane |
| `Backspace` | Go to the parent directory; at the project root focus Projects |
| `Esc` | Close Commands when visible; otherwise cancel |
| `q`, `Ctrl-C` | Cancel |
| `Ctrl-R` | Refresh the active project or filesystem listing |

Project and detail queries are independent. Changing the selected project,
right-pane mode, or directory clears the affected detail query and cursor.

### Search Mode

Printable characters, including `x`, `w`, `f`, `p`, and `?`, update the focused pane's
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
  moved tabs, closure scope, tab and popup launchers, ordered status providers,
  click dispatch, and compatibility fallback.
- Rustfmt, locked workspace tests, strict Clippy, StyLua, Lua tests, and real
  WezTerm configuration parsing pass.
- Manual verification covers `leader+s`, `leader+w`, tab activation, file
  opening, unmanaged-workspace fallback, and disposable-workspace closure.
