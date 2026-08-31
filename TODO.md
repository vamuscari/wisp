# TODO

Track planned Wisp features and fixes here. Keep each item scoped and include
its expected behavior and verification strategy.

## Features

### OpenCode Tab Status Colors

- [ ] Add opt-in OpenCode status colors to WezTerm tab backgrounds.

#### Goal

Color each WezTerm tab according to the most urgent OpenCode state reported by
any pane in that tab. Tabs without a fresh OpenCode state must retain WezTerm's
normal tab formatting.

#### Behavior

- Add a strict `opencode_tab_colors` WezTerm option that defaults to `false`.
- When enabled, color every tracked OpenCode state using the existing semantic
  `status_colors` entries:
  - `waiting` uses `waiting_background`.
  - `failure` uses `failure_background` and represents retrying or error states.
  - `running` uses `running_background`.
  - `idle` uses `idle_background`.
- Resolve tabs containing multiple OpenCode panes with this priority:
  `waiting > failure > running > idle`.
- Inspect every pane in a tab, not only its active pane.
- Keep the active tab distinguishable with bold title text.
- Preserve explicit tab titles, fall back to the active pane title, and truncate
  the result to the `format-tab-title` width.
- Return no custom formatting for an unaffected tab so WezTerm retains its
  configured active, inactive, and hover styles.
- Use steady colors. Do not copy the right-status waiting or failure flash
  animation into tab backgrounds.

#### Data Flow

- Use WezTerm pane user variables rather than extending
  `wisp opencode status --json`.
- Have `opencode/wisp.js` publish an internal `WISP_OPENCODE_STATUS` user
  variable through OSC 1337 when `WEZTERM_PANE` identifies a WezTerm launch.
- Encode a small `state:updated_at_seconds` payload, base64 encoded as required
  by WezTerm's user-variable protocol.
- Derive the state from the plugin's existing event-backed data:
  - Pending permissions or questions produce `waiting`.
  - Retry activity or a retained session error produces `failure`.
  - Busy activity produces `running`.
  - An idle session or a launch without a selected session produces `idle`.
- Refresh the timestamp during the existing 30-second plugin heartbeat, even
  when the semantic state has not changed.
- Clear the user variable during normal plugin disposal.
- Treat malformed values, future timestamps, and values older than 90 seconds
  as absent so an abnormal OpenCode exit cannot color a tab indefinitely.
- Keep the existing registry-backed aggregate counts and right status behavior
  unchanged.

#### WezTerm Adapter

- [ ] Add `opencode_tab_colors` to the strict option allowlist, validation,
  defaults, public option table, and option tests in `wezterm/options.lua`.
- [ ] Add synchronous tab-state parsing and formatting to `wezterm/status.lua`.
- [ ] Register `format-tab-title` from `wezterm/init.lua` only when the option is
  enabled.
- [ ] Keep the formatter entirely in memory: no child processes, filesystem
  reads, server requests, or yielding calls are allowed in the synchronous
  event.
- [ ] Compare pane IDs only through each `TabInformation.panes` snapshot and its
  pane-local user variables; do not infer OpenCode state from pane titles,
  process names, or working directories.
- [ ] Log malformed callback failures without replacing the default tab title.
- [ ] Document that WezTerm executes only the first `format-tab-title` handler,
  so enabling this option gives Wisp ownership of that handler.

#### OpenCode Plugin

- [ ] Add one status-derivation function so registry registration and pane-user
  variable publication use the same event-backed state.
- [ ] Publish the initial idle state after the supported OpenCode version check.
- [ ] Publish updates after activity, permission, question, error, session, and
  heartbeat reconciliation changes.
- [ ] Clear the user variable before unregistering during disposal.
- [ ] Keep publishing failures isolated from registration so terminal signaling
  cannot break OpenCode status tracking.
- [ ] Do not emit WezTerm control sequences when the process has no valid
  `WEZTERM_PANE` environment value.

#### Tests

- [ ] Extend `tests/opencode_plugin_test.mjs` to capture and decode OSC user-var
  writes for idle, running, waiting, failure, heartbeat renewal, and disposal.
