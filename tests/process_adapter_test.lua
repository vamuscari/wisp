package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local projects = {
  {
    id = "api",
    path = "/Users/test/Repos/api",
    group = "Repos",
    name = "api",
    display_name = "API",
  },
  {
    id = "artifacts",
    path = "/Users/test/Artifacts",
    group = "Home",
    name = "Artifacts",
    display_name = "Artifacts",
  },
}

local function argument_after(args, flag)
  for index, value in ipairs(args) do
    if value == flag then
      return args[index + 1]
    end
  end
end

local function fixture(result, mux_overrides)
  mux_overrides = mux_overrides or {}
  local encoded_annotations
  local child_calls = {}
  local picker_tab
  local picker_pane
  local picker_mux = helper.fake_mux_window("scratch", function(command, tab, pane)
    picker_tab = tab
    picker_pane = pane
    if mux_overrides.skip_result then
      return
    end
    local result_path = assert(argument_after(command.args, "--result-file"))
    local file = assert(io.open(result_path, "wb"))
    file:write "RESULT"
    file:close()
  end)
  local wezterm = helper.fake_wezterm {
    mux = {
      get_workspace_names = mux_overrides.get_workspace_names or function()
        return { "wisp:Repos/api" }
      end,
      all_windows = mux_overrides.all_windows or function()
        return {}
      end,
      get_pane = mux_overrides.get_pane or function()
        return picker_pane
      end,
      get_tab = mux_overrides.get_tab or function()
        return nil
      end,
    },
    run_child_process = function(args)
      table.insert(child_calls, args)
      if mux_overrides.run_child_process then
        return mux_overrides.run_child_process(args)
      end
      return true, "PROJECTS", ""
    end,
    json_encode = function(value)
      encoded_annotations = value
      return "ANNOTATIONS"
    end,
    json_parse = function(value)
      if value == "PROJECTS" then
        return projects
      end
      if value == "RESULT" then
        return result
      end
      error("unexpected JSON fixture " .. value)
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    config_file = "/Users/test/.config/wisp/config.toml",
    picker_domain = { DomainName = "unix" },
    wisp_path = "/opt/bin/wisp",
  })
  local window = helper.fake_window("wisp:Repos/api", picker_mux)
  local pane = helper.fake_pane()
  return {
    annotations = function()
      return encoded_annotations
    end,
    child_calls = child_calls,
    pane = pane,
    picker_mux = picker_mux,
    picker_pane = function()
      return picker_pane
    end,
    picker_tab = function()
      return picker_tab
    end,
    wezterm = wezterm,
    window = window,
    wisp = wisp,
  }
end

helper.test("project picker launches wisp with a v2 host context", function()
  local test = fixture {
    protocol_version = 2,
    status = "selected",
    selection = { kind = "project", project = projects[1] },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_table_equal(
    test.child_calls[1],
    { "/opt/bin/wisp", "--config", "/Users/test/.config/wisp/config.toml", "projects", "--json" },
    "project query"
  )
  local spawn = test.picker_mux.spawned[1]
  helper.assert_equal(spawn.domain.DomainName, "unix", "picker domain")
  helper.assert_equal(spawn.args[1], "/opt/bin/wisp", "picker executable")
  helper.assert_equal(argument_after(spawn.args, "--result-file") ~= nil, true, "result argument")
  helper.assert_equal(argument_after(spawn.args, "--host-context-file") ~= nil, true, "host context argument")
  helper.assert_equal(argument_after(spawn.args, "--initial-view"), "projects", "initial view")
  helper.assert_equal(test.annotations().protocol_version, 2, "host context protocol")
  helper.assert_table_equal(test.annotations().projects.api.labels, { "current", "open" }, "current labels")
  helper.assert_equal(test.annotations().projects.api.items, nil, "empty current items are omitted")
  helper.assert_table_equal(test.annotations().projects.artifacts.labels, { "new" }, "new labels")
  helper.assert_equal(test.annotations().projects.artifacts.items, nil, "empty new items are omitted")
  helper.assert_equal(test.picker_tab().activated, true, "picker tab activation")

  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "picker close action")
  helper.assert_equal(test.window.performed[1].pane, test.picker_pane(), "picker close pane")
  helper.assert_equal(test.window.performed[2].action.kind, "SwitchToWorkspace", "project action")
  helper.assert_equal(test.window.performed[2].pane, test.pane, "project original pane")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "project workspace")
