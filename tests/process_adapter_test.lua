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

local function nvim_state(path)
  return {
    protocol_version = 7,
    views = {
      {
        window_id = "1001",
        path = path,
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
      },
    },
  }
end

local function argument_after(args, flag)
  for index, value in ipairs(args) do
    if value == flag then
      return args[index + 1]
    end
  end
end

local function has_argument(args, expected)
  for _, value in ipairs(args) do
    if value == expected then
      return true
    end
  end
  return false
end

local function fixture(result, mux_overrides)
  mux_overrides = mux_overrides or {}
  local encoded_annotations
  local encoded_result
  local child_calls = {}
  local picker_tab
  local picker_pane
  local picker_mux = helper.fake_mux_window(
    mux_overrides.window_workspace or "wisp:Repos/api",
    function(command, tab, pane)
      picker_tab = tab
      picker_pane = pane
      if mux_overrides.picker_activate_error then
        function tab:activate()
          error(mux_overrides.picker_activate_error)
        end
      end
      if mux_overrides.skip_result then
        return
      end
      local result_path = assert(argument_after(command.args, "--result-file"))
      local file = assert(io.open(result_path, "wb"))
      file:write "RESULT"
      file:close()
    end
  )
  local wezterm = helper.fake_wezterm {
    call_after = mux_overrides.call_after,
    executable_dir = mux_overrides.executable_dir,
    target_triple = mux_overrides.target_triple,
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
      set_active_workspace = mux_overrides.set_active_workspace or function() end,
    },
    run_child_process = function(args)
      table.insert(child_calls, args)
      if mux_overrides.run_child_process then
        return mux_overrides.run_child_process(args)
      end
      return true, "PROJECTS", ""
    end,
    json_encode = function(value)
      if value.status then
        encoded_result = value
        return "RESULT_JSON"
      end
      encoded_annotations = value
      return "ANNOTATIONS"
    end,
    json_parse = function(value)
      if mux_overrides.nvim_states and mux_overrides.nvim_states[value] then
        return mux_overrides.nvim_states[value]
      end
      if value == "PROJECTS" then
        return mux_overrides.projects_result or { protocol_version = 7, projects = projects }
      end
      if value == "RESULT" then
        return result
      end
      error("unexpected JSON fixture " .. value)
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {
    config_file = "/Users/test/.config/wisp/config.toml",
    picker_domain = { DomainName = "unix" },
    single_pane_behavior = mux_overrides.single_pane_behavior,
    window_preview = mux_overrides.window_preview,
  })
  local window = helper.fake_window(mux_overrides.active_workspace or "wisp:Repos/api", picker_mux)
  local pane = helper.fake_pane(mux_overrides.pane)
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
    encoded_result = function()
      return encoded_result
    end,
    wezterm = wezterm,
    window = window,
    wisp = wisp,
  }
end

helper.test("project query rejects unsupported versions before reading the payload", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    projects_result = { protocol_version = 1, projects = "future schema" },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.picker_mux.spawned, 0, "picker spawn count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "project protocol log")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "protocol", "project protocol message")
  helper.assert_equal(test.window.toasts[1].title, "Wisp", "project protocol toast title")
  assert(test.window.toasts[1].message:match "protocol", "project protocol toast message")
end)

helper.test("project query rejects unknown envelope fields", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    projects_result = { protocol_version = 7, projects = projects, future_field = true },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.picker_mux.spawned, 0, "unknown project envelope spawn count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "invalid", "unknown project envelope message")
end)

helper.test("project query requires a JSON array", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    projects_result = { protocol_version = 7, projects = { api = projects[1] } },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.picker_mux.spawned, 0, "object project list spawn count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "project list", "object project list message")
end)