- [ ] Extend `tests/opencode_plugin_process_test.mjs` to verify a real bundled
  plugin process publishes for pane `42` and clears its value on disposal.
- [ ] Add `tests/wezterm_tab_status_test.lua` covering:
  - Default-disabled and explicit opt-in behavior.
  - Strict boolean option validation.
  - Status in an inactive pane coloring its containing tab.
  - `waiting > failure > running > idle` aggregation.
  - Idle tab coloring.
  - Malformed, future, stale, missing, and cleared values.
  - Numeric pane IDs and multiple tabs.
  - Explicit-title and active-pane-title fallback behavior.
  - Width truncation, custom semantic colors, and active-tab emphasis.
  - Returning default formatting for tabs without OpenCode.
- [ ] Add the new Lua test file to the manually maintained `tests/run.lua` list.
- [ ] Extend `tests/options_test.lua`, `tests/wezterm_config.lua`, and test helpers
  for the option and synchronous formatter APIs.
- [ ] Update `~/Artifacts/wezterm/wezterm_test.lua` to assert that the managed
  consumer enables the option and owns one `format-tab-title` handler.

#### Documentation And Versioning

- [ ] Document `opencode_tab_colors`, its default, state priority, semantic
  colors, freshness behavior, and event ownership in `README.md`.
- [ ] Enable `opencode_tab_colors = true` in
  `~/Artifacts/wezterm/wezterm.lua` after adapter tests pass.
- [ ] Bump the workspace package from `0.9.0` to `0.10.0` because this is a new
  public option and substantial bundled capability.
- [ ] Synchronize `Cargo.toml`, Wisp workspace entries in `Cargo.lock`, and exact
  package-version assertions.
- [ ] Keep protocol, config, cache, registry, and deployment schemas at version
  6. The plugin and adapter ship in the same content-addressed bundle, so this
  internal user-variable contract does not require a protocol change.
- [ ] Do not modify native WezTerm source; documented pane user variables and
  `format-tab-title` already provide the required host APIs.

#### Verification And Deployment

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `rustup run 1.85.0 cargo check --workspace --locked`.
- [ ] Run `node --check opencode/wisp.js`.
- [ ] Run the OpenCode plugin Node test suites.
- [ ] Run `lua tests/run.lua` and `stylua --check .`.
- [ ] Run the focused managed WezTerm configuration test in `~/Artifacts`.
- [ ] Install Wisp `0.10.0`, confirm `wisp --version`, and deploy the new bundle
  without `--replace-incompatible` because deployment schema v6 is unchanged.
- [ ] Refresh the stable OpenCode loader with `wisp opencode install` and restart
  OpenCode so the new bundled plugin publishes pane state.
- [ ] Dry-run and push only the managed WezTerm configuration through a reduced
  `~/Artifacts` manifest.
- [ ] Run `wisp deploy verify`, validate the live Wisp config, and confirm
  `wisp projects --json` still returns protocol v6.
- [ ] Manually verify split-pane tab coloring, priority changes, pane moves,
  normal OpenCode exit, and unaffected tabs without automating the host GUI.

#### Acceptance Criteria

- A tab containing any fresh OpenCode pane uses the color for its highest
  priority state within one plugin event or heartbeat update.
- A status change in a non-active pane updates that tab's color.
- Moving an OpenCode pane causes its new containing tab to inherit the color and
  its old tab to return to its remaining state or default theme.
- Normal OpenCode shutdown clears the color; stale state is ignored after 90
  seconds following an abnormal shutdown.
- Tabs without OpenCode remain visually identical to the existing theme.
- The aggregate right status, picker session behavior, and strict protocol v6
  contracts remain unchanged.

### Live Neovim File Preview And Editor Reuse

- [ ] Extend Files mode with a live Neovim preview, configurable Window or Pane
  opening, and exact reuse of files already visible in Neovim.

#### Goal

Make Files mode behave as a project-aware editor launcher without creating
duplicate Neovim instances unnecessarily. The highlighted file should render
in a live Neovim preview. `Enter` should jump to an existing visible editor
target when possible, while explicit hotkeys can force a new Window or Pane.

