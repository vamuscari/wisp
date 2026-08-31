package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"
local Popup = assert(loadfile "wezterm/popup.lua")()

local function popup_fixture()
  local wezterm = helper.fake_wezterm()
  local options = {
    get = function()
      return {
        popup = { direction = "Bottom", size = 0.65 },
      }
    end,
  }
  local split_calls = {}
  local closed_pane_ids = {}
  local closed = {}
  local close_failures = {}
  local next_id = 40
  local source_pane = {}
  function source_pane:pane_id()
    return 1
  end
  local function split(pane, spec)
    assert(not closed[pane:pane_id()], "cannot split a closed pane")
    table.insert(split_calls, spec)
    next_id = next_id + 1
    local popup_pane = {}
    function popup_pane:pane_id()
      return next_id
    end
    function popup_pane:split(next_spec)
      return split(self, next_spec)
    end
    return popup_pane
  end
  function source_pane:split(spec)
    return split(self, spec)
  end
  local window = helper.fake_window "wisp:Repos/api"
  function window:window_id()
    return 7
  end
  local workspace = {}
  function workspace:close_pane(pane_id)
    table.insert(closed_pane_ids, pane_id)
    if close_failures[pane_id] then
      return nil, "close failed"
    end
    closed[pane_id] = true
    return true
  end
  local popup = Popup.new(wezterm, options, workspace)
  return popup, window, source_pane, split_calls, closed_pane_ids, close_failures
end

helper.test("popup opens an argv command in a top-level split", function()
  local popup, window, source_pane, split_calls = popup_fixture()

  local handle = assert(popup:open(window, source_pane, {
    id = "projects",
    args = { "/opt/bin/wisp", "pick" },
    cwd = "/repos/api",
    domain = { DomainName = "unix" },
  }))

  helper.assert_equal(handle.id, "projects", "popup ID")
  helper.assert_equal(#split_calls, 1, "split count")
  helper.assert_table_equal(split_calls[1].args, { "/opt/bin/wisp", "pick" }, "popup argv")
  helper.assert_equal(split_calls[1].cwd, "/repos/api", "popup cwd")
  helper.assert_equal(split_calls[1].direction, "Bottom", "popup direction")
  helper.assert_equal(split_calls[1].size, 0.65, "popup size")
  helper.assert_equal(split_calls[1].top_level, true, "top-level split")
  helper.assert_equal(split_calls[1].domain.DomainName, "unix", "popup domain")
end)

helper.test("popup replaces one active surface per GUI window and closes idempotently", function()
  local popup, window, source_pane, _, closed_pane_ids = popup_fixture()
  local first = assert(popup:open(window, source_pane, { id = "projects", args = { "first" } }))
  local second = assert(popup:open(window, source_pane, { id = "sessions", args = { "second" } }))

  helper.assert_table_equal(closed_pane_ids, { 41 }, "replaced popup pane IDs")
  helper.assert_equal(#window.performed, 0, "unrelated pane action count")
  first:close()
  helper.assert_table_equal(closed_pane_ids, { 41 }, "stale handle close pane IDs")
  second:close()
  second:close()
  helper.assert_table_equal(closed_pane_ids, { 41, 42 }, "idempotent active close pane IDs")
end)

helper.test("popup replacement uses stable window IDs across callback wrappers", function()
  local popup, first_window, source_pane, _, closed_pane_ids = popup_fixture()
  local second_window = helper.fake_window "wisp:Repos/api"
  function second_window:window_id()
    return first_window:window_id()
  end

  popup:open(first_window, source_pane, { id = "projects", args = { "first" } })
  popup:open(second_window, source_pane, { id = "sessions", args = { "second" } })

  helper.assert_table_equal(closed_pane_ids, { 41 }, "stable window replacement pane IDs")
end)

helper.test("popup can replace itself when its pane is the action source", function()
  local popup, window, source_pane, split_calls, closed_pane_ids = popup_fixture()
  local first = assert(popup:open(window, source_pane, { id = "projects", args = { "first" } }))

  local second = assert(popup:open(window, first.pane, { id = "sessions", args = { "second" } }))

  helper.assert_equal(second.id, "sessions", "replacement popup ID")
  helper.assert_equal(#split_calls, 2, "self replacement split count")
  helper.assert_table_equal(closed_pane_ids, { 41 }, "self replacement close pane IDs")
end)

helper.test("failed popup closure retains ownership and can be retried", function()
  local popup, window, source_pane, _, closed_pane_ids, close_failures = popup_fixture()
  local handle = assert(popup:open(window, source_pane, { id = "projects", args = { "first" } }))
  close_failures[handle.pane_id] = true

  local closed, close_error = handle:close()

  assert(not closed, "failed closure should be reported")
  helper.assert_equal(close_error, "close failed", "popup close error")
  helper.assert_equal(handle.closed, false, "failed closure ownership")
  close_failures[handle.pane_id] = nil
  assert(handle:close(), "popup closure should be retryable")
  helper.assert_table_equal(closed_pane_ids, { 41, 41 }, "popup close retry pane IDs")
end)

helper.test("failed replacement preserves the existing popup and cleans the new pane", function()
  local popup, window, source_pane, _, closed_pane_ids, close_failures = popup_fixture()
  local first = assert(popup:open(window, source_pane, { id = "projects", args = { "first" } }))
  close_failures[first.pane_id] = true

  local replacement, replace_error = popup:open(window, source_pane, { id = "sessions", args = { "second" } })

  assert(not replacement, "failed replacement should not return a handle")
  assert(tostring(replace_error):match "could not replace", "replacement error")
  helper.assert_equal(first.closed, false, "existing popup ownership")
  helper.assert_table_equal(closed_pane_ids, { 41, 42 }, "failed replacement cleanup pane IDs")
  close_failures[first.pane_id] = nil
  assert(first:close(), "preserved popup should remain closable")
end)

helper.test("double close failure keeps both popup handles managed", function()
  local popup, window, source_pane, _, closed_pane_ids, close_failures = popup_fixture()
  local first = assert(popup:open(window, source_pane, { id = "projects", args = { "first" } }))
  close_failures[41] = true
  close_failures[42] = true

  local replacement = assert(popup:open(window, source_pane, { id = "sessions", args = { "second" } }))

  helper.assert_equal(first.closed, false, "existing managed handle")
  helper.assert_equal(replacement.closed, false, "replacement managed handle")
  helper.assert_equal(first:is_current(), false, "superseded popup ownership")
  helper.assert_equal(replacement:is_current(), true, "replacement popup ownership")
  close_failures[41] = nil
  close_failures[42] = nil
  assert(first:close(), "superseded popup should remain closable")
  assert(replacement:close(), "replacement popup should remain closable")
  helper.assert_table_equal(closed_pane_ids, { 41, 42, 41, 42 }, "managed popup close pane IDs")
end)
