package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"
local Client = assert(loadfile "wezterm/client.lua")()
local Picker = assert(loadfile "wezterm/picker.lua")()
local Workspace = assert(loadfile "wezterm/workspace.lua")()

local project = {
  id = "api",
  path = "/Users/test/Repos/api",
  group = "Repos",
  name = "api",
  display_name = "API",
}

local function nvim_view(path, window_id, active)
  return {
    window_id = window_id or "1001",
    path = path,
    active = active,
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
end

local function components(overrides)
  overrides = overrides or {}
  local parsed = overrides.parsed or {}
  local wezterm = helper.fake_wezterm {
    mux = overrides.mux,
    target_triple = overrides.target_triple,
    json_encode = function()
      return "RESULT_JSON"
    end,
    json_parse = function(encoded)
      return assert(parsed[encoded], "unexpected JSON " .. tostring(encoded))
    end,
  }
  local values = {
    executable_path = "/opt/bin/wisp",
    file_open = { default = "window" },
    spawn_domain = { DomainName = "local" },
  }
  local options = {
    get = function()
      return values
    end,
    workspace_for = function(_, selected)
      return "wisp:" .. selected.group .. "/" .. selected.name
    end,
    domain_for = function()
      return { DomainName = "local" }
    end,
  }
  local errors = {}
  local client = Client.new(wezterm, options, 8)
  local workspace = Workspace.new(wezterm, options, client, function(_, message)
    table.insert(errors, message)
  end)
  return wezterm, client, workspace, errors
end

helper.test("host pane publishes all valid Neovim views and active file comes from active view", function()
  local state = {
    protocol_version = 8,
    views = {
      nvim_view("/Users/test/Repos/api/src/first.rs", "1001", false),
      nvim_view("/Users/test/Repos/api/src/active.rs", "1002", true),
    },
  }
  local wezterm, client, workspace = components { parsed = { STATE = state } }
  local picker = Picker.new(wezterm, {
    get = function()
      return {}
    end,
  }, client, workspace, {}, function() end)
  local pane = helper.fake_pane {
    pane_id = 42,
    process_name = "/opt/bin/nvim",
    user_vars = {
      WISP_NVIM_STATE = "STATE",
    },
  }
  function pane:get_title()
    return "nvim"
  end

  local host = assert(picker:host_pane(project, { pane = pane, index = 0, is_active = true }))

  helper.assert_equal(#host.nvim_views, 2, "published Neovim view count")
  helper.assert_equal(picker:active_nvim_file(pane), "/Users/test/Repos/api/src/active.rs", "active published file")
end)

local function file_result(open_target, reuse_existing, host_target)
  return {
    protocol_version = 8,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = "/Users/test/Repos/api/src/main.rs",
      opener = { "nvim", "/Users/test/Repos/api/src/main.rs" },
      open_target = open_target,
      reuse_existing = reuse_existing,
      host_target = host_target,
    },
  }
end

local function open_workspace_fixture(options)
  options = options or {}
  local activated = false
  local split_calls = {}
  local target_tab = {
    tab_id = function()
      if options.tab_id_error then
        error "tab disappeared"
      end
      return options.tab_id or 17
    end,
  }
  local mux_window
  local target_pane = helper.fake_pane {
    pane_id = 42,
    process_name = options.process_name or "nvim",
    user_vars = { WISP_NVIM_STATE = options.state_name or "STATE" },
    split = function(spec)
      table.insert(split_calls, spec)
      return {
        activated = false,
        activate = function(self)
          self.activated = true
        end,
      }
    end,
  }
  function target_pane:tab()
    return target_tab
  end
  function target_pane:activate()
    activated = true
  end
  mux_window = helper.fake_mux_window "wisp:Repos/api"
  function mux_window:active_tab()
    return {
      active_pane = function()
        return target_pane
      end,
    }
  end
  function target_pane:window()
    if options.workspace then
      return {
        get_workspace = function()
          if options.workspace_error then
            error "window disappeared"
          end
          return options.workspace
        end,
      }
    end
    return mux_window
  end
  local open = options.open ~= false
  local parsed = {
    STATE = {
      protocol_version = 8,
      views = { nvim_view(options.path or "/Users/test/Repos/api/src/main.rs", "1001", true) },
    },
  }
  local wezterm, _, workspace, errors = components {
    parsed = parsed,
    target_triple = options.target_triple,
    mux = {
      get_workspace_names = function()
        return open and { "wisp:Repos/api" } or {}
      end,
      all_windows = function()
        return open and not options.windows_missing and { mux_window } or {}
      end,
      get_pane = function()
        if options.missing then
          return nil
        end
        return target_pane
      end,
    },
  }
  local window = helper.fake_window("default", mux_window)
  return wezterm, workspace, window, target_pane, mux_window, split_calls, function()
    return activated
  end, errors
end

helper.test("file reuse activates only the exact live pane containing the selected path", function()
  local _, workspace, window, pane, mux_window, _, activated, errors = open_workspace_fixture()
  local result = file_result("bottom_pane", true, { window_id = "17", pane_id = "42" })
  result.selection.opener = nil

  assert(workspace:open_file(window, pane, result))

  helper.assert_equal(activated(), true, "reused pane activation")
  helper.assert_equal(#mux_window.spawned, 0, "reused tab count")
  helper.assert_equal(#window.performed, 1, "reused workspace switch count")
  helper.assert_equal(window.performed[1].action.value.name, "wisp:Repos/api", "reused workspace")
  helper.assert_equal(#errors, 0, "reuse errors")
end)

helper.test("file reuse normalizes Unix drive and UNC path components", function()
  for _, case in ipairs {
    {
      selected = "/Users/test/Repos/api/src/main.rs",
      visible = "/Users/test/Repos/api/src/generated/../main.rs",
    },
    {
      selected = "C:\\Repos\\Api\\src\\main.rs",
      visible = "c:/repos/api/src/generated/../MAIN.rs",
      target_triple = "x86_64-pc-windows-msvc",
    },
    {
      selected = "\\\\Server\\Share\\Api\\src\\main.rs",
      visible = "//server/share/api/src/generated/../MAIN.rs",
      target_triple = "x86_64-pc-windows-msvc",
    },
  } do
    local _, workspace, window, pane, _, _, activated = open_workspace_fixture {
      path = case.visible,
      target_triple = case.target_triple,
    }
    local result = file_result("window", true, { window_id = "17", pane_id = "42" })
    result.selection.path = case.selected
    result.selection.opener = nil

    assert(workspace:open_file(window, pane, result))

    helper.assert_equal(activated(), true, "normalized reuse " .. case.selected)
  end
end)

helper.test("stale moved and changed-file reuse targets silently fall back", function()
  for _, case in ipairs {
    { missing = true },
    { tab_id = 18 },
    { workspace = "scratch" },
    { tab_id_error = true },
    { workspace = "wisp:Repos/api", workspace_error = true },
    { path = "/Users/test/Repos/api/src/other.rs" },
    { process_name = "zsh" },
  } do
    local _, workspace, window, pane, _, split_calls, activated, errors = open_workspace_fixture(case)

    assert(workspace:open_file(window, pane, file_result("right_pane", true, { window_id = "17", pane_id = "42" })))

    helper.assert_equal(activated(), false, "stale target activation")
    helper.assert_equal(#split_calls, 1, "stale target fallback split")
    helper.assert_equal(split_calls[1].direction, "Right", "stale target fallback direction")
    helper.assert_equal(#errors, 0, "silent fallback errors")
  end
end)

helper.test("file open targets create a tab or split with project spawn policy", function()
  for target, direction in pairs { window = false, right_pane = "Right", bottom_pane = "Bottom" } do
    local _, workspace, window, pane, mux_window, split_calls = open_workspace_fixture()

    assert(workspace:open_file(window, pane, file_result(target, false)))

    if direction then
      helper.assert_equal(#split_calls, 1, target .. " split count")
      helper.assert_equal(split_calls[1].direction, direction, target .. " split direction")
      helper.assert_equal(split_calls[1].cwd, project.path, target .. " cwd")
      helper.assert_equal(split_calls[1].set_environment_variables.WISP_PROJECT_NAME, "api", target .. " project env")
      helper.assert_table_equal(split_calls[1].args, { "/opt/bin/wisp", "open", "RESULT_JSON" }, target .. " argv")
    else
      helper.assert_equal(#mux_window.spawned, 1, "window tab count")
    end
  end
end)

helper.test("all file targets create the first workspace window when the project is closed", function()
  for _, target in ipairs { "window", "right_pane", "bottom_pane" } do
    local _, workspace, window, pane = open_workspace_fixture { open = false }

    assert(workspace:open_file(window, pane, file_result(target, false)))

    local switch = window.performed[1].action
    helper.assert_equal(switch.kind, "SwitchToWorkspace", target .. " closed action")
    helper.assert_equal(switch.value.name, "wisp:Repos/api", target .. " closed workspace")
    helper.assert_table_equal(
      switch.value.spawn.args,
      { "/opt/bin/wisp", "open", "RESULT_JSON" },
      target .. " closed argv"
    )
  end
end)

helper.test("all file targets create the first window when a stale workspace name has no mux window", function()
  for _, target in ipairs { "window", "right_pane", "bottom_pane" } do
    local _, workspace, window, pane = open_workspace_fixture { windows_missing = true }

    assert(workspace:open_file(window, pane, file_result(target, false)))

    local switch = window.performed[1].action
    helper.assert_equal(switch.kind, "SwitchToWorkspace", target .. " raced action")
    helper.assert_equal(switch.value.name, "wisp:Repos/api", target .. " raced workspace")
    helper.assert_table_equal(
      switch.value.spawn.args,
      { "/opt/bin/wisp", "open", "RESULT_JSON" },
      target .. " raced argv"
    )
  end
end)