In Wisp terminology, a project maps to a WezTerm workspace, a Window maps to a
WezTerm tab, and a Pane maps to a WezTerm split. In the Neovim adapter, Window
maps to a tab page while right and bottom Panes map to vertical and horizontal
splits in the originating tab.

#### Interaction

- `Enter` opens the selected file using the configured default target, which
  defaults to `window`.
- Before opening, `Enter` must look for the same file in a visible Neovim
  window. A valid match activates the exact project workspace, WezTerm Window,
  and WezTerm Pane instead of launching another editor.
- `Ctrl-T` must always force a new Window, even when the file is already
  visible.
- `Ctrl-V` must always force a new right Pane.
- `Ctrl-X` must always force a new bottom Pane.
- The three Ctrl hotkeys must work on the selected file in both normal and
  search mode. Printable search input must retain its current behavior.
- `Enter` on a directory must continue descending lazily. The explicit file
  target hotkeys must not open directories and should report a concise status
  when no file is selected.
- `p` in Files mode must toggle the live file preview. `p` in Windows mode must
  retain the existing terminal-text preview behavior.
- When preview support is configured, it must start visible when Files mode is
  entered, close when Files mode is left, and reopen when Files mode is
  revisited unless the user toggled it off.
- If Pane opening is requested for a project with no live workspace or Window,
  create the project workspace with the file opener as its first Window because
  no Pane exists to split.

#### Host Configuration

- Keep file placement and preview layout in each host adapter. Do not put host
  Window, Pane, direction, or size policy in shared Wisp TOML.
- Keep `openers.file` in shared Wisp TOML as the argv executed by `wisp open`.
  Documentation should use `file = ["nvim", "{path}"]` for this feature but
  must not hard-code or inspect the opener program name.
- Add strict WezTerm options with this shape:

```lua
wisp.apply_to_config(config, {
  file_open = { default = "window" },
  file_preview = {
    command = { "nvim" },
    direction = "Right",
    size = 0.5,
  },
})
```

- Add strict Neovim options with this shape:

```lua
require("wisp").setup({
  file_open = { default = "window" },
  file_preview = { width = 0.5 },
})
```

- Accept only `window`, `right_pane`, and `bottom_pane` as
  `file_open.default` values.
- Require the WezTerm preview command to be a dense, non-empty argv array.
  Launch it directly without a shell and permit an absolute Neovim executable
  plus user-owned startup flags.
- Omitted `file_preview` must disable live file preview without affecting file
  selection or opening.
- Validate preview direction and size with the same strict direction and
  positive-size rules used by existing WezTerm split options.

#### Neovim Pane State

- [ ] Replace the plain `WISP_NVIM_FILE` pane variable with a strict,
  versioned `WISP_NVIM_STATE` JSON payload encoded through OSC 1337.
- [ ] Publish every normal file shown by a Neovim window in the currently
  displayed Neovim tab page. Do not publish hidden buffers, unnamed buffers,
  terminal buffers, quickfix windows, help, or other non-file buffers.
- [ ] Include the exact absolute path, opaque Neovim window ID, active-window
  flag, source window width and height, visible bottom line, and these
  `winsaveview()` fields for every published view:
  `lnum`, `col`, `coladd`, `curswant`, `topline`, `topfill`, `leftcol`, and
  `skipcol`.
- [ ] Require the payload's exact protocol version and exact fields before any
  adapter uses it. Reject empty paths, invalid IDs, negative view values,
  invalid dimensions, unknown fields, and malformed JSON.
- [ ] Publish immediately on setup and after `BufEnter`, `BufFilePost`,
  `BufWinEnter`, `TabEnter`, `WinEnter`, `WinNew`, `WinClosed`, and UI attach.
- [ ] Refresh viewport state after `CursorMoved`, `WinScrolled`, and resize
  events through one short debounce so ordinary scrolling does not emit an OSC
  sequence for every intermediate movement.
- [ ] Clear the pane variable immediately on `VimLeavePre` and whenever no
  qualifying file view remains.
- [ ] When foreground-process inspection is available, ignore a stale pane
  variable unless the pane is still running Neovim. Preserve the existing rule
  that mux panes may use pane state when process inspection is unavailable.
