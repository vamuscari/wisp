package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local projects = {
  {
    id = "artifacts",
    path = "/Users/test/Artifacts",
    group = "Home",
    name = "Artifacts",
    display_name = "Artifacts",
  },
}

local function configured(overrides)
  overrides = overrides or {}
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return true, "PROJECTS", ""
    end,
    json_parse = function(value)
      assert(value == "PROJECTS")
      return { protocol_version = 2, projects = projects }
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, overrides)
  return wezterm, wisp
end

helper.test("a project id creates a reusable direct workspace action", function()
  local wezterm, wisp = configured()
  local window = helper.fake_window()
  local pane = helper.fake_pane()

  helper.run_callback(wisp.switch_to_project_action "artifacts", window, pane)

  local switch = window.performed[1].action
  helper.assert_equal(switch.kind, "SwitchToWorkspace", "direct action kind")
  helper.assert_equal(switch.value.name, "wisp:Home/Artifacts", "direct workspace")
  helper.assert_equal(switch.value.spawn.cwd, "/Users/test/Artifacts", "direct cwd")
  helper.assert_equal(switch.value.spawn.domain.DomainName, "local", "direct domain")
  helper.assert_equal(
    switch.value.spawn.set_environment_variables.WISP_PROJECT_DIR,
    "/Users/test/Artifacts",
    "direct project directory"
  )

  local missing_window = helper.fake_window()
  helper.run_callback(wisp.switch_to_project_action "missing", missing_window, pane)
  helper.assert_equal(#missing_window.performed, 0, "missing project action count")
  helper.assert_equal(wezterm.logs[#wezterm.logs].level, "error", "missing project log level")
end)

helper.test("workspace and domain callbacks keep mux policy in the adapter", function()
  local _, wisp = configured {
    domain_for_project = function(project)
      return { DomainName = "unix:" .. project.id }
    end,
    workspace_for_project = function(project)
      return "project:" .. project.id
    end,
  }
  local window = helper.fake_window()

  helper.run_callback(wisp.switch_to_project_action "artifacts", window, helper.fake_pane())

  local switch = window.performed[1].action.value
  helper.assert_equal(switch.name, "project:artifacts", "custom workspace")
  helper.assert_equal(switch.spawn.domain.DomainName, "unix:artifacts", "custom domain")
end)