helper.test("project picker launches wisp with a v7 host context", function()
  local test = fixture {
    protocol_version = 7,
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
  helper.assert_equal(test.annotations().protocol_version, 7, "host context protocol")
  helper.assert_table_equal(test.annotations().projects.api.labels, { "current", "open" }, "current labels")
  helper.assert_equal(test.annotations().projects.api.windows, nil, "empty current windows are omitted")
  helper.assert_table_equal(test.annotations().projects.artifacts.labels, { "new" }, "new labels")
  helper.assert_equal(test.annotations().projects.artifacts.windows, nil, "empty new windows are omitted")
  helper.assert_equal(next(test.annotations().workspaces), nil, "empty host workspace map")
  helper.assert_equal(test.picker_tab().activated, true, "picker tab activation")

  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "picker close action")
  helper.assert_equal(test.window.performed[1].pane, test.picker_pane(), "picker close pane")
  helper.assert_equal(test.window.performed[2].action.kind, "SwitchToWorkspace", "project action")
  helper.assert_equal(test.window.performed[2].pane, test.pane, "project original pane")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "project workspace")
end)

helper.test("Windows host context derives project-relative details across case and separators", function()
  local windows_project = {
    id = "api",
    path = "C:\\Users\\Test\\Repos\\Api\\",
    group = "Repos",
    name = "api",
    display_name = "API",
  }
  local active_pane = {
    pane_id = function()
      return 41
    end,
    get_current_working_dir = function()
      return { scheme = "file", file_path = "/c:/users/TEST/Repos/API/src\\Handlers/" }
    end,
    get_foreground_process_name = function()
      return "C:\\Program Files\\Neovim\\bin\\nvim.exe"
    end,
    get_title = function()
      return "nvim"
    end,
  }
  local active_tab = {
    active_pane = function()
      return active_pane
    end,
    get_title = function()
      return "editor"
    end,
    tab_id = function()
      return 17
    end,
  }
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs_with_info = function()
      return { { index = 0, is_active = true, tab = active_tab } }
    end,
  }
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    target_triple = "x86_64-pc-windows-msvc",
    projects_result = { protocol_version = 7, projects = { windows_project } },
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(test.annotations().projects.api.windows[1].detail, "src\\Handlers", "project-relative cwd")
end)

helper.test("Windows host context derives project-relative details from UNC file URLs", function()
  local windows_project = {
    id = "api",
    path = "\\\\SERVER\\Share\\",
    group = "Repos",
    name = "api",
    display_name = "API",
  }
  local active_pane = {
    pane_id = function()
      return 42
    end,
    get_current_working_dir = function()
      return { scheme = "file", host = "server", file_path = "/share/src/" }
    end,
    get_foreground_process_name = function()
      return "C:\\Windows\\System32\\cmd.exe"
    end,
    get_title = function()
      return "server"
    end,
  }
  local active_tab = {
    active_pane = function()
      return active_pane
    end,
    get_title = function()
      return "server"
    end,
    tab_id = function()
      return 18
    end,
  }
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs_with_info = function()
      return { { index = 0, is_active = true, tab = active_tab } }
    end,
  }
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    target_triple = "x86_64-pc-windows-msvc",
    projects_result = { protocol_version = 7, projects = { windows_project } },
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(test.annotations().projects.api.windows[1].detail, "src", "UNC project-relative cwd")
end)

helper.test("project picker forwards the active published Neovim view from an unmanaged workspace", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    active_workspace = "default",
    window_workspace = "default",
    get_workspace_names = function()
      return { "default", "wisp:Repos/api" }
    end,
    pane = {
      process_name = "/opt/homebrew/bin/nvim",
      user_vars = { WISP_NVIM_STATE = "ACTIVE_STATE" },
    },
    nvim_states = { ACTIVE_STATE = nvim_state "/Users/test/Repos/api/src/main.rs" },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(
    argument_after(test.picker_mux.spawned[1].args, "--active-file"),
    "/Users/test/Repos/api/src/main.rs",
    "active Neovim file"
  )
  helper.assert_table_equal(test.annotations().projects.api.labels, { "open" }, "unmanaged project labels")
  helper.assert_equal(test.annotations().workspaces.default.current, true, "current unmanaged workspace")
end)

