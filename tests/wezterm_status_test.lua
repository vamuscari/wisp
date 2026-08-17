package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local valid_status = {
  protocol_version = 3,
  sessions = {
    waiting = 1,
    running = 2,
    retrying = 3,
    idle = 4,
    error = 5,
  },
}

local function status_text(status)
  local text = {}
  for _, item in ipairs(status) do
    if item.Text then
      table.insert(text, item.Text)
    end
  end
  return text
end

local function status_backgrounds(status)
  local colors = {}
  for _, item in ipairs(status) do
    if item.Background then
      table.insert(colors, item.Background.Color)
    end
  end
  return colors
end

local function status_invisibility(status)
  local values = {}
  for _, item in ipairs(status) do
    if item.Attribute and item.Attribute.Invisible ~= nil then
      table.insert(values, item.Attribute.Invisible)
    end
  end
  return values
end

helper.test("status bar renders compact OpenCode counts before the project name", function()
  local calls = {}
  local wezterm = helper.fake_wezterm {
    run_child_process = function(args)
      table.insert(calls, args)
      return true, "status", ""
    end,
    json_parse = function(value)
      helper.assert_equal(value, "status", "status JSON input")
      return valid_status
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { config_file = "/tmp/wisp.toml" })

  assert(wezterm.events["update-status"], "modern status event should be registered")
  helper.assert_equal(wezterm.events["update-right-status"], nil, "deprecated status event")

  local mux_window = helper.fake_mux_window "wisp:group/repo"
  local window = helper.fake_window("wisp:group/repo", mux_window)
  local pane = helper.fake_pane { mux_window = mux_window }
  wezterm.events["update-status"](window, pane)

  helper.assert_equal(#calls, 1, "status command count")
  helper.assert_table_equal(
    calls[1],
    { "/opt/bin/wisp", "--config", "/tmp/wisp.toml", "opencode", "status", "--json" },
    "status command"
  )
  helper.assert_table_equal(status_text(window.right_status), {
    " OC ",
    " 4 ",
    " 2 ",
    " 1 ",
    " 8 ",
    " repo ",
  }, "status text")
  helper.assert_table_equal(status_backgrounds(window.right_status), {
    "#2A5173",
    "#66615C",
    "#50620F",
    "#957C16",
    "#5E0F04",
    "#333F0A",
  }, "default status colors")
end)

helper.test("status bar omits zero waiting and failure counts", function()
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return {
        protocol_version = 3,
        sessions = { waiting = 0, running = 2, retrying = 0, idle = 4, error = 0 },
      }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local window = helper.fake_window "wisp:group/repo"

  wezterm.events["update-status"](window, helper.fake_pane())

  helper.assert_table_equal(status_text(window.right_status), {
    " OC ",
    " 4 ",
    " 2 ",
    " repo ",
  }, "status text")
  helper.assert_table_equal(status_backgrounds(window.right_status), {
    "#2A5173",
    "#66615C",
    "#50620F",
    "#333F0A",
  }, "status colors")
end)

helper.test("waiting and failure counts flash three times when they appear", function()
  local scheduled = {}
  local wezterm = helper.fake_wezterm {
    call_after = function(interval, callback)
      table.insert(scheduled, { interval = interval, callback = callback })
    end,
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return valid_status
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local window = helper.fake_window "wisp:group/repo"
  function window:effective_config()
    return { text_blink_rate = 0 }
  end

  wezterm.events["update-status"](window, helper.fake_pane())

  helper.assert_table_equal(status_invisibility(window.right_status), {}, "initial attention visibility")
  helper.assert_equal(#scheduled, 3, "cooldown and flash timers")
  helper.assert_equal(scheduled[1].interval, 2, "refresh cooldown")
  helper.assert_equal(scheduled[2].interval, 0.25, "waiting flash interval")
  helper.assert_equal(scheduled[3].interval, 0.25, "failure flash interval")

  scheduled[2].callback()
  scheduled[3].callback()
  helper.assert_table_equal(status_invisibility(window.right_status), {
    true,
    false,
    true,
    false,
  }, "hidden attention counts")

  local next_timer = 4
  while next_timer <= #scheduled do
    scheduled[next_timer].callback()
    next_timer = next_timer + 1
  end

  helper.assert_equal(#scheduled, 13, "three flashes per attention count")
  helper.assert_table_equal(status_invisibility(window.right_status), {}, "solid attention counts")
  helper.assert_table_equal(status_text(window.right_status), {
    " OC ",
    " 4 ",
    " 2 ",
    " 1 ",
    " 8 ",
    " repo ",
  }, "solid status text")

  scheduled[1].callback()
  wezterm.events["update-status"](window, helper.fake_pane())
  helper.assert_equal(#scheduled, 14, "positive counts do not restart flashes")
  helper.assert_equal(scheduled[14].interval, 2, "next refresh cooldown")
end)

helper.test("stale flash timers do not stop a later appearance", function()
  local scheduled = {}
  local sessions = { waiting = 1, running = 0, retrying = 0, idle = 0, error = 0 }
  local wezterm = helper.fake_wezterm {
    call_after = function(interval, callback)
      table.insert(scheduled, { interval = interval, callback = callback })
    end,
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return { protocol_version = 3, sessions = sessions }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local window = helper.fake_window "wisp:group/repo"
  local pane = helper.fake_pane()

  wezterm.events["update-status"](window, pane)
  local stale_expiration = scheduled[2].callback
  scheduled[1].callback()

  sessions = { waiting = 0, running = 0, retrying = 0, idle = 1, error = 0 }
  wezterm.events["update-status"](window, pane)
  scheduled[3].callback()

  sessions = { waiting = 2, running = 0, retrying = 0, idle = 0, error = 0 }
  wezterm.events["update-status"](window, pane)
  stale_expiration()
  scheduled[5].callback()

  helper.assert_table_equal(status_invisibility(window.right_status), { true, false }, "guarded waiting flash")
end)

helper.test("flash transitions rerender every observed GUI window", function()
  local scheduled = {}
  local wezterm = helper.fake_wezterm {
    call_after = function(interval, callback)
      table.insert(scheduled, { interval = interval, callback = callback })
    end,
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return {
        protocol_version = 3,
        sessions = { waiting = 1, running = 0, retrying = 0, idle = 0, error = 0 },
      }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local first = helper.fake_window "wisp:group/one"
  local second = helper.fake_window "wisp:group/two"
  local pane = helper.fake_pane()

  wezterm.events["update-status"](first, pane)
  wezterm.events["update-status"](second, pane)
  scheduled[2].callback()

  helper.assert_table_equal(status_invisibility(first.right_status), { true, false }, "first window flash")
  helper.assert_table_equal(status_invisibility(second.right_status), { true, false }, "second window flash")
end)

helper.test("status bar can be disabled", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { status_bar = false })

  helper.assert_equal(wezterm.events["update-status"], nil, "status event")
end)

helper.test("status options are strict and custom colors are used", function()
  local function assert_config_error(configured, pattern)
    local wezterm = helper.fake_wezterm()
    local wisp = helper.load_wezterm_adapter(wezterm)
    local ok, err = pcall(wisp.apply_to_config, {}, configured)
    assert(not ok, "invalid status option should fail")
    assert(tostring(err):match(pattern), "configuration error should mention " .. pattern .. ": " .. tostring(err))
  end

  assert_config_error({ status_bar = "yes" }, "status_bar")
  assert_config_error({ status_interval_seconds = 0 }, "status_interval_seconds")
  assert_config_error({ status_colors = false }, "status_colors")
  assert_config_error({ status_colors = { foreground = "" } }, "foreground")
  assert_config_error({ status_colors = { retrying_background = "#000000" } }, "retrying_background")
  assert_config_error({ status_colors = { unknown = "#000000" } }, "unknown")

  local colors = {
    foreground = "fg",
    opencode_background = "opencode",
    workspace_background = "workspace",
    active_workspace_background = "active",
    waiting_background = "waiting",
    running_background = "running",
    idle_background = "idle",
    failure_background = "failure",
  }
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return valid_status
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { status_colors = colors })
  local window = helper.fake_window "default"
  wezterm.events["update-status"](window, helper.fake_pane())

  helper.assert_table_equal(status_backgrounds(window.right_status), {
    "opencode",
    "idle",
    "running",
    "waiting",
    "failure",
    "workspace",
  }, "custom status colors")
  for _, item in ipairs(window.right_status) do
    if item.Foreground then
      helper.assert_equal(item.Foreground.Color, "fg", "custom status foreground")
    end
  end
end)

helper.test("status cache is shared throttled and retained across deduplicated failures", function()
  local responses = {
    { true, "first", "" },
    { false, "", "offline" },
    { false, "", "offline" },
    { true, "bad", "" },
    { true, "second", "" },
    { false, "", "offline" },
  }
  local calls = 0
  local release_cooldown
  local wezterm = helper.fake_wezterm {
    call_after = function(interval, callback)
      if interval == 2 then
        release_cooldown = callback
      else
        callback()
      end
    end,
    run_child_process = function()
      calls = calls + 1
      return table.unpack(responses[calls])
    end,
    json_parse = function(value)
      if value == "bad" then
        error "broken JSON"
      end
      if value == "second" then
        return {
          protocol_version = 3,
          sessions = { waiting = 6, running = 7, retrying = 8, idle = 9, error = 10 },
        }
      end
      return valid_status
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local event = wezterm.events["update-status"]
  local first = helper.fake_window "one"
  local second = helper.fake_window "two"
  local pane = helper.fake_pane { process_name = "fish" }
  local function release()
    local callback = assert(release_cooldown, "status cooldown should be scheduled")
    release_cooldown = nil
    callback()
  end

  event(first, pane)
  event(second, pane)
  helper.assert_equal(calls, 1, "shared cache command count")

  release()
  event(first, pane)
  release()
  event(first, pane)
  helper.assert_equal(#wezterm.logs, 1, "duplicate command errors")
  helper.assert_equal(status_text(first.right_status)[4], " 1 ", "retained status after command errors")

  release()
  event(first, pane)
  helper.assert_equal(#wezterm.logs, 2, "different JSON error")
  helper.assert_equal(status_text(first.right_status)[4], " 1 ", "retained status after JSON error")

  release()
  event(first, pane)
  helper.assert_equal(status_text(first.right_status)[4], " 6 ", "updated status after success")
  release()
  event(first, pane)
  helper.assert_equal(#wezterm.logs, 3, "error logged again after success")
end)

helper.test("fractional status intervals remain throttled until the cooldown expires", function()
  local calls = 0
  local scheduled_interval
  local release_cooldown
  local wezterm = helper.fake_wezterm {
    call_after = function(interval, callback)
      scheduled_interval = interval
      release_cooldown = callback
    end,
    run_child_process = function()
      calls = calls + 1
      return true, "status", ""
    end,
    json_parse = function()
      return {
        protocol_version = 3,
        sessions = { waiting = 0, running = 2, retrying = 0, idle = 4, error = 0 },
      }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { status_interval_seconds = 0.5 })
  local event = wezterm.events["update-status"]
  local window = helper.fake_window "default"
  local pane = helper.fake_pane { process_name = "fish" }

  event(window, pane)
  event(window, pane)
  helper.assert_equal(calls, 1, "status command count before cooldown")
  helper.assert_equal(scheduled_interval, 0.5, "scheduled cooldown interval")

  release_cooldown()
  event(window, pane)
  helper.assert_equal(calls, 2, "status command count after cooldown")
end)

helper.test("status response validation rejects every malformed envelope without replacing counts", function()
  local malformed = {
    { protocol_version = 2, sessions = valid_status.sessions },
    { protocol_version = 3, sessions = valid_status.sessions, extra = true },
    { protocol_version = 3 },
    {
      protocol_version = 3,
      sessions = { waiting = 9, running = 2, retrying = 3, idle = 4, error = 5, extra = 0 },
    },
    { protocol_version = 3, sessions = { waiting = 9, running = 2, retrying = 3, idle = 4 } },
    { protocol_version = 3, sessions = { waiting = 9, running = -1, retrying = 3, idle = 4, error = 5 } },
    { protocol_version = 3, sessions = { waiting = 9, running = 2.5, retrying = 3, idle = 4, error = 5 } },
    { protocol_version = 3, sessions = { waiting = 9, running = 2, retrying = "3", idle = 4, error = 5 } },
  }

  for index, response in ipairs(malformed) do
    local wezterm = helper.fake_wezterm {
      run_child_process = function()
        return true, "status", ""
      end,
      json_parse = function()
        return response
      end,
    }
    local wisp = helper.load_wezterm_adapter(wezterm)
    wisp.apply_to_config({}, {})
    local window = helper.fake_window "default"
    wezterm.events["update-status"](window, helper.fake_pane { process_name = "fish" })

    helper.assert_equal(#wezterm.logs, 1, "validation error " .. index)
    helper.assert_table_equal(status_text(window.right_status), {
      " OC ",
      " 0 ",
      " 0 ",
      " default ",
    }, "unchanged counts " .. index)
  end
end)

helper.test("status refresh prevents overlapping commands", function()
  local calls = 0
  local nested_window = helper.fake_window "nested"
  local pane = helper.fake_pane { process_name = "fish" }
  local wezterm
  wezterm = helper.fake_wezterm {
    run_child_process = function()
      calls = calls + 1
      wezterm.events["update-status"](nested_window, pane)
      return true, "status", ""
    end,
    json_parse = function()
      return {
        protocol_version = 3,
        sessions = { waiting = 0, running = 2, retrying = 0, idle = 4, error = 0 },
      }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  wezterm.events["update-status"](helper.fake_window "outer", pane)

  helper.assert_equal(calls, 1, "overlapping status command count")
end)

helper.test("status refresh cooldown starts after the command completes", function()
  local calls = 0
  local command_completed = false
  local release_cooldown
  local wezterm = helper.fake_wezterm {
    call_after = function(_, callback)
      assert(command_completed, "status cooldown should be scheduled after the command")
      command_completed = false
      release_cooldown = callback
    end,
    run_child_process = function()
      calls = calls + 1
      command_completed = true
      return true, "status", ""
    end,
    json_parse = function()
      return {
        protocol_version = 3,
        sessions = { waiting = 0, running = 2, retrying = 0, idle = 4, error = 0 },
      }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local event = wezterm.events["update-status"]
  local window = helper.fake_window "default"
  local pane = helper.fake_pane { process_name = "fish" }

  event(window, pane)
  event(window, pane)
  helper.assert_equal(calls, 1, "status command count before cooldown")
  release_cooldown()
  event(window, pane)
  helper.assert_equal(calls, 2, "status command count after cooldown")
end)

helper.test("status renderer tolerates a stale startup pane", function()
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return true, "status", ""
    end,
    json_parse = function()
      return valid_status
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  local mux_window = helper.fake_mux_window "wisp:Repos/wisp"
  local window = helper.fake_window("wisp:Repos/wisp", mux_window)
  local stale_pane = helper.fake_pane {
    process_error = "pane id 0 not found in mux",
    window_error = "pane id 0 not found in mux",
  }

  wezterm.events["update-status"](window, stale_pane)

  assert(window.right_status, "stale pane should not prevent status rendering")
  helper.assert_equal(status_text(window.right_status)[4], " 1 ", "status count")
  helper.assert_equal(status_text(window.right_status)[6], " wisp ", "fallback project")
end)
