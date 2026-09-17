package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"
local Client = assert(loadfile "wezterm/client.lua")()

local function view(overrides)
  local value = {
    window_id = "1001",
    path = "/Users/test/Repos/api/src/main.rs",
    active = true,
    width = 120,
    height = 40,
    bottomline = 30,
    view = {
      lnum = 12,
      col = 0,
      coladd = 0,
      curswant = 0,
      topline = 4,
      topfill = 0,
      leftcol = 0,
      skipcol = 0,
    },
  }
  for key, value_override in pairs(overrides or {}) do
    value[key] = value_override
  end
  return value
end

local function client(parsed_by_text, target_triple)
  local wezterm = helper.fake_wezterm {
    target_triple = target_triple,
    json_parse = function(encoded)
      local parsed = parsed_by_text[encoded]
      if parsed == nil then
        error "invalid JSON"
      end
      return parsed
    end,
  }
  return Client.new(wezterm, {
    get = function()
      return { executable_path = "wisp" }
    end,
  }, 8)
end

helper.test("pane state accepts exact v8 multi-view state with required activity", function()
  local inactive = view { window_id = "1002", path = "C:\\Repos\\api\\README.md" }
  inactive.active = false
  local parsed = {
    protocol_version = 8,
    views = { view(), inactive },
  }
  local subject = client { STATE = parsed }

  local views = assert(subject:parse_nvim_state "STATE")

  helper.assert_equal(#views, 2, "pane state view count")
  helper.assert_equal(views[2].active, false, "inactive view state")
end)

helper.test("pane state rejects malformed envelopes views and viewport values", function()
  local missing_active = view()
  missing_active.active = nil
  local cases = {
    { protocol_version = 7, views = { view() } },
    { protocol_version = 8, views = { view() }, future = true },
    { protocol_version = 8, views = { first = view() } },
    { protocol_version = 8, views = { view { window_id = "" } } },
    { protocol_version = 8, views = { view { path = "relative/main.rs" } } },
    { protocol_version = 8, views = { view { active = 1 } } },
    { protocol_version = 8, views = { missing_active } },
    { protocol_version = 8, views = { view { width = 0 } } },
    { protocol_version = 8, views = { view { bottomline = 3 } } },
    { protocol_version = 8, views = { view { view = { lnum = 1 } } } },
    { protocol_version = 8, views = { view(), view() } },
  }
  local parsed = {}
  for index, value in ipairs(cases) do
    parsed[tostring(index)] = value
  end
  local subject = client(parsed)

  for index in ipairs(cases) do
    local views = subject:parse_nvim_state(tostring(index))
    helper.assert_equal(views, nil, "invalid pane state " .. index)
  end
end)

helper.test("pane state and picker results reject duplicate raw JSON fields", function()
  local duplicate_pane = [[{"protocol_version":8,"views":[],"views":[]}]]
  local duplicate_result = [[{"protocol_version":8,"status":"cancelled","status":"selected"}]]
  local subject = client {
    [duplicate_pane] = { protocol_version = 8, views = {} },
    [duplicate_result] = { protocol_version = 8, status = "selected" },
  }

  helper.assert_equal(subject:parse_nvim_state(duplicate_pane), nil, "duplicate pane field")
  helper.assert_equal(subject:parse_json(duplicate_result), nil, "duplicate result field")
end)

helper.test("pane inspection ignores stale state for known non-Neovim processes", function()
  local subject = client {
    STATE = { protocol_version = 8, views = { view() } },
  }
  local stale = helper.fake_pane {
    process_name = "/bin/zsh",
    user_vars = { WISP_NVIM_STATE = "STATE" },
  }
  local unavailable = helper.fake_pane {
    process_error = "mux process unavailable",
    user_vars = { WISP_NVIM_STATE = "STATE" },
  }

  helper.assert_equal(subject:nvim_views(stale), nil, "known shell state")
  helper.assert_equal(#assert(subject:nvim_views(unavailable)), 1, "unavailable process state")
end)

helper.test("file selections require open policy and strict optional host target", function()
  local subject = client {}
  local project = {
    id = "api",
    path = "/Users/test/Repos/api",
    group = "Repos",
    name = "api",
    display_name = "API",
  }
  local function result(selection)
    return { protocol_version = 8, status = "selected", selection = selection }
  end
  local valid = {
    kind = "file",
    project = project,
    path = "/Users/test/Repos/api/README.md",
    opener = { "nvim", "/Users/test/Repos/api/README.md" },
    open_target = "right_pane",
    reuse_existing = true,
    host_target = { window_id = "17", pane_id = "42" },
  }
  assert(subject:validate_result(result(valid)))

  for _, invalid in ipairs {
    { kind = "file", project = project, path = valid.path, opener = valid.opener, reuse_existing = false },
    {
      kind = "file",
      project = project,
      path = valid.path,
      opener = valid.opener,
      open_target = "tab",
      reuse_existing = false,
    },
    { kind = "file", project = project, path = valid.path, opener = valid.opener, open_target = "window" },
    {
      kind = "file",
      project = project,
      path = valid.path,
      opener = valid.opener,
      open_target = "window",
      reuse_existing = true,
      host_target = { window_id = "17", pane_id = "42", extra = true },
    },
  } do
    local accepted = subject:validate_result(result(invalid))
    helper.assert_equal(accepted, nil, "invalid file selection")
  end
end)