helper.test("project picker ignores stale Neovim state when a shell is active", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    pane = {
      process_name = "/bin/zsh",
      user_vars = { WISP_NVIM_STATE = "STALE_STATE" },
    },
    nvim_states = { STALE_STATE = nvim_state "/Users/test/Repos/api/src/stale.rs" },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(argument_after(test.picker_mux.spawned[1].args, "--active-file"), nil, "stale active file")
end)

helper.test("project picker accepts Neovim pane context when mux process inspection is unavailable", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    pane = {
      user_vars = { WISP_NVIM_STATE = "MUX_STATE" },
    },
    nvim_states = { MUX_STATE = nvim_state "/Users/test/Repos/api/src/mux.rs" },
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(
    argument_after(test.picker_mux.spawned[1].args, "--active-file"),
    "/Users/test/Repos/api/src/mux.rs",
    "mux active file"
  )
end)

helper.test("host context uses the displayed mux window workspace when client state is stale", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    active_workspace = "default",
    window_workspace = "wisp:Repos/api",
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_table_equal(test.annotations().projects.api.labels, { "current", "open" }, "current labels")
end)

helper.test("host context describes the selected project's WezTerm tabs", function()
  local function pane(id, title, cwd, process)
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
      pane_id = function()
        return id
      end,
    }
  end

  local function tab(id, title, panes, active_index)
    return {
      active_pane = function()
        return panes[active_index]
      end,
      get_title = function()
        return title
      end,
      panes_with_info = function()
        local items = {}
        for index, item in ipairs(panes) do
          table.insert(items, { index = index - 1, is_active = index == active_index, pane = item })
        end
        return items
      end,
      tab_id = function()
        return id
      end,
    }
  end

  local editor = tab(17, "editor", {
    pane(41, "nvim", "/Users/test/Repos/api/src", "/opt/bin/nvim"),
    pane(44, "tests", "/Users/test/Repos/api/tests", "/bin/zsh"),
  }, 1)
  local server = tab(18, "", { pane(42, "server", "/Users/test/Repos/api", "/usr/bin/node") }, 1)
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
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    window_preview = true,
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  local windows = test.annotations().projects.api.windows
  helper.assert_equal(#windows, 2, "host window count")
  helper.assert_equal(windows[1].id, "17", "first tab ID")
  helper.assert_equal(windows[1].label, "editor", "explicit tab title")
  helper.assert_equal(windows[1].detail, "src", "project-relative cwd")
  helper.assert_equal(windows[1].active, true, "active tab")
  helper.assert_equal(#windows[1].panes, 2, "first tab pane count")
  helper.assert_equal(windows[1].panes[1].id, "41", "active pane ID")
  helper.assert_equal(windows[1].panes[1].label, "nvim", "active pane label")
  helper.assert_equal(windows[1].panes[1].detail, "src", "active pane detail")
  helper.assert_equal(windows[1].panes[1].active, true, "active pane state")
  helper.assert_equal(windows[1].panes[2].id, "44", "split pane ID")
  helper.assert_equal(windows[1].panes[2].detail, "tests", "split pane detail")
  helper.assert_equal(windows[2].id, "18", "second tab ID")
  helper.assert_equal(windows[2].label, "server", "pane title fallback")
  helper.assert_equal(windows[2].detail, ".", "project root cwd")
  helper.assert_equal(windows[2].panes[1].id, "42", "second tab pane ID")
  helper.assert_equal(windows[2].active, false, "inactive tab")
  helper.assert_equal(test.annotations().projects.artifacts.windows, nil, "closed project windows are omitted")
  helper.assert_equal(
    argument_after(test.picker_mux.spawned[1].args, "--wezterm-executable"),
    "/Applications/WezTerm.app/Contents/MacOS/wezterm",
    "WezTerm executable argument"
  )
  helper.assert_equal(
    has_argument(test.picker_mux.spawned[1].args, "--window-preview"),
    true,
    "always-visible preview argument"
  )
end)

helper.test("host context includes only live workspaces not owned by projects", function()
  local active_pane = {
    get_current_working_dir = function()
      return { scheme = "file", file_path = "/Users/test" }
    end,
    get_foreground_process_name = function()
      return "/bin/zsh"
    end,
    get_title = function()
      return "shell"
    end,
    pane_id = function()
      return 43
    end,
  }
  local active_tab = {
    active_pane = function()
      return active_pane
    end,
    get_title = function()
      return "default-shell"
    end,
    tab_id = function()
      return 29
    end,
  }
  local default_window = {
    get_workspace = function()
      return "default"
    end,
    tabs_with_info = function()
      return { { index = 0, is_active = true, tab = active_tab } }
    end,
  }
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    active_workspace = "default",
    window_workspace = "default",
    get_workspace_names = function()
      return { "default", "wisp:Repos/api" }
    end,
    all_windows = function()
      return { default_window }
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_table_equal(test.annotations().projects.api.labels, { "open" }, "owned workspace labels")
  helper.assert_equal(test.annotations().projects.default, nil, "host workspace is not a synthetic project")
  local workspace = test.annotations().workspaces.default
  assert(workspace, "live host workspace should be keyed by name")
  helper.assert_equal(workspace.current, true, "current host workspace")
  helper.assert_equal(#workspace.windows, 1, "host workspace window count")
  helper.assert_equal(workspace.windows[1].id, "29", "host workspace tab ID")
  helper.assert_equal(workspace.windows[1].label, "default-shell", "host workspace tab label")
  helper.assert_equal(workspace.windows[1].panes[1].id, "43", "host workspace pane ID")
  helper.assert_equal(workspace.windows[1].active, true, "active host workspace tab")
  helper.assert_equal(
    argument_after(test.picker_mux.spawned[1].args, "--wezterm-executable"),
    "/Applications/WezTerm.app/Contents/MacOS/wezterm",
    "on-demand preview executable"
  )
  helper.assert_equal(
    has_argument(test.picker_mux.spawned[1].args, "--window-preview"),
    false,
    "on-demand preview starts hidden"
  )
end)

helper.test("window picker requests the windows initial view", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    single_pane_behavior = "activate",
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  local spawn = test.picker_mux.spawned[1]
  helper.assert_equal(argument_after(spawn.args, "--initial-view"), "windows", "window initial view")
  helper.assert_equal(argument_after(spawn.args, "--host-context-file") ~= nil, true, "host context argument")
  helper.assert_equal(argument_after(spawn.args, "--single-pane-behavior"), "activate", "single-pane behavior")
end)

helper.test("directory status click opens the project picker in a popup split", function()
  local split_calls = {}
  local popup_pane = {
    pane_id = function()
      return 99
    end,
  }
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    pane = {
      pane_id = 41,
      split = function(command)
        table.insert(split_calls, command)
        local result_path = assert(argument_after(command.args, "--result-file"))
        local file = assert(io.open(result_path, "wb"))
        file:write "RESULT"
        file:close()
        return popup_pane
      end,
    },
  })

  local handled = test.wezterm.events["open-uri"](test.window, test.pane, "wisp://status/directory")

  helper.assert_equal(handled, false, "status URI result")
  helper.assert_equal(#split_calls, 1, "popup split count")
  helper.assert_equal(split_calls[1].top_level, true, "popup top-level split")
  helper.assert_equal(argument_after(split_calls[1].args, "--initial-view"), "projects", "popup initial view")
  helper.assert_equal(#test.picker_mux.spawned, 0, "popup temporary tab count")
  helper.assert_equal(#test.window.performed, 0, "popup pane action count")
  helper.assert_table_equal(test.child_calls[2], {
    "/Applications/WezTerm.app/Contents/MacOS/wezterm",
    "cli",
    "kill-pane",
    "--pane-id",
    "99",
  }, "exact popup close command")
end)

helper.test("popup result waits for exact pane closure to succeed", function()
  local kill_attempts = 0
  local popup_pane = {
    pane_id = function()
      return 99
    end,
  }
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    get_pane = function(pane_id)
      if pane_id == 99 then
        return popup_pane
      end
    end,
    pane = {
      pane_id = 41,
      split = function(command)
        local result_path = assert(argument_after(command.args, "--result-file"))
        local file = assert(io.open(result_path, "wb"))
        file:write "RESULT"
        file:close()
        return popup_pane
      end,
    },
    run_child_process = function(args)
      if args[2] ~= "cli" then
        return true, "PROJECTS", ""
      end
      kill_attempts = kill_attempts + 1
      if kill_attempts == 1 then
        return false, "", "busy"
      end
      return true, "", ""
    end,
  })

  test.wezterm.events["open-uri"](test.window, test.pane, "wisp://status/directory")

  helper.assert_equal(kill_attempts, 2, "popup close retry count")
  helper.assert_equal(#test.window.performed, 0, "cancelled popup host action count")
end)

helper.test("superseded popup discards its stale result", function()
  local scheduled = {}
  local popup_panes = {}
  local result_paths = {}
  local kill_fails = true
  local next_pane_id = 90
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "project", project = projects[1] },
  }, {
    call_after = function(_, callback)
      table.insert(scheduled, callback)
    end,
    get_pane = function(pane_id)
      return popup_panes[pane_id]
    end,
    pane = {
      pane_id = 41,
      split = function(command)
        next_pane_id = next_pane_id + 1
        local pane_id = next_pane_id
        local popup_pane = {
          pane_id = function()
            return pane_id
          end,
        }
        popup_panes[pane_id] = popup_pane
        table.insert(result_paths, assert(argument_after(command.args, "--result-file")))
        return popup_pane
      end,
    },
    run_child_process = function(args)
      if args[2] ~= "cli" then
        return true, "PROJECTS", ""
      end
      if kill_fails then
        return false, "", "busy"
      end
      popup_panes[tonumber(args[5])] = nil
      return true, "", ""
    end,
  })

  test.wezterm.events["open-uri"](test.window, test.pane, "wisp://status/directory")
  test.wezterm.events["open-uri"](test.window, test.pane, "wisp://status/directory")
  kill_fails = false
  local stale_result = assert(io.open(result_paths[1], "wb"))
  stale_result:write "RESULT"
  stale_result:close()

  scheduled[1]()

  helper.assert_equal(#test.window.performed, 0, "superseded popup host action count")
  helper.assert_equal(popup_panes[91], nil, "superseded popup pane cleanup")
end)

