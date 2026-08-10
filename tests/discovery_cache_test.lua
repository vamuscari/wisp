package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

helper.test("the project picker caches local directories and excludes files", function()
  helper.with_fake_time(100, function(set_time)
    local calls = {}
    local entries = {
      ["/Users/test/Repos"] = {
        "/Users/test/Repos/zeta",
        "/Users/test/Repos/notes.txt",
        "/Users/test/Repos/api",
      },
      ["/Users/test/Repos/api"] = { "/Users/test/Repos/api/README.md" },
      ["/Users/test/Repos/zeta"] = {},
      ["/Users/test/Artifacts"] = { "/Users/test/Artifacts/wezterm" },
    }
    local wezterm = helper.fake_wezterm {
      read_dir = function(path)
        calls[path] = (calls[path] or 0) + 1
        if not entries[path] then
          error("not a directory: " .. path)
        end
        return entries[path]
      end,
      mux = {
        get_workspace_names = function()
          return { "wisp:Home/Artifacts" }
        end,
        all_windows = function()
          return {}
        end,
      },
    }
    local wisp = helper.load_plugin(wezterm)
    wisp.apply_to_config({}, {
      roots = { "~/Repos" },
      projects = { { group = "Home", name = "Artifacts", path = "~/Artifacts" } },
      cache_ttl_seconds = 60,
    })

    local window = helper.fake_window()
    local pane = helper.fake_pane()
    local picker = wisp.project_picker_action()
    helper.run_callback(picker, window, pane)

    local selector = window.performed[1].action
    helper.assert_equal(selector.kind, "InputSelector", "selector action")
    helper.assert_equal(selector.value.title, "Projects", "selector title")
    helper.assert_equal(selector.value.fuzzy, true, "fuzzy mode")
    helper.assert_equal(#selector.value.choices, 3, "project count")
    helper.assert_equal(selector.value.choices[1].id, "/Users/test/Repos/api", "first project id")
    helper.assert_equal(selector.value.choices[1].label, "Repos / api [new]", "first project label")
    helper.assert_equal(selector.value.choices[2].id, "/Users/test/Artifacts", "fixed project id")
    helper.assert_equal(selector.value.choices[2].label, "Home / Artifacts [open]", "open project label")
    helper.assert_equal(selector.value.choices[3].id, "/Users/test/Repos/zeta", "last project id")

    helper.run_callback(picker, window, pane)
    helper.assert_equal(calls["/Users/test/Repos"], 1, "cached root reads")
    helper.assert_equal(calls["/Users/test/Repos/api"], 1, "cached project reads")
    helper.assert_equal(calls["/Users/test/Repos/notes.txt"], 1, "cached file probes")
    helper.assert_equal(calls["/Users/test/Artifacts"], 1, "cached fixed project reads")

    set_time(159)
    helper.run_callback(picker, window, pane)
    helper.assert_equal(calls["/Users/test/Repos"], 1, "root reads before expiry")

    set_time(160)
    helper.run_callback(picker, window, pane)
    helper.assert_equal(calls["/Users/test/Repos"], 2, "root reads at expiry")
    helper.assert_equal(calls["/Users/test/Repos/api"], 2, "project reads at expiry")
    helper.assert_equal(calls["/Users/test/Repos/notes.txt"], 2, "file probes at expiry")
    helper.assert_equal(calls["/Users/test/Artifacts"], 2, "fixed project reads at expiry")
  end)
end)

helper.test("missing roots warn without hiding fixed projects", function()
  local wezterm = helper.fake_wezterm {
    read_dir = function(path)
      if path == "/Users/test/Artifacts" then
        return {}
      end
      error("missing: " .. path)
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, {
    roots = { "~/missing" },
    projects = { { group = "Home", name = "Artifacts", path = "~/Artifacts" } },
  })

  local window = helper.fake_window()
  helper.run_callback(wisp.project_picker_action(), window, helper.fake_pane())

  helper.assert_equal(#window.performed[1].action.value.choices, 1, "fixed project count")
  helper.assert_equal(wezterm.logs[1].level, "warn", "missing root log level")
end)