- [ ] Do not retain a reader for `WISP_NVIM_FILE`; the protocol bump replaces
  the old pane-state contract outright.

The decoded pane value should have this conceptual shape:

```json
{
  "protocol_version": 7,
  "views": [
    {
      "window_id": "1001",
      "path": "/home/user/Repos/api/src/main.rs",
      "active": true,
      "width": 120,
      "height": 40,
      "bottomline": 157,
      "view": {
        "lnum": 132,
        "col": 8,
        "coladd": 0,
        "curswant": 8,
        "topline": 118,
        "topfill": 0,
        "leftcol": 0,
        "skipcol": 0
      }
    }
  ]
}
```

#### Host Context And Matching

- [ ] Extend protocol `HostPane` with optional strict Neovim view metadata.
  Keep WezTerm Window and Pane IDs opaque strings.
- [ ] Have `wezterm/picker.lua` decode `WISP_NVIM_STATE` for every captured
  Pane and attach valid views to that Pane's host-context entry.
- [ ] Continue passing the originating active file to Wisp, but derive it from
  the active published Neovim view rather than a separate pane variable.
- [ ] Match a selected file to editor views with Wisp's normalized path identity
  so Windows drive paths, UNC paths, separator differences, and case rules
  behave consistently with project discovery.
- [ ] Consider only views owned by the selected project's exact workspace.
  Host-only workspaces and another project's panes must never satisfy a match.
- [ ] If several panes show the same file, rank the originating active Pane
  first, then an active Window and Pane, then an active internal Neovim view,
  and finally stable host snapshot order.
- [ ] Show a concise marker or detail on file rows that already have a live
  editor target so users can predict that `Enter` will jump rather than open.

#### Preview State Protocol

- [ ] Add an adapter-created temporary preview-state path passed to
  `wisp pick`. Standalone Wisp without that path must keep preview unavailable.
- [ ] Define a strict protocol-v7 preview envelope with a monotonically
  increasing sequence and exactly one of these states:
  - `hidden`: preview is disabled or Files mode is not active.
  - `empty`: preview is visible but the highlighted entry is not a file.
  - `file`: includes the selected project, absolute path, and optional matched
    Neovim view state.
- [ ] Atomically replace the sidecar in its own directory. Readers must never
  interpret a partial write, mismatched version, duplicate field, unknown
  field, or invalid state/field combination.
- [ ] Publish only when the effective preview state changes and debounce rapid
  file highlights so the preview renders the newest path rather than every
  intermediate row.
- [ ] Remove the sidecar after selection, cancellation, picker failure,
  timeout, popup replacement, and normal adapter cleanup.

#### Neovim Preview Renderer

- [ ] Add one canonical bundled Neovim preview module used by both host
  integrations.
- [ ] Render into an isolated scratch buffer rather than loading highlighted
  paths into the user's normal buffer list.
- [ ] Read at most 1 MiB or 10,000 lines, whichever limit is reached first.
  Render a clear truncation message and reject binary content containing NUL.
- [ ] Report unreadable, deleted, directory, binary, and oversized inputs in
  the preview without failing or closing the picker.
- [ ] Detect filetype from the path and let Neovim apply its normal syntax and
  active color scheme. Do not use `--clean` by default; users may add it to the
  configured WezTerm preview argv when they prefer isolation over their theme.
- [ ] Keep the preview buffer non-modifiable, read-only, unswapped, and excluded
  from ShaDa and ordinary buffer history.
- [ ] When matched live view state exists, apply its cursor, top line,
  horizontal scroll, fill, and skip offsets with `winrestview()` after loading
  the scratch content.
- [ ] Display the source viewport line range and source dimensions so a preview
  with different dimensions still communicates the original scroll height and
  location.
- [ ] Prefer the active internal Neovim view when one pane publishes the same
  file in multiple visible windows.
- [ ] Fall back to the beginning of the file when no matching live view exists
  or the captured view lies beyond the bounded preview content.

#### TUI And Selection Contract

- [ ] Add a strict `FileOpenTarget` enum with serialized values `window`,
  `right_pane`, and `bottom_pane`.
