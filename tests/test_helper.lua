local helper = {}

function helper.assert_equal(actual, expected, message)
  assert(actual == expected, string.format("%s: expected %s, got %s", message, tostring(expected), tostring(actual)))
end

function helper.assert_table_equal(actual, expected, message)
  helper.assert_equal(#actual, #expected, message .. " length")
  for index, value in ipairs(expected) do
    helper.assert_equal(actual[index], value, message .. " item " .. index)
  end
end

function helper.fake_wezterm(overrides)
  overrides = overrides or {}
  local logs = {}
  local wezterm = {
    home_dir = overrides.home_dir or "/Users/test",
    logs = logs,
    action = {},
    mux = overrides.mux or {
      get_workspace_names = function()
        return {}
      end,
      all_windows = function()
        return {}
      end,
    },
    read_dir = overrides.read_dir or function()
      return {}
    end,
  }

  local function action(name)
    return function(value)
      return { kind = name, value = value }
    end
  end

  wezterm.action.InputSelector = action "InputSelector"
  wezterm.action.SwitchToWorkspace = action "SwitchToWorkspace"
  wezterm.action.SpawnCommandInNewTab = action "SpawnCommandInNewTab"
  wezterm.action.SplitPane = action "SplitPane"

  function wezterm.action_callback(callback)
    return { kind = "Callback", callback = callback }
  end

  function wezterm.log_error(message)
    table.insert(logs, { level = "error", message = message })
  end

  function wezterm.log_warn(message)
    table.insert(logs, { level = "warn", message = message })
  end

  function wezterm.log_info(message)
    table.insert(logs, { level = "info", message = message })
  end

  return wezterm
end

function helper.load_plugin(wezterm)
  package.loaded.wezterm = nil
  package.loaded.plugin = nil
  package.loaded["plugin.init"] = nil
  package.preload.wezterm = function()
    return wezterm
  end

  return require "plugin"
end

function helper.fake_window(workspace)
  local performed = {}
  local window = { performed = performed }

  function window:perform_action(action, pane)
    table.insert(performed, { action = action, pane = pane })
  end

  function window:active_workspace()
    return workspace or "default"
  end

  return window
end

function helper.fake_pane(options)
  options = options or {}
  local pane = {}

  function pane:get_current_working_dir()
    return options.cwd
  end

  function pane:get_domain_name()
    return options.domain or "local"
  end

  return pane
end

function helper.fake_mux_window(workspace)
  local spawned = {}
  local window = { spawned = spawned }

  function window:get_workspace()
    return workspace
  end

  function window:spawn_tab(command)
    table.insert(spawned, command)
  end

  return window
end

function helper.run_callback(action, window, pane, ...)
  helper.assert_equal(action.kind, "Callback", "callback action kind")
  return action.callback(window, pane, ...)
end

function helper.with_fake_time(initial, callback)
  local original_time = os.time
  local now = initial
  os.time = function()
    return now
  end

  local ok, err = pcall(callback, function(value)
    now = value
  end)
  os.time = original_time
  assert(ok, err)
end

function helper.test(name, callback)
  local ok, err = pcall(callback)
  if not ok then
    error(name .. ": " .. tostring(err), 0)
  end
  io.write("ok - ", name, "\n")
end

return helper