end)

helper.test("host context describes the selected project's WezTerm tabs", function()
  local function pane(title, cwd, process)
    return {
      get_current_working_dir = function()
        return { scheme = "file", file_path = cwd }
      end,
      get_foreground_process_name = function()
        return process
      end,
      get_title = function()
        return title
      end,
    }
  end

  local function tab(id, title, active_pane)
    return {
      active_pane = function()
        return active_pane
      end,
      get_title = function()
        return title
      end,
      tab_id = function()
        return id
      end,
    }
  end

  local editor = tab(17, "editor", pane("nvim", "/Users/test/Repos/api/src", "/opt/bin/nvim"))
  local server = tab(18, "", pane("server", "/Users/test/Repos/api", "/usr/bin/node"))
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs_with_info = function()
      return {
        { index = 0, is_active = true, tab = editor },
        { index = 1, is_active = false, tab = server },
      }
    end,
  }
  local test = fixture({ protocol_version = 2, status = "cancelled" }, {
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  local items = test.annotations().projects.api.items
  helper.assert_equal(#items, 2, "host item count")
  helper.assert_equal(items[1].id, "17", "first tab ID")
  helper.assert_equal(items[1].label, "editor", "explicit tab title")
  helper.assert_equal(items[1].detail, "src", "project-relative cwd")
  helper.assert_equal(items[1].active, true, "active tab")
  helper.assert_equal(items[2].id, "18", "second tab ID")
  helper.assert_equal(items[2].label, "server", "pane title fallback")
  helper.assert_equal(items[2].detail, ".", "project root cwd")
  helper.assert_equal(items[2].active, false, "inactive tab")
  helper.assert_equal(test.annotations().projects.artifacts.items, nil, "closed project items are omitted")
end)

helper.test("window picker requests the windows initial view", function()
  local test = fixture { protocol_version = 2, status = "cancelled" }

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  local spawn = test.picker_mux.spawned[1]
  helper.assert_equal(argument_after(spawn.args, "--initial-view"), "windows", "window initial view")
  helper.assert_equal(argument_after(spawn.args, "--host-context-file") ~= nil, true, "host context argument")
end)

helper.test("cancelled picker closes its temporary tab without a host action", function()
  local test = fixture { protocol_version = 2, status = "cancelled" }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "cancel action count")
  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "cancel closes picker")
end)

helper.test("selected file uses its resolved opener in an existing workspace", function()
  local project_window = helper.fake_mux_window "wisp:Repos/api"
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[1],
      path = "/Users/test/Repos/api/README.md",
      opener = { "nvim", "/Users/test/Repos/api/README.md" },
    },
  }, {
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#project_window.spawned, 1, "file tab count")
  helper.assert_table_equal(
    project_window.spawned[1].args,
    { "nvim", "/Users/test/Repos/api/README.md" },
    "resolved file opener"
  )
  helper.assert_equal(project_window.spawned[1].cwd, "/Users/test/Repos/api", "file cwd")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "file workspace switch")
end)

helper.test("selected file becomes the initial process in a new workspace", function()
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[2],
      path = "/Users/test/Artifacts/README.md",
      opener = { "nvim", "/Users/test/Artifacts/README.md" },
    },
  }, {
    get_workspace_names = function()
      return {}
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  local switch = test.window.performed[2].action
  helper.assert_equal(switch.kind, "SwitchToWorkspace", "new file workspace action")
  helper.assert_equal(switch.value.name, "wisp:Home/Artifacts", "new file workspace")
  helper.assert_table_equal(switch.value.spawn.args, { "nvim", "/Users/test/Artifacts/README.md" }, "new file opener")
  helper.assert_equal(switch.value.spawn.cwd, "/Users/test/Artifacts", "new file cwd")
end)

