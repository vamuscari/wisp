# OpenCode Plugin Guide

## Session State

- A launch is registered under its selected session, but subagent permission and question events carry child session IDs. Use `info.parentID` from session events and recursive `client.session.children({ path: { id } })` calls to determine the selected session's owned tree.
- `/permission` and `/question` list pending requests for every session in the in-process instance. Heartbeat recovery must filter those lists to the owned session tree instead of copying instance-wide totals into one registration.
- OpenCode invokes v1 `event` hooks without awaiting their promises. Update event-backed state before the first `await`, and generation-guard asynchronous snapshots so late list responses cannot resurrect answered requests.
- During plugin initialization, only global endpoints such as `/global/health` are safe. Instance endpoints re-enter the unresolved `InstanceStore.load`; register synchronously, then query sessions and pending requests from later heartbeats.

## Integration

- `opencode/wisp.js` is a canonical asset embedded by `crates/wisp-cli/src/deploy.rs`. Plugin edits require a rebuilt deployment and an OpenCode restart; `wisp opencode install` manages the stable loader that selects the active bundle.
- `crates/wisp-cli/tests/cli_test.rs::canonical_opencode_plugin_uses_in_process_state_and_argv_registrations` checks source markers, not state transitions. Run `node --test tests/opencode_plugin_test.mjs` for hierarchy and reconciliation behavior; `node --check opencode/wisp.js` only verifies syntax.

## Debugging

- OpenCode's permission and question `message=asking` log lines omit `sessionID`. Correlate their `run=` value with nearby `process` or `stream` lines, then use `created ... parentID=` entries to establish root and child ownership.
