# Wisp Implementation Plan

## Goal

Extract the reusable project sessionizer from Artifacts into a public WezTerm
plugin without carrying personal roots, mappings, themes, shell hooks, or
environment-specific behavior into the plugin.

Wisp will provide a project-first fuzzy picker, local workspace creation, lazy
file-tree navigation, configurable file opening, project-aware tab and split
actions, and a portable filesystem cache.

## Decisions

- The initial release discovers and browses local roots only.
- Projects use `{ DomainName = name }` and default to the `local` domain, not
  `CurrentPaneDomain`, `DefaultDomain`, or a domain ID. A stable named domain
  can be configured for a same-host mux server.
- Generated workspace names use `wisp:group/name` unless explicitly
  overridden.
- Filesystem listings use a configurable TTL with a 60-second default.
- A public refresh action silently invalidates the cache and preloads project
  roots; Wisp installs no refresh binding.
- Discovery caches every immediate entry returned by `wezterm.read_dir`,
  including files, but does not recursively index projects.
- Deeper directories are listed and cached only when the user navigates into
  them.
- The first picker displays projects only. Selecting a project opens a second
  menu with `Open workspace` and `Browse files`.
- Opening a file requires a configured argv or callback. Wisp ships no editor
  default.
- Files open in the selected project's workspace, as the initial process for a
  new workspace or in a new tab for an existing workspace.
- CI initially covers Linux, macOS, and Windows. The implementation must avoid
  platform-specific commands so BSD remains a compatibility target, but BSD
  support will not be claimed as verified until it has a runner.

## Implementation

1. Bootstrap this directory from the existing private `vamuscari/wisp`
   repository while preserving its focused history. Keep `opencode.json`
   local through `.git/info/exclude`.
2. Implement the documented `plugin/init.lua` and
   `apply_to_config(config, options)` boundary in one Lua module. Preserve
   existing user mappings and install no personal defaults.
3. Model projects with a native path, normalized comparison key, display name,
   group, workspace name, and explicit spawn domain. Reject duplicate workspace
   identities while deduplicating repeated paths idempotently.
4. Replace `/usr/bin/find` with protected `wezterm.read_dir` calls. Keep native
   paths for `cwd` and normalize comparison keys for POSIX, Windows drive, and
   UNC forms.
5. Cache listings per directory. Each record stores its native path, normalized
   key, parent/root context, observed entries, and scan timestamp. Expired
   records reload on access.
6. Positively identify directories by successfully reading them. Keep failed
   probes unresolved rather than assuming that every failure is a regular
   file; selecting an unresolved entry delegates to the configured file opener
   and reports any failure without corrupting the cache.
7. Expose a silent refresh action that clears all listings and immediately
   reloads configured roots and fixed local projects. Deeper listings remain
   lazy.
8. Build staged fuzzy selectors: projects, project action, then directory
   contents. Directory selectors support parent and project-menu navigation.
   Cancellation is always a no-op.
9. Default to the explicit local domain for workspace creation while permitting
   a configured named same-host mux domain. Recompute open/new state from the
   mux when rendering choices; do not cache workspace state.
10. Expose reusable picker, refresh, direct-project, project-aware tab, and
    split action constructors. Tabs and splits preserve the active pane's local
    working directory when available and reapply Wisp metadata.
11. For file opening, create a missing project workspace with the configured
    command as its initial process. For an existing workspace, find a mux
    window in that workspace, spawn a tab there, and then switch to it.
12. Add Lua tests, StyLua configuration, GitHub Actions, an MIT license, and
    complete installation, options, compatibility, update, and limitation
    documentation.
13. During uncommitted development, load `plugin/init.lua` directly from the
    checkout because `wezterm.plugin.require` clones committed Git history.
    After the first local commit, verify through a `file://` plugin URL. Rewrite
    all sessionizer call sites, including the direct Artifacts shortcut, before
    removing the local module and obsolete finder scripts.
14. Rename shell consumers to `WISP_PROJECT_DIR` and `WISP_PROJECT_NAME`, remove
    repository-controlled script sourcing, and retain personal global hooks
    and Pipenv behavior only in Artifacts.
15. Verify Wisp and Artifacts while the repository is private, publish Wisp,
    switch Artifacts to the public HTTPS plugin URL, and verify again. Dotfile
    deployment remains a separate operation.

## Test Matrix

- Missing, unreadable, and empty roots
- Files versus directories and unresolved entries
- Exact-path deduplication and duplicate workspace rejection
- POSIX, Windows drive, mixed-separator, and UNC path normalization
- Stable sorting and live open/new labels
- 60-second TTL boundaries and an injected clock
- Silent refresh invalidation and root preloading
- Project-first selection, action menu, nested navigation, parent navigation,
  cancellation, and original-pane retention across selector overlays
- File opening in missing and existing project workspaces
- Explicit local domain selection
- Environment propagation for initial shells, tabs, splits, and file commands
- Active-pane cwd handling, cross-domain cwd rejection, and non-file or missing
  pane URLs
- Preservation of existing user key mappings

## Documentation Contract

The README will document the plugin URL, local-development URL, full option
schema, exported action constructors, minimum WezTerm version
`20240127-113634-bbcac864`, cache semantics, local-only discovery, file opener,
manual update process, BSD compatibility status, and the fact that Wisp cannot
restore processes after mux-server loss.

## Completion Criteria

- All Lua tests and formatting checks pass.
- The plugin loads through WezTerm's config parser at the minimum supported API
  level used by the implementation.
- Artifacts' Lua, Bash, and Zsh checks pass after migration.
- No project-controlled shell file is sourced automatically.
- No personal path, key binding, theme, mux policy, editor, or Pipenv behavior
  ships as a Wisp default.