helper.test("cancelled picker closes its temporary tab without a host action", function()
  local test = fixture { protocol_version = 7, status = "cancelled" }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "cancel action count")
  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "cancel closes picker")
end)

helper.test("picker cleanup accepts a temporary tab that already exited", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
    picker_activate_error = "tab id 1 not found in mux",
    get_tab = function()
      error "tab id 1 not found in mux"
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 0, "exited picker close action count")
  for _, log in ipairs(test.wezterm.logs) do
    assert(not log.message:match "could not close", "exited picker cleanup should not report a close failure")
  end
end)

helper.test("result projects require every protocol v7 field", function()
  local test = fixture {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "project",
      project = {
        id = "api",
        path = "/Users/test/Repos/api",
        group = "Repos",
        name = "api",
      },
    },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "invalid project action count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "invalid project", "invalid project message")
end)

helper.test("result projects reject unknown protocol fields", function()
  local project = {
    id = "api",
    path = "/Users/test/Repos/api",
    group = "Repos",
    name = "api",
    display_name = "API",
    future_field = true,
  }
  local test = fixture {
    protocol_version = 7,
    status = "selected",
    selection = { kind = "project", project = project },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "unknown project field action count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "invalid project", "unknown project field message")
end)

helper.test("selections reject unknown protocol fields", function()
  local test = fixture {
    protocol_version = 7,
    status = "selected",
    selection = { kind = "project", project = projects[1], future_field = true },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "unknown selection field action count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "valid selection", "unknown selection field message")
end)

helper.test("selections reject malformed opener fields", function()
  local test = fixture {
    protocol_version = 7,
    status = "selected",
    selection = { kind = "project", project = projects[1], opener = "nvim" },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "malformed opener action count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "valid selection", "malformed opener message")
end)

helper.test("selected file delegates its resolved opener to wisp open in an existing workspace", function()
  local project_window = helper.fake_mux_window "wisp:Repos/api"
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[1],
      path = "/Users/test/Repos/api/README.md",
      opener = { "nvim", "/Users/test/Repos/api/README.md" },
      open_target = "window",
      reuse_existing = false,
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
    { "/opt/bin/wisp", "--config", "/Users/test/.config/wisp/config.toml", "open", "RESULT_JSON" },
    "wisp open command"
  )
  helper.assert_equal(test.encoded_result().selection.opener[1], "nvim", "encoded resolved opener")
  helper.assert_equal(project_window.spawned[1].cwd, "/Users/test/Repos/api", "file cwd")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "file workspace switch")
end)

helper.test("wisp open becomes the initial process for a selected file in a new workspace", function()
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[2],
      path = "/Users/test/Artifacts/README.md",
      opener = { "nvim", "/Users/test/Artifacts/README.md" },
      open_target = "window",
      reuse_existing = false,
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
  helper.assert_table_equal(switch.value.spawn.args, {
    "/opt/bin/wisp",
    "--config",
    "/Users/test/.config/wisp/config.toml",
    "open",
    "RESULT_JSON",
  }, "new file wisp open command")
  helper.assert_equal(test.encoded_result().selection.opener[1], "nvim", "new file encoded opener")
  helper.assert_equal(switch.value.spawn.cwd, "/Users/test/Artifacts", "new file cwd")
end)

helper.test("selected host workspace activates the exact existing workspace", function()
  local activated_workspace
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "workspace", workspace = "default" },
  }, {
    get_workspace_names = function()
      return { "default", "wisp:Repos/api" }
    end,
    set_active_workspace = function(workspace)
      activated_workspace = workspace
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(activated_workspace, "default", "activated host workspace")
  helper.assert_equal(#test.window.performed, 1, "host workspace action count")
  helper.assert_equal(test.window.performed[1].action.kind, "CloseCurrentTab", "host workspace picker cleanup")
end)

helper.test("stale host workspace selection does not recreate the workspace", function()
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "workspace", workspace = "default" },
  }, {
    set_active_workspace = function()
      error "default is not an existing workspace"
    end,
  })

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "stale workspace action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "stale workspace log level")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "could not activate", "stale workspace error")
end)

