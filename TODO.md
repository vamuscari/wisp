# TODO

Track planned Wisp features and fixes here. Keep each item scoped and include
its expected behavior and verification strategy.

## Features

### OpenCode Tab Status Colors

- [x] Add opt-in OpenCode status colors to WezTerm tab backgrounds.

#### Goal

Color each WezTerm tab according to the most urgent OpenCode state reported by
any pane in that tab. Tabs without a fresh OpenCode state must retain WezTerm's
normal tab formatting.

#### Behavior

- Add a strict `opencode_tab_colors` WezTerm option that defaults to `false`.
- Require `status_bar = true` when tab colors are enabled. A changing zero-width
  status format attribute must trigger tab-title recomputation without changing
  visible right-status content, because WezTerm has no direct tab-bar
  invalidation API.
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

- [x] Add `opencode_tab_colors` to the strict option allowlist, validation,
  defaults, public option table, and option tests in `wezterm/options.lua`.
- [x] Add synchronous tab-state parsing and formatting to `wezterm/status.lua`.
- [x] Register `format-tab-title` from `wezterm/init.lua` only when the option is
  enabled.
- [x] Keep the formatter entirely in memory: no child processes, filesystem
  reads, server requests, or yielding calls are allowed in the synchronous
  event.
- [x] Compare pane IDs only through each `TabInformation.panes` snapshot and its
  pane-local user variables; do not infer OpenCode state from pane titles,
  process names, or working directories.
- [x] Keep the invisible refresh marker independent per WezTerm window so
  interleaved status callbacks continue invalidating every tab bar.
- [x] Log malformed callback failures without replacing the default tab title.
- [x] Document that WezTerm executes only the first `format-tab-title` handler,
  so enabling this option gives Wisp ownership of that handler.

#### OpenCode Plugin

- [x] Add one status-derivation function so registry registration and pane-user
  variable publication use the same event-backed state.
- [x] Publish the initial idle state after the supported OpenCode version check.
- [x] Publish updates after activity, permission, question, error, session, and
  heartbeat reconciliation changes.
- [x] Clear the user variable before unregistering during disposal.
- [x] Keep publishing failures isolated from registration so terminal signaling
  cannot break OpenCode status tracking.
- [x] Do not emit WezTerm control sequences when the process has no valid
  `WEZTERM_PANE` environment value.

#### Tests

- [x] Extend `tests/opencode_plugin_test.mjs` to capture and decode OSC user-var
  writes for idle, running, waiting, failure, heartbeat renewal, and disposal.
- [x] Extend `tests/opencode_plugin_process_test.mjs` to verify a real bundled
  plugin process publishes for pane `42` and clears its value on disposal.
- [x] Add `tests/wezterm_tab_status_test.lua` covering:
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
  - Independent freshness redraw markers across multiple WezTerm windows.
- [x] Add the new Lua test file to the manually maintained `tests/run.lua` list.
- [x] Extend `tests/options_test.lua`, `tests/wezterm_config.lua`, and test helpers
  for the option and synchronous formatter APIs.
- [x] Update `~/Artifacts/wezterm/wezterm_test.lua` to assert that the managed
  consumer enables the option and owns one `format-tab-title` handler.

#### Documentation And Versioning

- [x] Document `opencode_tab_colors`, its default, state priority, semantic
  colors, freshness behavior, and event ownership in `README.md`.
- [x] Enable `opencode_tab_colors = true` in
  `~/Artifacts/wezterm/wezterm.lua` after adapter tests pass.
- [x] Bump the workspace package from `0.10.4` to `0.11.0` because this is a new
  public option and substantial bundled capability.
- [x] Bump the workspace package to `0.11.1` before correcting the already
  deployed `0.11.0` bundle.
- [x] Synchronize `Cargo.toml`, Wisp workspace entries in `Cargo.lock`, and exact
  package-version assertions at `0.11.1`.
- [x] Keep protocol, config, cache, registry, and deployment schemas at version
  7. The plugin and adapter ship in the same content-addressed bundle, so this
  internal user-variable contract does not require a protocol change.
- [x] Do not modify native WezTerm source; documented pane user variables and
  `format-tab-title` already provide the required host APIs.

#### Verification And Deployment

- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [x] Run `cargo test --workspace --locked`.
- [x] Run `rustup run 1.85.0 cargo check --workspace --locked`.
- [x] Run `node --check opencode/wisp.js`.
- [x] Run the OpenCode plugin Node test suites.
- [x] Run `lua tests/run.lua` and `stylua --check .`.
- [x] Run the focused managed WezTerm configuration test in `~/Artifacts`.
- [x] Install Wisp `0.11.0`, confirm `wisp --version`, and deploy the new bundle
  without `--replace-incompatible` because deployment schema v7 is unchanged.
- [x] Refresh the stable OpenCode loader with `wisp opencode install`.
- [x] Install Wisp `0.11.1`, confirm `wisp --version`, and deploy the corrected
  bundle without `--replace-incompatible`.