- [ ] Extend `Selection::File` with required `open_target` and
  `reuse_existing` fields plus an optional nested host target containing exact
  WezTerm `window_id` and `pane_id`.
- [ ] `Enter` must set `reuse_existing = true`, carry the configured default
  target for stale-target fallback, and include the best matching host target
  when one exists.
- [ ] `Ctrl-T`, `Ctrl-V`, and `Ctrl-X` must set `reuse_existing = false`, set
  their explicit target, and omit a host target so they always create another
  editor.
- [ ] Keep `wisp open` placement-agnostic. It must validate the complete v7
  selection, ignore host placement fields, and execute only the resolved opener
  argv directly.
- [ ] Extend the Commands pane and README key table with the default and forced
  file-open actions without obscuring the existing project, Window, Pane,
  search, and preview commands.

#### WezTerm Application

- [ ] Before applying a reusable file selection, resolve the captured Pane ID
  and verify that it still belongs to the captured Window and selected project
  workspace.
- [ ] Read and strictly validate the Pane's fresh `WISP_NVIM_STATE`, normalize
  its paths, and confirm the selected file is still visible before activation.
- [ ] For a valid target, reuse `Workspace:activate_host_pane` to activate the
  exact project workspace, Window, and Pane. Do not run `wisp open`, spawn a
  tab, split a Pane, or try to focus an internal Neovim window.
- [ ] If the Pane disappeared, moved, stopped running Neovim, published invalid
  state, or changed files, fall back to the selection's configured
  `open_target`. Report an error only if that fallback also fails.
- [ ] For `window`, retain the existing behavior: create the project workspace
  with `wisp open` as its initial process or spawn a new tab in an existing
  project workspace.
- [ ] For `right_pane` and `bottom_pane`, split the active Pane of the selected
  project's active Window, run `wisp open` as the new Pane's initial process,
  activate the new Pane, and switch to the project workspace.
- [ ] If the target workspace has no mux Window, create its first Window for all
  three targets rather than splitting the originating workspace or rejecting
  the selection.
- [ ] Keep cwd, configured domain, `WISP_PROJECT_DIR`, and
  `WISP_PROJECT_NAME` on every new Window or Pane command.

#### WezTerm Preview Lifecycle

- [ ] Extend the picker result poller to read strict preview-state updates and
  create the configured Neovim preview split only while state is visible.
- [ ] Split the owned picker Pane, not an arbitrary project Pane. Keep preview
  execution in the same host-neutral local domain as the picker.
- [ ] Pass the sidecar path, protocol version, and canonical bundled preview
  module path without constructing a shell command.
- [ ] Track the preview Pane as part of picker ownership. Temporary-tab,
  top-level-popup, cancellation, timeout, process-exit, result-error, and popup
  replacement paths must close every owned preview Pane.
- [ ] If preview Neovim exits unexpectedly, report one actionable toast and do
  not respawn continuously until preview is toggled or Files mode is re-entered.
- [ ] Keep one preview per picker. Rapid state changes must update the existing
  Neovim process rather than create additional Panes.

#### Neovim Adapter

- [ ] Pass the file-open default and preview sidecar path to the picker through
  explicit CLI arguments.
- [ ] When preview becomes visible, resize and move the picker float within its
  configured footprint and create a companion preview float to its right.
  Closing preview must restore the picker to its original dimensions.
- [ ] Use the canonical scratch renderer for sidecar updates and keep focus in
  the picker terminal while preview follows the highlighted file.
- [ ] Close the preview float, stop timers/watchers, and remove temporary state
  before applying a selection or reporting picker failure.
- [ ] For `reuse_existing = true`, scan visible normal-file Neovim windows for
  the selected normalized path and focus the best live match before opening a
  new target.
- [ ] Map `window` to a new Neovim tab page, `right_pane` to a vertical split,
  and `bottom_pane` to a horizontal split. Pane targets must operate in the
  originating tab, then set its tab-local project cwd and Wisp metadata.
- [ ] Use structured `nvim_cmd` arguments for paths. Do not interpolate file or
  project paths into Ex command strings.