helper.test("selected host item activates its tab in the project workspace", function()
  local activated = false
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
  }
  local target_tab = {
    activate = function()
      activated = true
    end,
    window = function()
      return project_window
    end,
  }
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = { kind = "host_item", project = projects[1], id = "17" },
  }, {
    get_tab = function(id)
      helper.assert_equal(id, 17, "numeric mux tab ID")
      return target_tab
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, true, "target tab activation")
  helper.assert_equal(test.window.performed[2].action.kind, "SwitchToWorkspace", "workspace switch action")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "target workspace")
  helper.assert_equal(test.window.performed[2].pane, test.pane, "original pane")
end)

helper.test("stale host item IDs perform no workspace action", function()
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = { kind = "host_item", project = projects[1], id = "17" },
  }, {
    get_tab = function()
      return nil
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "stale tab action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "stale tab log level")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "no longer exists", "stale tab error")
end)

helper.test("host items moved to another workspace are rejected", function()
  local activated = false
  local target_tab = {
    activate = function()
      activated = true
    end,
    window = function()
      return {
        get_workspace = function()
          return "scratch"
        end,
      }
    end,
  }
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = { kind = "host_item", project = projects[1], id = "17" },
  }, {
    get_tab = function()
      return target_tab
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, false, "moved tab activation")
  helper.assert_equal(#test.window.performed, 1, "moved tab action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "moved tab log level")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "no longer belongs", "moved tab error")
end)

helper.test("close project terminates every pane in only that workspace", function()
  local function pane(id)
    return {
      pane_id = function()
        return id
      end,
    }
  end
  local function tab(...)
    local panes = { ... }
    return {
      panes = function()
        return panes
      end,
    }
  end
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs = function()
      return { tab(pane(101), pane(102)), tab(pane(103)) }
    end,
    tabs_with_info = function()
      return {}
    end,
  }
  local unrelated_window = {
    get_workspace = function()
      return "scratch"
    end,
    tabs = function()
      return { tab(pane(999)) }
    end,
    tabs_with_info = function()
      return {}
    end,
  }
  local test = fixture({
    protocol_version = 2,
    status = "selected",
    selection = { kind = "close_project", project = projects[1] },
  }, {
    all_windows = function()
      return { unrelated_window, project_window }
    end,
    run_child_process = function(args)
      if args[2] == "cli" then
        return true, "", ""
      end
      return true, "PROJECTS", ""
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  local executable = "/Applications/WezTerm.app/Contents/MacOS/wezterm"
  helper.assert_table_equal(
    test.child_calls[2],
    { executable, "cli", "kill-pane", "--pane-id", "101" },
    "first pane close"
  )
  helper.assert_table_equal(
    test.child_calls[3],
    { executable, "cli", "kill-pane", "--pane-id", "102" },
    "second pane close"
  )
  helper.assert_table_equal(
    test.child_calls[4],
    { executable, "cli", "kill-pane", "--pane-id", "103" },
    "third pane close"
  )
  helper.assert_equal(#test.child_calls, 4, "project close child call count")
end)

helper.test("selected file without an opener reports an actionable error", function()
  local test = fixture {
    protocol_version = 2,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[1],
      path = "/Users/test/Repos/api/README.md",
    },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "missing opener action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "missing opener log")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "opener", "missing opener message")
end)

helper.test("invalid result protocol closes the picker and reports an error", function()
  local test = fixture { protocol_version = 1, status = "cancelled" }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "invalid result action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "invalid result log")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "protocol", "protocol error message")
end)

helper.test("picker pane disappearance fails immediately without waiting for timeout", function()
  local test = fixture({ protocol_version = 2, status = "cancelled" }, {
    get_pane = function()
      return nil
    end,
    skip_result = true,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "missing pane close action count")
  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "missing pane cleanup")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "missing pane log")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "exited", "missing pane message")
end)