helper.test("selected host workspace pane activates in the exact window and workspace", function()
  local activated = false
  local activated_workspace
  local workspace_window = {
    get_workspace = function()
      return "default"
    end,
  }
  local target_tab = {
    tab_id = function()
      return 29
    end,
  }
  local target_pane = {
    activate = function()
      activated = true
    end,
    tab = function()
      return target_tab
    end,
    window = function()
      return workspace_window
    end,
  }
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "workspace_pane", workspace = "default", window_id = "29", pane_id = "43" },
  }, {
    get_pane = function(id)
      helper.assert_equal(id, 43, "numeric host workspace pane ID")
      return target_pane
    end,
    set_active_workspace = function(workspace)
      activated_workspace = workspace
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, true, "host workspace pane activation")
  helper.assert_equal(activated_workspace, "default", "host workspace pane workspace")
  helper.assert_equal(#test.window.performed, 1, "host workspace pane action count")
end)

helper.test("host workspace panes moved to another workspace are rejected", function()
  local activated = false
  local target_pane = {
    activate = function()
      activated = true
    end,
    tab = function()
      return {
        tab_id = function()
          return 29
        end,
      }
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
    protocol_version = 7,
    status = "selected",
    selection = { kind = "workspace_pane", workspace = "default", window_id = "29", pane_id = "43" },
  }, {
    get_pane = function()
      return target_pane
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, false, "moved host workspace pane activation")
  helper.assert_equal(#test.window.performed, 1, "moved host workspace pane action count")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "no longer belongs", "moved host workspace pane error")
end)

helper.test("selected host pane activates in its captured project window", function()
  local activated = false
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
  }
  local target_tab = {
    tab_id = function()
      return 17
    end,
  }
  local target_pane = {
    activate = function()
      activated = true
    end,
    tab = function()
      return target_tab
    end,
    window = function()
      return project_window
    end,
  }
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "host_pane", project = projects[1], window_id = "17", pane_id = "42" },
  }, {
    get_pane = function(id)
      helper.assert_equal(id, 42, "numeric mux pane ID")
      return target_pane
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, true, "target pane activation")
  helper.assert_equal(test.window.performed[2].action.kind, "SwitchToWorkspace", "workspace switch action")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "target workspace")
  helper.assert_equal(test.window.performed[2].pane, test.pane, "original pane")