- [ ] Preserve the existing error when the originating tab no longer exists,
  unless a valid existing file Window can be focused without that tab.

#### Protocol And Deployment

- [ ] Bump strict protocol 6 to 7 because `Selection::File`, `HostPane`, the
  pane user variable, and the preview sidecar all change their contracts.
- [ ] Update `PROTOCOL_VERSION`, both Lua adapter versions and deployment
  tokens, every strict validator, protocol fixture, adapter test, and canonical
  JSON example together.
- [ ] Rename all canonical `tests/fixtures/*-v6.json` files to v7, add the
  required file-open fields, and add valid hidden, empty, and matched-view
  preview fixtures.
- [ ] Accept no v6 selection, host context, pane state, preview state, cache,
  registry, config, or deployment state after the bump. Do not add fallback
  readers or migrations.
- [ ] Account for current version coupling: config, cache, OpenCode registry,
  and deployment schema constants follow the protocol and therefore also move
  to 7. Old cache and registry state must be discarded and rebuilt; v6 shared
  TOML must be rejected clearly.
- [ ] Add the canonical preview module to the content-addressed deployment
  assets, manifest verification, stable loader arguments, bundle hash tests,
  and exact-file assertions.
- [ ] Treat this as a substantial capability and breaking protocol change. If
  it lands before the planned OpenCode tab-color feature, use package `0.10.0`
  here and move that later feature to `0.11.0`. If they ship together, update
  the tab-color plan to protocol/schema 7 and let package `0.10.0` cover both.
  If tab colors ship first as `0.10.0`, use `0.11.0` here.
- [ ] Synchronize `Cargo.toml`, the three Wisp workspace entries in
  `Cargo.lock`, exact package assertions, and release/deployment expectations
  for the selected package version.

#### Tests

- [ ] Extend `crates/wisp-core/tests/navigation_protocol_test.rs` for every
  `FileOpenTarget`, required reuse fields, optional host target, strict unknown
  fields, missing fields, invalid targets, and v6 rejection.
- [ ] Extend TUI tests for Enter defaults, `Ctrl-T`, `Ctrl-V`, `Ctrl-X`, normal
  and search modes, directory no-ops, matching-path ranking, open-file markers,
  preview toggling, and Commands guidance.
- [ ] Cover normalized Unix, Windows drive, and UNC matches, including case and
  separator differences and files with spaces.
- [ ] Add CLI tests for atomic preview sidecar replacement, sequence ordering,
  debounce-to-latest behavior, all strict states, cleanup, and `wisp open`
  ignoring placement while preserving opener argv.
- [ ] Extend `tests/nvim_adapter_test.lua` for multi-window state publication,
  exact `winsaveview()` fields, viewport debounce, clear-on-exit, companion
  preview layout, scratch limits, syntax detection, view restoration, existing
  Window focus, and all three open targets.
- [ ] Extend `tests/process_adapter_test.lua` for pane-state decoding, host
  context propagation, multiple matching panes, exact reuse without an opener
  process, strict live revalidation, stale fallback, forced duplicates, and
  closed-project first-Window behavior.
- [ ] Add focused WezTerm preview lifecycle tests for temporary tabs, popups,
  toggle-off, cancellation, timeout, malformed state, preview-process exit,
  replacement, and orphan prevention. Add any new Lua test file to
  `tests/run.lua` explicitly.
- [ ] Extend options tests for strict nested option fields, dense preview argv,
  valid defaults, directions, sizes, widths, unknown fields, and disabled
  defaults in both adapters.
- [ ] Extend deployment tests to hash, install, verify, and load the canonical
  preview module from the active bundle.
- [ ] Keep the real CI smoke tests passing on minimum WezTerm and Neovim 0.10.4
  so no newer host API becomes an accidental requirement.

#### Verification And Deployment

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `cargo +1.85.0 check --workspace --locked`.
- [ ] Run `node --check opencode/wisp.js` and both OpenCode plugin test suites
  because the protocol-coupled registry version changes.
- [ ] Run `lua tests/run.lua` and `stylua --check .`.
- [ ] Parse the minimum WezTerm test configuration and load the deployed adapter
  in Neovim 0.10.4 through the existing CI smoke tests.
