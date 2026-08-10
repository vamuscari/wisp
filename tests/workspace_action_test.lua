package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function open_project_menu(wisp, window, pane, project_path)
  helper.run_callback(wisp.project_picker_action(), window, pane)
  local projects = window.performed[1].action.value
  helper.run_callback(projects.action, window, pane, project_path, "project")
  return window.performed[2].action.value
end

helper.test("opening a project uses its namespaced workspace and local domain", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function(path)
      if path == "/Users/test/Repos" then
        return { "/Users/test/Repos/api" }
      end
      if path == "/Users/test/Repos/api" then
        return {}
      end
      error "not a directory"
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, { roots = { "~/Repos" } })
  local window = helper.fake_window()
  local pane = helper.fake_pane { domain = "ssh" }

  local menu = open_project_menu(wisp, window, pane, "/Users/test/Repos/api")
  helper.run_callback(menu.action, window, pane, menu.choices[1].id, "Open workspace")

  local switch = window.performed[3].action
  helper.assert_equal(switch.kind, "SwitchToWorkspace", "workspace action kind")
  helper.assert_equal(switch.value.name, "wisp:Repos/api", "workspace name")
  helper.assert_equal(switch.value.spawn.cwd, "/Users/test/Repos/api", "workspace cwd")
  helper.assert_equal(switch.value.spawn.domain.DomainName, "local", "workspace domain")
  helper.assert_equal(
    switch.value.spawn.set_environment_variables.WISP_PROJECT_DIR,
    "/Users/test/Repos/api",
    "workspace project directory"
  )
  helper.assert_equal(switch.value.spawn.set_environment_variables.WISP_PROJECT_NAME, "api", "workspace project name")
end)

helper.test("a configured project id creates a reusable direct action", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function(path)
      if path == "/Users/test/Artifacts" then
        return {}
      end
      error "not a directory"
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    projects = {
      { id = "artifacts", group = "Home", name = "Artifacts", path = "~/Artifacts" },
    },
  })
  local window = helper.fake_window()
  local pane = helper.fake_pane()

  helper.run_callback(wisp.switch_to_project_action "artifacts", window, pane)
  helper.assert_equal(window.performed[1].action.kind, "SwitchToWorkspace", "direct action kind")
  helper.assert_equal(window.performed[1].action.value.name, "wisp:Home/Artifacts", "direct workspace")

  local missing_window = helper.fake_window()
  helper.run_callback(wisp.switch_to_project_action "missing", missing_window, pane)
  helper.assert_equal(#missing_window.performed, 0, "missing project action count")
  helper.assert_equal(wezterm.logs[#wezterm.logs].level, "error", "missing project log level")
end)

helper.test("a configured spawn domain does not inherit the active pane domain", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function(path)
      if path == "/Users/test/Artifacts" then
        return {}
      end
      error "not a directory"
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    projects = {
      { id = "artifacts", group = "Home", name = "Artifacts", path = "~/Artifacts" },
    },
    spawn_domain = { DomainName = "unix" },
  })
  local window = helper.fake_window()
  local pane = helper.fake_pane { domain = "ssh" }

  helper.run_callback(wisp.switch_to_project_action "artifacts", window, pane)

  helper.assert_equal(window.performed[1].action.value.spawn.domain.DomainName, "unix", "configured spawn domain")
end)
