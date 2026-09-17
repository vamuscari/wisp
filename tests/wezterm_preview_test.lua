package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function argument_after(args, flag)
  for index, argument in ipairs(args) do
    if argument == flag then
      return args[index + 1]
    end
  end
end

local function has_argument(args, expected)
  for _, argument in ipairs(args) do
    if argument == expected then
      return true
    end
  end
  return false
end

helper.test("WezTerm enables Wisp's internal file preview without a companion pane", function()
  local scheduled = {}
  local picker_args
  local mux_window = helper.fake_mux_window("default", function(command)
    picker_args = command.args
  end)
  local wezterm = helper.fake_wezterm {
    call_after = function(_, callback)
      table.insert(scheduled, callback)
    end,
    mux = {
      get_workspace_names = function()
        return {}
      end,
      all_windows = function()
        return {}
      end,
      get_pane = function()
        return nil
      end,
      get_tab = function()
        return nil
      end,
    },
    run_child_process = function()
      return true, "PROJECTS", ""
    end,
    json_encode = function()
      return "HOST"
    end,
    json_parse = function(encoded)
      if encoded == "PROJECTS" then
        return {
          protocol_version = 8,
          projects = {
            {
              id = "api",
              path = "/Users/test/Repos/api",
              group = "Repos",
              name = "api",
              display_name = "API",
            },
          },
        }
      end
      error "unexpected JSON"
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { file_preview = true })
  local window = helper.fake_window("default", mux_window, 7)
  local pane = helper.fake_pane { pane_id = 41 }

  helper.run_callback(wisp.project_picker_action(), window, pane)

  assert(picker_args, "picker should launch")
  helper.assert_equal(has_argument(picker_args, "--file-preview"), true, "internal preview flag")
  helper.assert_equal(argument_after(picker_args, "--file-preview-state-file"), nil, "no preview sidecar")
  helper.assert_equal(#mux_window.spawned, 1, "only the picker pane should be spawned")
  helper.assert_equal(#scheduled, 1, "result polling should remain active")
end)
