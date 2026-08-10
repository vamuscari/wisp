package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function browse_project_file(wisp, window, pane, project_path, file_path)
  helper.run_callback(wisp.project_picker_action(), window, pane)
  local projects = window.performed[1].action.value
  helper.run_callback(projects.action, window, pane, project_path, "project")
  local menu = window.performed[2].action.value
  helper.run_callback(menu.action, window, pane, menu.choices[2].id, "Browse files")
  local browser = window.performed[3].action.value
  helper.run_callback(browser.action, window, pane, file_path, "file")
end

local function file_system(path)
  if path == "/Users/test/Repos" then
    return { "/Users/test/Repos/api" }
  end
  if path == "/Users/test/Repos/api" then
    return { "/Users/test/Repos/api/README.md" }
  end
  error("not a directory: " .. path)
end

helper.test("a file opens as the initial process in a missing project workspace", function()
  local wezterm = helper.fake_wezterm { read_dir = file_system }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    roots = { "~/Repos" },
    open_file = { "nvim", "--clean" },
  })
  local window = helper.fake_window()
  local pane = helper.fake_pane()

  browse_project_file(wisp, window, pane, "/Users/test/Repos/api", "/Users/test/Repos/api/README.md")

  local switch = window.performed[4].action
  helper.assert_equal(switch.kind, "SwitchToWorkspace", "file workspace action")
  helper.assert_equal(switch.value.name, "wisp:Repos/api", "file workspace name")
  helper.assert_table_equal(
    switch.value.spawn.args,
    { "nvim", "--clean", "/Users/test/Repos/api/README.md" },
    "file command"
  )
  helper.assert_equal(switch.value.spawn.cwd, "/Users/test/Repos/api", "file command cwd")
  helper.assert_equal(switch.value.spawn.domain.DomainName, "local", "file command domain")
  helper.assert_equal(
    switch.value.spawn.set_environment_variables.WISP_PROJECT_NAME,
    "api",
    "file command project name"
  )
end)

helper.test("a file opens in a new tab when its project workspace exists", function()
  local mux_window = helper.fake_mux_window "wisp:Repos/api"
  local wezterm = helper.fake_wezterm {
    read_dir = file_system,
    mux = {
      get_workspace_names = function()
        return { "wisp:Repos/api" }
      end,
      all_windows = function()
        return { mux_window }
      end,
    },
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    roots = { "~/Repos" },
    open_file = function(project, path)
      return { "editor", "--project", project.path, path }
    end,
  })
  local window = helper.fake_window()
  local pane = helper.fake_pane()

  browse_project_file(wisp, window, pane, "/Users/test/Repos/api", "/Users/test/Repos/api/README.md")

  helper.assert_equal(#mux_window.spawned, 1, "spawned file tab count")
  helper.assert_table_equal(
    mux_window.spawned[1].args,
    { "editor", "--project", "/Users/test/Repos/api", "/Users/test/Repos/api/README.md" },
    "callback file command"
  )
  helper.assert_equal(mux_window.spawned[1].cwd, "/Users/test/Repos/api", "existing workspace cwd")
  helper.assert_equal(mux_window.spawned[1].domain.DomainName, "local", "existing workspace domain")
  helper.assert_equal(window.performed[4].action.kind, "SwitchToWorkspace", "existing workspace switch")
  helper.assert_equal(window.performed[4].action.value.name, "wisp:Repos/api", "existing workspace name")
  helper.assert_equal(window.performed[4].action.value.spawn, nil, "existing workspace spawn")
end)

helper.test("selecting a file without an opener reports an actionable error", function()
  local wezterm = helper.fake_wezterm { read_dir = file_system }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, { roots = { "~/Repos" } })
  local window = helper.fake_window()
  local pane = helper.fake_pane()

  browse_project_file(wisp, window, pane, "/Users/test/Repos/api", "/Users/test/Repos/api/README.md")

  helper.assert_equal(#window.performed, 3, "unconfigured file action count")
  helper.assert_equal(wezterm.logs[#wezterm.logs].level, "error", "unconfigured opener log level")
  assert(wezterm.logs[#wezterm.logs].message:match "open_file", "unconfigured opener log message")
end)