end)

helper.test("stale host pane IDs perform no workspace action", function()
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "host_pane", project = projects[1], window_id = "17", pane_id = "42" },
  }, {
    get_pane = function()
      return nil
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.window.performed, 1, "stale pane action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "stale pane log level")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "no longer exists", "stale pane error")
end)

helper.test("host panes moved to another workspace are rejected", function()
  local activated = false
  local target_pane = {
    activate = function()
      activated = true
    end,
    tab = function()
      return {
        tab_id = function()
          return 17
        end,
      }
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
    protocol_version = 7,
    status = "selected",
    selection = { kind = "host_pane", project = projects[1], window_id = "17", pane_id = "42" },
  }, {
    get_pane = function()
      return target_pane
    end,
  })

  helper.run_callback(test.wisp.window_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, false, "moved pane activation")
  helper.assert_equal(#test.window.performed, 1, "moved pane action count")
  helper.assert_equal(test.wezterm.logs[#test.wezterm.logs].level, "error", "moved pane log level")
  assert(test.wezterm.logs[#test.wezterm.logs].message:match "no longer belongs", "moved pane error")
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
    protocol_version = 7,
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

helper.test("close host workspace terminates panes in only the exact workspace", function()
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
  local default_window = {
    get_workspace = function()
      return "default"
    end,
    tabs = function()
      return { tab(pane(201), pane(202)) }
    end,
    tabs_with_info = function()
      return {}
    end,
  }
  local project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs = function()
      return { tab(pane(999)) }
    end,
    tabs_with_info = function()
      return {}
    end,
  }
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = { kind = "close_workspace", workspace = "default" },
  }, {
    all_windows = function()
      return { project_window, default_window }
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
    { executable, "cli", "kill-pane", "--pane-id", "201" },
    "first host workspace pane close"
  )
  helper.assert_table_equal(
    test.child_calls[3],
    { executable, "cli", "kill-pane", "--pane-id", "202" },
    "second host workspace pane close"
  )
  helper.assert_equal(#test.child_calls, 3, "host workspace close child call count")
end)

helper.test("selected file without an opener reports an actionable error", function()
  local test = fixture {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = projects[1],
      path = "/Users/test/Repos/api/README.md",
      open_target = "window",
      reuse_existing = false,
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

helper.test("result envelopes reject unknown protocol fields", function()
  local test = fixture { protocol_version = 7, status = "cancelled", future_field = true }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.wezterm.logs, 1, "unknown result field log count")
  assert(test.wezterm.logs[1].message:match "valid result", "unknown result field message")
end)

helper.test("result envelopes reject fields that do not match their status", function()
  local test = fixture {
    protocol_version = 7,
    status = "cancelled",
    selection = { kind = "project", project = projects[1] },
  }

  helper.run_callback(test.wisp.project_picker_action(), test.window, test.pane)

  helper.assert_equal(#test.wezterm.logs, 1, "inconsistent result log count")
  assert(test.wezterm.logs[1].message:match "valid result", "inconsistent result message")
end)

helper.test("picker pane disappearance fails immediately without waiting for timeout", function()
  local test = fixture({ protocol_version = 7, status = "cancelled" }, {
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

helper.test("OpenCode picker starts in the sessions view", function()
  local test = fixture { protocol_version = 7, status = "cancelled" }

  helper.run_callback(test.wisp.opencode_picker_action(), test.window, test.pane)

  local spawn = test.picker_mux.spawned[1]
  helper.assert_equal(argument_after(spawn.args, "--initial-view"), "sessions", "OpenCode initial view")
end)

helper.test("OpenCode selection focuses an exact registered pane", function()
  local activated = false
  local target_pane = {
    activate = function()
      activated = true
    end,
    window = function()
      return {
        get_workspace = function()
          return "wisp:Repos/api"
        end,
      }
    end,
  }
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "open_code_session",
      project = projects[1],
      session_id = "ses_123",
      opener = { "opencode", "attach", "http://127.0.0.1:4096", "--session", "ses_123" },
      host_item_id = "pane:42",
    },
  }, {
    get_pane = function(id)
      helper.assert_equal(id, 42, "numeric mux pane ID")
      return target_pane
    end,
  })

  helper.run_callback(test.wisp.opencode_picker_action(), test.window, test.pane)

  helper.assert_equal(activated, true, "target pane activation")
  helper.assert_equal(test.window.performed[2].action.kind, "SwitchToWorkspace", "session workspace switch")
  helper.assert_equal(test.window.performed[2].action.value.name, "wisp:Repos/api", "session workspace")
end)

helper.test("stale OpenCode host targets attach in a new project tab", function()
  local project_window = helper.fake_mux_window "wisp:Repos/api"
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "open_code_session",
      project = projects[1],
      session_id = "ses_123",
      opener = { "opencode", "attach", "http://127.0.0.1:4096", "--session", "ses_123" },
      host_item_id = "tab:17",
    },
  }, {
    get_tab = function()
      return nil
    end,
    all_windows = function()
      return { project_window }
    end,
  })

  helper.run_callback(test.wisp.opencode_picker_action(), test.window, test.pane)

  helper.assert_equal(#project_window.spawned, 1, "session fallback tab count")
  helper.assert_table_equal(project_window.spawned[1].args, {
    "/opt/bin/wisp",
    "--config",
    "/Users/test/.config/wisp/config.toml",
    "open",
    "RESULT_JSON",
  }, "session fallback command")
  helper.assert_equal(
    project_window.spawned[1].set_environment_variables.WISP_OPENCODE_SESSION_ID,
    "ses_123",
    "session fallback environment"
  )
end)

helper.test("OpenCode sessions spawned in new workspaces are remembered", function()
  local project_window
  local target_tab = {
    tab_id = function()
      return 17
    end,
    window = function()
      return project_window
    end,
  }
  project_window = {
    get_workspace = function()
      return "wisp:Repos/api"
    end,
    tabs_with_info = function()
      return {}
    end,
    active_tab = function()
      return target_tab
    end,
  }
  local test = fixture({
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "open_code_session",
      project = projects[1],
      session_id = "ses_123",
      opener = { "opencode", "attach", "http://127.0.0.1:4096", "--session", "ses_123" },
    },
  }, {
    get_workspace_names = function()
      return {}
    end,
    all_windows = function()
      return { project_window }
    end,
    get_tab = function(id)
      if id == 17 then
        return target_tab
      end
    end,
  })

  helper.run_callback(test.wisp.opencode_picker_action(), test.window, test.pane)
  helper.run_callback(test.wisp.opencode_picker_action(), test.window, test.pane)

  helper.assert_equal(
    test.annotations().projects.api.session_items.ses_123,
    "tab:17",
    "remembered new-workspace session tab"
  )
end)