- [x] Refresh the stable OpenCode loader after deploying `0.11.1`.
- [x] Restart OpenCode so the new bundled plugin publishes pane state.
- [x] Dry-run and push only the managed WezTerm configuration through a reduced
  `~/Artifacts` manifest.
- [x] Run `wisp deploy verify`, validate the live Wisp config, and confirm
  `wisp projects --json` still returns protocol v7.
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
- The aggregate right status, picker session behavior, and strict protocol v7
  contracts remain unchanged.

### Internal File Preview And Editor Reuse

- [x] Keep exact editor reuse and render safe file previews inside Wisp's TUI.

#### Goal

Files mode must remain a project-aware editor launcher while previewing the
highlighted file without starting another editor, creating a host Pane, opening
a companion float, or coordinating through a temporary sidecar.

#### Interaction And Opening

- [x] Keep `Enter` reuse of an exact visible Neovim file target, with stale
  targets falling back to the configured `window`, `right_pane`, or
  `bottom_pane` target.
- [x] Keep `Ctrl-T`, `Ctrl-V`, and `Ctrl-X` as forced duplicate opens in a new
  Window, right Pane, and bottom Pane.
- [x] Keep lazy directory navigation and visible-file `◆` markers unchanged.
- [x] Make `p` toggle internal file Preview in Files mode while retaining
  terminal-text Preview in Windows mode.
- [x] Make `Ctrl-R` reload the visible preview as well as the active listing.

#### Internal Preview

- [x] Add host-neutral preview requests, updates, content states, and rendering
  to `wisp-tui`'s existing auxiliary pane.
- [x] Load previews on a CLI-owned background worker after a 40 ms debounce.
- [x] Reject stale updates by both request ID and exact path.
- [x] Read at most 1 MiB or 10,000 lines and mark truncated output.
- [x] Treat NUL-containing and non-UTF-8 data as binary. Report empty, binary,
  missing, deleted, directory, and unreadable inputs inline without closing the
  picker.
- [x] Clear the active target when Preview is hidden, Commands is shown, Files
  mode is left, or the picker exits, and ignore any stale worker result.

#### Host Options And Adapters

- [x] Keep `file_open` as the strict host placement policy and shared TOML
  openers as argv arrays.
- [x] Replace both structured `file_preview` options with strict booleans. The
  default is `false`; `true` only controls initial visibility because `p`
  remains available on demand.
- [x] Have WezTerm and Neovim pass only `--file-preview` when initial visibility
  is enabled.
- [x] Remove the WezTerm preview Pane lifecycle and Neovim companion-float,
  watcher, resize, scratch-renderer, and cleanup paths.
- [x] Keep Neovim pane-state publication and exact file-target reuse. Extract
  its strict JSON decoder into the bundled `nvim/lua/wisp/json.lua` module.

#### Protocol And Versioning

- [x] Remove `FilePreviewEnvelope`, `FilePreviewState`, their canonical JSON
  fixtures, the public `--file-preview-state-file` option, and all sidecar
  readers and writers.
- [x] Remove the canonical Neovim preview renderer from deployment assets and
  bundle the shared strict JSON decoder instead.
- [x] Bump protocol, config, cache, registry, and deployment schema from 7 to 8
  because the public strict preview envelope is removed. Do not add a v7 reader,
  migration, or compatibility path.
- [x] Bump the workspace package from `0.11.1` to `0.12.0` for the intentional
  breaking adapter option and protocol changes.

#### Session Status Colors

- [x] Color attached-session markers by semantic state: waiting yellow,
  retrying and error red, running green, and idle muted.
- [x] Keep conflict markers red regardless of the reported session state.

#### Tests

- [x] Cover internal preview visibility, Commands restoration, empty targets,
  refresh, stale updates, and terminal-loop rendering in Rust.
- [x] Cover text, empty, binary, unavailable, bounded reads, and
  debounce-to-latest behavior in the CLI.
- [x] Cover strict boolean options and the absence of companion Panes, floats,
  and sidecar arguments in Lua adapter tests.
- [x] Cover semantic attached-session marker colors in TUI rendering tests.
- [x] Update deployment tests for the strict JSON module and removal of the
  preview renderer.

#### Verification And Deployment

- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [x] Run `cargo test --workspace --locked`.
- [x] Run `cargo +1.85.0 check --workspace --locked`.
- [x] Run `node --check opencode/wisp.js` and both OpenCode plugin test suites.
- [x] Run `lua tests/run.lua` and `stylua --check .`.
- [ ] Update managed config to version 8 before a live deployment, install
  package `0.12.0`, deploy with incompatible-schema replacement, and run bundle
  and live-consumer verification.

#### Acceptance Criteria

- Files mode previews the newest highlighted plain-text file without blocking
  input or creating another host/editor surface.
