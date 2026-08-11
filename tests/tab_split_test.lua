package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local project = {
  id = "api",
  path = "/Users/test/Repos/api",
  group = "Repos",
  name = "api",
  display_name = "api",
}

local function configured(projects)
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return true, "PROJECTS", ""
    end,
    json_parse = function()
      return { protocol_version = 2, projects = projects }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})
  return wisp
end

helper.test("project-aware tabs and splits preserve cwd and metadata", function()
  local wisp = configured { project }
  local window = helper.fake_window "wisp:Repos/api"
  local pane = helper.fake_pane {
    cwd = { scheme = "file", file_path = "/Users/test/Repos/api/src" },
    domain = "local",
  }

  helper.run_callback(wisp.new_tab_action(), window, pane)
  local tab = window.performed[1].action
  helper.assert_equal(tab.kind, "SpawnCommandInNewTab", "new tab action")
  helper.assert_equal(tab.value.cwd, "/Users/test/Repos/api/src", "new tab cwd")
  helper.assert_equal(tab.value.domain.DomainName, "local", "new tab domain")
  helper.assert_equal(tab.value.set_environment_variables.WISP_PROJECT_NAME, "api", "new tab project name")

  helper.run_callback(wisp.split_pane_action("Right", true), window, pane)
  local split = window.performed[2].action
  helper.assert_equal(split.kind, "SplitPane", "split action")
  helper.assert_equal(split.value.direction, "Right", "split direction")
  helper.assert_equal(split.value.top_level, true, "split top-level flag")
  helper.assert_equal(split.value.command.cwd, "/Users/test/Repos/api/src", "split cwd")
end)

helper.test("project-aware spawns use the mux window workspace when client state is stale", function()
  local wisp = configured { project }
  local mux_window = helper.fake_mux_window "wisp:Repos/api"
  local window = helper.fake_window("default", mux_window)
  local pane = helper.fake_pane {
    cwd = { scheme = "file", file_path = "/Users/test/Repos/api/src" },
    domain = "local",
  }

  helper.run_callback(wisp.new_tab_action(), window, pane)

  local command = window.performed[1].action.value
  helper.assert_equal(type(command.set_environment_variables), "table", "project environment")
  helper.assert_equal(command.set_environment_variables.WISP_PROJECT_NAME, "api", "project name")
end)

helper.test("project-aware spawns ignore cwd from a different pane domain", function()
  local wisp = configured { project }
  local window = helper.fake_window "wisp:Repos/api"
  local pane = helper.fake_pane {
    cwd = { scheme = "file", file_path = "/remote/api/src", host = "remote.example" },
    domain = "ssh:remote.example",
  }

  helper.run_callback(wisp.new_tab_action(), window, pane)

  local command = window.performed[1].action.value
  helper.assert_equal(command.cwd, "/Users/test/Repos/api", "cross-domain cwd")
  helper.assert_equal(command.domain.DomainName, "local", "cross-domain target")
end)

helper.test("unknown workspaces retain the current pane domain without non-file cwd", function()
  local wisp = configured {}
  local window = helper.fake_window "scratch"
  local pane = helper.fake_pane {
    cwd = { scheme = "ssh", file_path = "/remote/path" },
    domain = "ssh",
  }

  helper.run_callback(wisp.new_tab_action(), window, pane)

  local command = window.performed[1].action.value
  helper.assert_equal(command.domain, "CurrentPaneDomain", "fallback domain")
  helper.assert_equal(command.cwd, nil, "non-file cwd")
  helper.assert_equal(command.set_environment_variables, nil, "fallback environment")
end)
