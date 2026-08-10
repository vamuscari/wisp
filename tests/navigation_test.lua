package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

helper.test("the picker navigates from projects into a lazy file tree", function()
  local calls = {}
  local entries = {
    ["/Users/test/Repos"] = { "/Users/test/Repos/api" },
    ["/Users/test/Repos/api"] = {
      "/Users/test/Repos/api/src",
      "/Users/test/Repos/api/README.md",
    },
    ["/Users/test/Repos/api/src"] = { "/Users/test/Repos/api/src/main.lua" },
  }
  local wezterm = helper.fake_wezterm {
    read_dir = function(path)
      calls[path] = (calls[path] or 0) + 1
      if not entries[path] then
        error("not a directory: " .. path)
      end
      return entries[path]
    end,
  }
  local wisp = helper.load_plugin(wezterm)
  wisp.apply_to_config({}, { roots = { "~/Repos" } })

  local window = helper.fake_window()
  local pane = helper.fake_pane()
  helper.run_callback(wisp.project_picker_action(), window, pane)

  local projects = window.performed[1].action.value
  helper.assert_equal(#projects.choices, 1, "project-only choice count")
  helper.assert_equal(projects.choices[1].label, "Repos / api [new]", "project choice")
  local project_overlay = helper.fake_pane()
  helper.run_callback(projects.action, window, project_overlay, "/Users/test/Repos/api", projects.choices[1].label)

  local project_menu = window.performed[2].action.value
  helper.assert_equal(window.performed[2].pane, pane, "project menu uses original pane")
  helper.assert_equal(project_menu.title, "Repos / api", "project menu title")
  helper.assert_equal(#project_menu.choices, 2, "project action count")
  helper.assert_equal(project_menu.choices[1].label, "Open workspace", "workspace action")
  helper.assert_equal(project_menu.choices[2].label, "Browse files", "browse action")
  local menu_overlay = helper.fake_pane()
  helper.run_callback(project_menu.action, window, menu_overlay, project_menu.choices[2].id, "Browse files")

  local root_browser = window.performed[3].action.value
  helper.assert_equal(window.performed[3].pane, pane, "file browser uses original pane")
  helper.assert_equal(root_browser.title, "Browse: /Users/test/Repos/api", "root browser title")
  helper.assert_equal(#root_browser.choices, 3, "root browser choice count")
  helper.assert_equal(root_browser.choices[1].label, "Project actions", "root back choice")
  helper.assert_equal(root_browser.choices[2].label, "README.md", "sorted file choice")
  helper.assert_equal(root_browser.choices[3].label, "src", "sorted directory choice")
  helper.assert_equal(calls["/Users/test/Repos/api/src"], nil, "child remains lazy before selection")
  local root_overlay = helper.fake_pane()
  helper.run_callback(root_browser.action, window, root_overlay, "/Users/test/Repos/api/src", "src")

  local src_browser = window.performed[4].action.value
  helper.assert_equal(window.performed[4].pane, pane, "nested browser uses original pane")
  helper.assert_equal(src_browser.title, "Browse: /Users/test/Repos/api/src", "nested browser title")
  helper.assert_equal(src_browser.choices[1].label, "..", "nested parent choice")
  helper.assert_equal(src_browser.choices[2].label, "main.lua", "nested file choice")
  helper.assert_equal(calls["/Users/test/Repos/api/src"], 1, "selected directory read")
  helper.run_callback(src_browser.action, window, pane, src_browser.choices[1].id, "..")

  local returned_root = window.performed[5].action.value
  helper.assert_equal(returned_root.title, root_browser.title, "returned root title")
  helper.assert_equal(calls["/Users/test/Repos/api"], 1, "parent navigation uses cache")
  helper.run_callback(returned_root.action, window, pane, returned_root.choices[1].id, "Project actions")
  helper.assert_equal(window.performed[6].action.value.title, "Repos / api", "returned project menu")
end)

helper.test("cancelling any selector is a no-op", function()
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
  local pane = helper.fake_pane()

  helper.run_callback(wisp.project_picker_action(), window, pane)
  local project_selector = window.performed[1].action.value
  helper.run_callback(project_selector.action, window, pane, nil, nil)
  helper.assert_equal(#window.performed, 1, "project cancellation action count")
  helper.assert_equal(#wezterm.logs, 0, "project cancellation logs")

  helper.run_callback(project_selector.action, window, pane, "/Users/test/Repos/api", "api")
  local project_menu = window.performed[2].action.value
  helper.run_callback(project_menu.action, window, pane, nil, nil)
  helper.assert_equal(#window.performed, 2, "menu cancellation action count")
  helper.assert_equal(#wezterm.logs, 0, "menu cancellation logs")
end)