- Preview reads are bounded, stale work cannot replace a newer target, and
  unsupported content is reported without failing the picker.
- `Enter` still activates an exact live editor target when available; forced
  file targets and stale fallbacks retain their existing behavior.
- Both adapters accept only a boolean preview preference and create no preview
  Pane, float, process, watcher, or sidecar.
- All current strict readers require protocol version 8; old internal state is
  discarded and old external configuration or protocol input is rejected.

<!--
- [ ] Feature name
  - Goal:
  - Notes:
  - Verification:
-->

## Fixes

### Open Unopened Projects Directly

- [x] Open an unopened project with no host windows instead of focusing an
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

- [x] Update `App::drill_project` in `crates/wisp-tui/src/lib.rs` to return the
  existing `Selection::Project` before changing focus when all direct-open
  conditions are true: Windows mode, configured project target, not host-open,
  and an empty raw `HostContext::windows` list.
- [x] Check the raw host window list rather than filtered visible rows so an
  active detail query cannot turn `No matching windows` into a project-open
  action.
- [x] Reuse `App::select_project`; do not add a command, selection variant,
  protocol field, or adapter-specific branch to the host-neutral TUI.
- [x] Keep the existing normal-mode `o` handler and add a concise `o Jump
  Project` hint to the Commands pane without increasing its required height.

#### WezTerm Adapter

- [x] Keep `Picker:apply_result` routing `Selection::Project` through
  `Workspace:switch_to_project`.
- [x] Keep `Workspace:switch_to_project` based on `SwitchToWorkspace` and
  `spawn_command(project)`. Do not pre-create a tab, run `wisp open`, or execute
  opener argv for direct project activation.
- [x] Preserve `cwd`, `domain`, `WISP_PROJECT_DIR`, and `WISP_PROJECT_NAME` in
  the default-shell spawn.
- [x] Do not add a default global WezTerm mapping. Host bindings remain
  consumer-owned; `o` is an in-picker command.
- [x] Do not modify native WezTerm source. Its documented `SwitchToWorkspace`
  behavior already creates a window when the target workspace has none.

#### Tests

- [x] Add a focused `crates/wisp-tui/tests/two_pane_test.rs` case proving that
  `Enter` on a configured, not-host-open project with zero raw windows finishes
  with that project's `Selection::Project`.
- [x] Cover the same direct-open result after project search and verify that
  `o` remains search text while search mode is active.
- [x] Preserve or add coverage proving that `Enter` still drills when a raw host
  window exists and that exact pane selection is unchanged.
- [x] Preserve Files and Sessions mode Enter coverage.
- [x] Extend the Commands renderer test to require the `o` project-jump hint.
- [x] Extend `tests/process_adapter_test.lua` or
  `tests/workspace_action_test.lua` to assert that a direct project action has
  the expected workspace, cwd, domain, and Wisp environment, with no `args` so
  WezTerm launches its default program.

#### Documentation And Versioning

- [x] Update the `README.md` picker key table so `Enter` documents the
  unopened-project exception and `o` explicitly says it jumps directly without
  selecting a Window or Pane.
- [x] Document that a direct project selection switches to an existing project
  workspace or creates one default-shell Window at the project directory.
- [x] Keep protocol, config, cache, registry, and deployment schemas at version
  7; this reuses the existing project selection and adapter contract.
- [x] Ship the runtime change under package version `0.10.4`, which was the next
  undeployed version after the prerequisite picker lifecycle fixes.
- [x] Synchronize `Cargo.toml`, the three Wisp workspace entries in
  `Cargo.lock`, and the exact package-version assertion in
  `crates/wisp-cli/tests/cli_test.rs` for the selected release version.

#### Verification

- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [x] Run `cargo test --workspace --locked`.
- [x] Run `cargo +1.85.0 check --workspace --locked`.
- [x] Run `lua tests/run.lua` and `stylua --check .`.
- [x] Manually open the project picker in WezTerm, highlight a `○` project, and
  verify that `Enter` closes the picker and creates one default-shell project
  Window at the configured directory.
- [ ] Manually verify that `o` jumps to both new and existing projects without
  selecting a Window or Pane, while `Enter` still drills into an open project's
  existing windows.
- [x] Before a live deployment, install the selected package version, confirm
  `wisp --version`, run `wisp deploy`, then run `wisp deploy verify`, live config
  validation, and `wisp projects --json` against the consumer config.

#### Acceptance Criteria

- `Enter` on a not-host-open project with no windows creates and enters its
  workspace with one default-shell Window.
- `o` directly jumps into a new or existing project without Window or Pane
  selection and is visible in the Commands pane.
- Existing open-project Window and Pane navigation remains exact and unchanged.
- File, session, host-only workspace, opener, and strict protocol v7 behavior
  remain unchanged.

<!--
- [ ] Fix name
  - Problem:
  - Expected behavior:
  - Verification:
-->
