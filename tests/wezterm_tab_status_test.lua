package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"
local ZERO_WIDTH_SPACE = "\u{200b}"

local function pane(id, title, state)
  return {
    pane_id = id,
    title = title,
    user_vars = state and { WISP_OPENCODE_STATUS = state } or {},
  }
end

local function tab(options)
  return {
    tab_id = options.id or 1,
    is_active = options.active == true,
    tab_title = options.title or "",
    active_pane = options.active_pane,
    panes = options.panes,
  }
end

local function item_value(items, field)
  for _, item in ipairs(items or {}) do
    if item[field] ~= nil then
      return item[field]
    end
  end
end

local function formatter(configured)
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      error "tab formatting must not run a child process"
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, configured or { opencode_tab_colors = true })
  return wezterm, wezterm.events["format-tab-title"]
end

helper.test("OpenCode tab colors are disabled by default and installed only when enabled", function()
  local disabled = helper.fake_wezterm()
  helper.load_wezterm_adapter(disabled).apply_to_config({}, {})
  helper.assert_equal(disabled.events["format-tab-title"], nil, "default tab formatter")

  local enabled, format_tab = formatter()
  helper.assert_equal(type(format_tab), "function", "enabled tab formatter")
  helper.assert_equal(type(enabled.events["format-tab-title"]), "function", "registered tab formatter")
end)

helper.test("an inactive pane colors its containing tab", function()
  local wezterm, format_tab = formatter()
  local now = os.time()
  local active = pane(10, "editor")
  local background = pane(11, "agent", "running:" .. now)
  local current = tab { active_pane = active, panes = { active, background } }

  local rendered = format_tab(current, { current }, { active }, {}, false, 20)

  helper.assert_equal(item_value(rendered, "Background").Color, "#50620F", "running background")
  helper.assert_equal(item_value(rendered, "Text"), "editor", "active pane title")
end)

helper.test("tab state uses waiting failure running idle priority", function()
  local _, format_tab = formatter()
  local now = os.time()
  local cases = {
    { states = { "idle" }, expected = "#66615C" },
    { states = { "idle", "running" }, expected = "#50620F" },
    { states = { "idle", "running", "failure" }, expected = "#5E0F04" },
    { states = { "idle", "running", "failure", "waiting" }, expected = "#957C16" },
  }

  for index, case in ipairs(cases) do
    local panes = {}
    for pane_index, state in ipairs(case.states) do
      table.insert(panes, pane(pane_index, state, state .. ":" .. now))
    end
    local current = tab { id = index, active_pane = panes[1], panes = panes }
    local rendered = format_tab(current, { current }, panes, {}, false, 20)
    helper.assert_equal(item_value(rendered, "Background").Color, case.expected, "priority case " .. index)
  end
end)

helper.test("tab state ignores malformed future stale missing and cleared values", function()
  local _, format_tab = formatter()
  local now = os.time()
  local absent = {
    false,
    "",
    "running",
    "unknown:" .. now,
    "running:not-a-time",
    "running:" .. (now + 60),
    "running:" .. (now - 91),
  }

  for index, state in ipairs(absent) do
    local current_pane = pane(index, "shell", state)
    local current = tab { id = index, active_pane = current_pane, panes = { current_pane } }
    helper.assert_equal(format_tab(current, { current }, {}, {}, false, 20), nil, "absent state " .. index)
  end
end)

helper.test("formatter scopes numeric pane snapshots to the requested tab", function()
  local _, format_tab = formatter()
  local now = os.time()
  local idle = pane(42, "current", "idle:" .. now)
  local waiting = pane(84, "other", "waiting:" .. now)
  local current = tab { id = 1, active_pane = idle, panes = { idle } }
  local other = tab { id = 2, active_pane = waiting, panes = { waiting } }

  local rendered = format_tab(current, { current, other }, { waiting }, {}, false, 20)

  helper.assert_equal(item_value(rendered, "Background").Color, "#66615C", "tab-local idle background")
end)

helper.test("formatter preserves titles truncates width and emphasizes the active tab", function()
  local colors = {
    foreground = "foreground",
    waiting_background = "waiting",
  }
  local _, format_tab = formatter { opencode_tab_colors = true, status_colors = colors }
  local now = os.time()
  local active = pane(42, "pane-title", "waiting:" .. now)
  local current = tab { active = true, title = "explicit-title", active_pane = active, panes = { active } }

  local rendered = format_tab(current, { current }, { active }, {}, true, 8)

  helper.assert_equal(item_value(rendered, "Background").Color, "waiting", "custom waiting background")
  helper.assert_equal(item_value(rendered, "Foreground").Color, "foreground", "custom foreground")
  helper.assert_equal(item_value(rendered, "Attribute").Intensity, "Bold", "active tab intensity")
  helper.assert_equal(item_value(rendered, "Text"), "explicit", "truncated explicit title")

  current.is_active = false
  current.tab_title = ""
  rendered = format_tab(current, { current }, { active }, {}, false, 20)
  helper.assert_equal(item_value(rendered, "Attribute").Intensity, "Normal", "inactive tab intensity")
  helper.assert_equal(item_value(rendered, "Text"), "pane-title", "active pane title fallback")
end)

helper.test("tab colors vary a zero-width right-status marker to force freshness redraws", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { opencode_tab_colors = true, status_items = {} })
  local window = helper.fake_window "default"
  local active = helper.fake_pane()

  wezterm.events["update-status"](window, active)
  local first = window.right_status
  wezterm.events["update-status"](window, active)
  local second = window.right_status

  helper.assert_equal(item_value(first, "Attribute").Intensity, "Bold", "first refresh marker")
  helper.assert_equal(item_value(second, "Attribute").Intensity, "Normal", "second refresh marker")
  helper.assert_equal(item_value(first, "Text"), ZERO_WIDTH_SPACE, "first marker text")
  helper.assert_equal(item_value(second, "Text"), ZERO_WIDTH_SPACE, "second marker text")
end)

helper.test("tab refresh markers vary independently for each window", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { opencode_tab_colors = true, status_items = {} })
  local first_window = helper.fake_window "first"
  local second_window = helper.fake_window "second"
  local active = helper.fake_pane()

  wezterm.events["update-status"](first_window, active)
  local first_window_initial = first_window.right_status
  wezterm.events["update-status"](second_window, active)
  local second_window_initial = second_window.right_status
  wezterm.events["update-status"](first_window, active)
  local first_window_next = first_window.right_status
  wezterm.events["update-status"](second_window, active)
  local second_window_next = second_window.right_status

  helper.assert_equal(item_value(first_window_initial, "Attribute").Intensity, "Bold", "first window initial marker")
  helper.assert_equal(item_value(first_window_next, "Attribute").Intensity, "Normal", "first window next marker")
  helper.assert_equal(item_value(second_window_initial, "Attribute").Intensity, "Bold", "second window initial marker")
  helper.assert_equal(item_value(second_window_next, "Attribute").Intensity, "Normal", "second window next marker")
end)

helper.test("formatter failures log and preserve WezTerm default formatting", function()
  local wezterm, format_tab = formatter()
  local current = tab { active_pane = pane(1, "shell"), panes = 42 }

  helper.assert_equal(format_tab(current, { current }, {}, {}, false, 20), nil, "failed formatter result")
  helper.assert_equal(#wezterm.logs, 1, "formatter error count")
  assert(wezterm.logs[1].message:match "wisp tab status format failed", "formatter error message")
end)