- [ ] Update `~/Artifacts/wisp/config.toml` to config version 7, then use the
  reduced `~/Artifacts` manifest to dry-run and push only the managed Wisp TOML
  to `$HOME/.config/wisp/config.toml`.
- [ ] Install the selected package version, confirm `wisp --version`, and run
  `wisp deploy --replace-incompatible` because deployment schema 7 intentionally
  replaces schema 6.
- [ ] Run `wisp deploy verify`.
- [ ] Run both consumer checks with
  `WISP_CONFIG_FILE="$HOME/.config/wisp/config.toml"`: `wisp config validate`
  and `wisp projects --json`.
- [ ] Refresh the OpenCode loader if the protocol-coupled registry or bundled
  plugin changed, then restart WezTerm, Neovim, and OpenCode as required.
- [ ] Manually verify live syntax preview and viewport restoration from multiple
  Neovim splits, exact Enter reuse across workspace/Window/Pane, forced
  Ctrl-key duplicates, stale fallback, closed-project creation, popup cleanup,
  and preview toggle behavior without screenshots or host GUI automation.

#### Acceptance Criteria

- Files mode shows a live, syntax-highlighted Neovim preview when configured and
  follows the highlighted file without blocking input or leaking Panes.
- A file already visible in Neovim previews at its captured cursor and viewport,
  including source scroll range and dimensions.
- `Enter` activates the exact live project workspace, WezTerm Window, and Pane
  without running an opener when the file is still visible there.
- A stale reuse target safely falls back to the configured default open target.
- `Ctrl-T`, `Ctrl-V`, and `Ctrl-X` always create the requested new target even
  when another Neovim Pane already shows the file.
- Closed projects receive one first Window regardless of the requested target;
  Wisp never opens a project file in an unrelated source workspace.
- The Neovim adapter reuses a visible matching window or applies the requested
  tab/split in the originating tab with correct project metadata.
- Preview buffers are bounded, read-only, unswapped, path-safe, and never use a
  shell or pollute the user's normal buffer list.
- Every external and internal reader rejects non-v7 or malformed state exactly,
  and all v6 state is rebuilt or rejected according to its ownership.

<!--
- [ ] Feature name
  - Goal:
  - Notes:
  - Verification:
-->

## Fixes

### Open Unopened Projects Directly

- [ ] Open an unopened project with no host windows instead of focusing an
  empty Windows pane.

#### Problem

`Enter` on a project currently drills from Projects into Windows even when the
project is not host-open and has no windows to select. The picker then shows
`Project is not open` and requires the user to discover the existing `o`
shortcut. That shortcut already returns a direct project or host-workspace
selection, but it is missing from the on-screen Commands pane.

#### Expected Behavior

- In Windows mode, pressing `Enter` from Projects must directly select a
  configured project when it is not host-open and its unfiltered host window
  list is empty.
- The resulting project selection must use the existing WezTerm adapter path:
  `Workspace:switch_to_project` calls `SwitchToWorkspace` with the configured
  project workspace, domain, project directory, and Wisp environment.
- When that workspace has no mux windows, WezTerm must create one default-shell
  Window in Wisp's model, represented by one WezTerm tab and pane. The spawn
  command must have no `args`; direct project activation must not execute
  `openers.project`.
- Pressing `Enter` on an open project with windows must continue drilling into
  Window and Pane selection so exact host targets remain available.
- Pressing `Enter` in Files or Sessions mode must retain its current behavior.
- Host-only workspaces must retain exact existing-workspace activation and must
  never be recreated from a stale selection.
- Keep normal-mode `o` as the direct project/workspace jump key. It must bypass
  Window and Pane selection for both new and existing projects.
- In search mode, printable `o` must remain query input; `Enter` on the matched
  unopened project must use the same direct-open condition as normal mode.

#### TUI Implementation

- [ ] Update `App::drill_project` in `crates/wisp-tui/src/lib.rs` to return the
  existing `Selection::Project` before changing focus when all direct-open
  conditions are true: Windows mode, configured project target, not host-open,
  and an empty raw `HostContext::windows` list.
