package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function picker_choices(wisp)
  local window = helper.fake_window()
  helper.run_callback(wisp.project_picker_action(), window, helper.fake_pane())
  return window.performed[1].action.value.choices
end

helper.test("path identity handles Windows, UNC, and POSIX forms", function()
  local wezterm = helper.fake_wezterm {
    home_dir = "C:\\Users\\test",
    read_dir = function()
      return {}
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    projects = {
      { group = "Repos", name = "Api", path = "C:\\Repos\\Api" },
      { group = "Duplicate", name = "api", path = "c:/repos/api/" },
      { group = "Duplicate", name = "api", path = "C:\\Repos\\group\\..\\Api" },
      { group = "UNC", name = "Shared", path = "\\\\Server\\Share\\Project" },
      { group = "Duplicate", name = "shared", path = "//server/share/project/" },
      { group = "Duplicate", name = "shared", path = "\\\\Server\\Share\\folder\\..\\Project" },
      { group = "Posix", name = "Upper", path = "/Repos/API" },
      { group = "Posix", name = "Lower", path = "/Repos/api" },
      { group = "Duplicate", name = "lower", path = "/Repos/group/../api" },
      { group = "Home", name = "HomeApi", path = "~\\Repos\\HomeApi" },
    },
  })

  local choices = picker_choices(wisp)
  helper.assert_equal(#choices, 5, "deduplicated project count")

  local ids = {}
  for _, choice in ipairs(choices) do
    ids[choice.id] = true
  end
  assert(ids["C:\\Repos\\Api"], "native Windows path should be preserved")
  assert(ids["\\\\Server\\Share\\Project"], "native UNC path should be preserved")
  assert(ids["/Repos/API"], "case-sensitive POSIX path should be retained")
  assert(ids["/Repos/api"], "distinct POSIX casing should be retained")
  assert(ids["C:\\Users\\test\\Repos\\HomeApi"], "Windows home path should expand")
end)

helper.test("distinct projects cannot share a workspace identity", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function()
      return {}
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    projects = {
      { group = "One", name = "api", path = "/one/api", workspace = "wisp:api" },
      { group = "Two", name = "api", path = "/two/api", workspace = "wisp:api" },
    },
  })

  local ok, err = pcall(function()
    picker_choices(wisp)
  end)
  assert(not ok, "duplicate workspaces should be rejected")
  assert(tostring(err):match "duplicate workspace", "duplicate workspace error should be actionable")
end)

helper.test("distinct projects cannot share a configured id", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function()
      return {}
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    projects = {
      { id = "api", group = "One", name = "api", path = "/one/api" },
      { id = "api", group = "Two", name = "api", path = "/two/api" },
    },
  })

  local ok, err = pcall(function()
    picker_choices(wisp)
  end)
  assert(not ok, "duplicate project ids should be rejected")
  assert(tostring(err):match "duplicate project id", "duplicate project id error should be actionable")
end)