- [ ] Check the raw host window list rather than filtered visible rows so an
  active detail query cannot turn `No matching windows` into a project-open
  action.
- [ ] Reuse `App::select_project`; do not add a command, selection variant,
  protocol field, or adapter-specific branch to the host-neutral TUI.
- [ ] Keep the existing normal-mode `o` handler and add a concise `o Jump
  Project` hint to the Commands pane without increasing its required height.

#### WezTerm Adapter

- [ ] Keep `Picker:apply_result` routing `Selection::Project` through
  `Workspace:switch_to_project`.
- [ ] Keep `Workspace:switch_to_project` based on `SwitchToWorkspace` and
  `spawn_command(project)`. Do not pre-create a tab, run `wisp open`, or execute
  opener argv for direct project activation.
- [ ] Preserve `cwd`, `domain`, `WISP_PROJECT_DIR`, and `WISP_PROJECT_NAME` in
  the default-shell spawn.
- [ ] Do not add a default global WezTerm mapping. Host bindings remain
  consumer-owned; `o` is an in-picker command.
- [ ] Do not modify native WezTerm source. Its documented `SwitchToWorkspace`
  behavior already creates a window when the target workspace has none.

#### Tests

- [ ] Add a focused `crates/wisp-tui/tests/two_pane_test.rs` case proving that
  `Enter` on a configured, not-host-open project with zero raw windows finishes
  with that project's `Selection::Project`.
- [ ] Cover the same direct-open result after project search and verify that
  `o` remains search text while search mode is active.
- [ ] Preserve or add coverage proving that `Enter` still drills when a raw host
  window exists and that exact pane selection is unchanged.
- [ ] Preserve Files and Sessions mode Enter coverage.
- [ ] Extend the Commands renderer test to require the `o` project-jump hint.
- [ ] Extend `tests/process_adapter_test.lua` or
  `tests/workspace_action_test.lua` to assert that a direct project action has
  the expected workspace, cwd, domain, and Wisp environment, with no `args` so
  WezTerm launches its default program.

#### Documentation And Versioning

- [ ] Update the `README.md` picker key table so `Enter` documents the
  unopened-project exception and `o` explicitly says it jumps directly without
  selecting a Window or Pane.
- [ ] Document that a direct project selection switches to an existing project
  workspace or creates one default-shell Window at the project directory.
- [ ] Keep protocol, config, cache, registry, and deployment schemas at version
  6; this reuses the existing project selection and adapter contract.
- [ ] Ship the runtime change under the next undeployed package version. If it
  lands before the OpenCode tab-color feature, bump `0.9.0` to `0.9.1` and
  update that feature's version baseline; if they ship together, `0.10.0`
  covers both changes.
- [ ] Synchronize `Cargo.toml`, the three Wisp workspace entries in
  `Cargo.lock`, and the exact package-version assertion in
  `crates/wisp-cli/tests/cli_test.rs` for the selected release version.

#### Verification

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `cargo +1.85.0 check --workspace --locked`.
- [ ] Run `lua tests/run.lua` and `stylua --check .`.
- [ ] Manually open the project picker in WezTerm, highlight a `○` project, and
  verify that `Enter` closes the picker and creates one default-shell project
  Window at the configured directory.
- [ ] Manually verify that `o` jumps to both new and existing projects without
  selecting a Window or Pane, while `Enter` still drills into an open project's
  existing windows.
- [ ] Before a live deployment, install the selected package version, confirm
  `wisp --version`, run `wisp deploy`, then run `wisp deploy verify`, live config
  validation, and `wisp projects --json` against the consumer config.

#### Acceptance Criteria

- `Enter` on a not-host-open project with no windows creates and enters its
  workspace with one default-shell Window.
- `o` directly jumps into a new or existing project without Window or Pane
  selection and is visible in the Commands pane.
- Existing open-project Window and Pane navigation remains exact and unchanged.
- File, session, host-only workspace, opener, and strict protocol v6 behavior
  remain unchanged.

<!--
- [ ] Fix name
  - Problem:
  - Expected behavior:
  - Verification:
-->
