package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local project = {
  id = "api",
  path = "/Users/test/Repos/api",
  group = "Repos",
  name = "api",
  display_name = "API",
}

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

local function fixture(surface, timeout, overrides)
  overrides = overrides or {}
  local scheduled = {}
  local panes = {}
  local killed = {}
  local preview_specs = {}
  local parsed = {
    PROJECTS = { protocol_version = 7, projects = { project } },
    EMPTY1 = { protocol_version = 7, sequence = 1, state = { state = "empty" } },
    FILE2 = {
      protocol_version = 7,
      sequence = 2,
      state = { state = "file", project = project, path = "/Users/test/Repos/api/src/main.rs" },
    },
    FILE3 = {
      protocol_version = 7,
      sequence = 3,
      state = { state = "file", project = project, path = "/Users/test/Repos/api/src/lib.rs" },
    },
    HIDDEN3 = { protocol_version = 7, sequence = 3, state = { state = "hidden" } },
    HIDDEN4 = { protocol_version = 7, sequence = 4, state = { state = "hidden" } },
    EMPTY5 = { protocol_version = 7, sequence = 5, state = { state = "empty" } },
    MALFORMED = { protocol_version = 7, sequence = 6, state = { state = "empty", extra = true } },
    CANCELLED = { protocol_version = 7, status = "cancelled" },
    ERROR = { protocol_version = 7, status = "error", error = "failed" },
    SELECTED = { protocol_version = 7, status = "selected", selection = { kind = "project", project = project } },
  }
  local next_pane_id = 80
  local picker_args = {}
  local function new_picker_pane(args)
    next_pane_id = next_pane_id + 1
    local pane_id = next_pane_id
    local picker_pane = {
      pane_id = function()
        return pane_id
      end,
      get_foreground_process_info = function()
        return { executable = "/opt/bin/wisp" }
      end,
    }
    function picker_pane:split(spec)
      if overrides.preview_split_error then
        error "preview split failed"
      end
      table.insert(preview_specs, spec)
      next_pane_id = next_pane_id + 1
      local preview_id = next_pane_id
      local preview = {
        pane_id = function()
          return preview_id
        end,
        activate = function() end,
      }
      panes[preview_id] = preview
      return preview
    end
    panes[pane_id] = picker_pane
    table.insert(picker_args, args)
    return picker_pane
  end
  local mux_window = helper.fake_mux_window("default", function(command, _, pane)
    local picker = new_picker_pane(command.args)
    for key, value in pairs(picker) do
      pane[key] = value
    end
    panes[picker:pane_id()] = pane
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
      get_pane = function(id)
        return panes[id]
      end,
      get_tab = function()
        return nil
      end,
    },
    run_child_process = function(args)
      if args[2] == "cli" then
        local pane_id = tonumber(args[5])
        panes[pane_id] = nil
        table.insert(killed, pane_id)
        return true, "", ""
      end
      return true, "PROJECTS", ""
    end,
    json_encode = function()
      return "HOST"
    end,
    json_parse = function(encoded)
      local value = parsed[encoded]
      if value == nil then
        error "malformed JSON"
      end
      return value
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {
    file_open = { default = "bottom_pane" },
    file_preview = { command = { "nvim", "--clean" }, direction = "Right", size = 0.4 },
    picker_timeout_seconds = timeout or 30,
    poll_interval_seconds = 0.05,
  })
  local window = helper.fake_window("default", mux_window, 7)
  local source_pane
  source_pane = helper.fake_pane {
    pane_id = 41,
    split = function(spec)
      return new_picker_pane(spec.args)
    end,
  }

  local function launch()
    if surface == "popup" then
      helper.run_callback(wisp.popup_action "projects", window, source_pane)
    else
      helper.run_callback(wisp.project_picker_action(), window, source_pane)
    end
    return picker_args[#picker_args]
  end

  local function write(path, contents)
    local file = assert(io.open(path, "wb"))
    assert(file:write(contents))
    assert(file:close())
  end

  local function poll()
    local callback = table.remove(scheduled, 1)
    assert(callback, "expected a scheduled poll")
    callback()
  end

  return {
    killed = killed,
    launch = launch,
    panes = panes,
    picker_args = picker_args,
    poll = poll,
    preview_specs = preview_specs,
    scheduled = scheduled,
    wezterm = wezterm,
    window = window,
    write = write,
  }
end

helper.test("file preview launch passes target and sidecar flags and spawns exact Neovim argv", function()
  local test = fixture()
  local args = test.launch()
  local state_path = assert(argument_after(args, "--file-preview-state-file"))

  helper.assert_equal(argument_after(args, "--file-open-target"), "bottom-pane", "file open target")
  helper.assert_equal(has_argument(args, "--file-preview"), true, "file preview initial visibility")
  test.write(state_path, "EMPTY1")
  test.poll()

  helper.assert_equal(#test.preview_specs, 1, "preview split count")
  local spec = test.preview_specs[1]
  helper.assert_table_equal(spec.args, {
    "nvim",
    "--clean",
    "-n",
    "-i",
    "NONE",
    "-S",
    "wezterm/../nvim/lua/wisp/file_preview.lua",
  }, "preview argv")
  helper.assert_equal(spec.direction, "Right", "preview direction")
  helper.assert_equal(spec.size, 0.4, "preview size")
  helper.assert_equal(spec.cwd, nil, "empty preview cwd")
  helper.assert_equal(spec.set_environment_variables.WISP_FILE_PREVIEW_STATE_FILE, state_path, "preview sidecar env")

  test.write(state_path, "FILE2")
  test.poll()
  helper.assert_equal(#test.preview_specs, 1, "one pane across file updates")

  test.write(state_path, "MALFORMED")
  test.poll()
  helper.assert_equal(#test.preview_specs, 1, "malformed state ignored")

  test.write(state_path, "HIDDEN3")
  test.poll()
  helper.assert_equal(#test.killed, 1, "hidden preview close count")
end)

helper.test("unexpected preview exit blocks respawn until hidden then visible", function()
  local test = fixture()
  local args = test.launch()
  local state_path = assert(argument_after(args, "--file-preview-state-file"))
  test.write(state_path, "EMPTY1")
  test.poll()
  local exited_id = test.preview_specs[1] and 82
  test.panes[exited_id] = nil

  test.write(state_path, "FILE2")
  test.poll()
  test.write(state_path, "FILE3")
  test.poll()
  helper.assert_equal(#test.preview_specs, 1, "unexpected exit respawn count")
  helper.assert_equal(#test.window.toasts, 1, "unexpected exit toast count")
  assert(test.window.toasts[1].message:match "toggle", "unexpected exit toast action")

  test.write(state_path, "HIDDEN4")
  test.poll()
  test.write(state_path, "EMPTY5")
  test.poll()
  helper.assert_equal(#test.preview_specs, 2, "respawn after hidden transition")
end)

helper.test("preview launch failure is nonfatal and reports one actionable toast", function()
  local test = fixture(nil, nil, { preview_split_error = true })
  local args = test.launch()
  local state_path = assert(argument_after(args, "--file-preview-state-file"))
  test.write(state_path, "EMPTY1")

  test.poll()
  test.write(state_path, "FILE2")
  test.poll()

  helper.assert_equal(#test.window.toasts, 1, "preview launch toast count")
  assert(test.window.toasts[1].message:match "toggle", "preview launch toast action")
  helper.assert_equal(#test.killed, 0, "picker remains open")
  assert(#test.scheduled > 0, "picker should continue polling")
end)

helper.test("preview pane and sidecar are cleaned for every result path and timeout", function()
  for _, result in ipairs { "CANCELLED", "ERROR", "SELECTED", "BROKEN" } do
    local test = fixture()
    local args = test.launch()
    local state_path = assert(argument_after(args, "--file-preview-state-file"))
    local result_path = assert(argument_after(args, "--result-file"))
    test.write(state_path, "EMPTY1")
    test.poll()
    test.write(result_path, result)
    test.poll()

    helper.assert_equal(#test.killed, 1, result .. " preview close count")
    helper.assert_equal(io.open(state_path, "rb"), nil, result .. " sidecar cleanup")
  end

  local timed_out = fixture(nil, 0.05)
  local args = timed_out.launch()
  local state_path = assert(argument_after(args, "--file-preview-state-file"))
  timed_out.write(state_path, "EMPTY1")
  timed_out.poll()
  helper.assert_equal(#timed_out.killed, 1, "timeout preview close count")
  helper.assert_equal(io.open(state_path, "rb"), nil, "timeout sidecar cleanup")
end)

helper.test("popup replacement closes the superseded preview and removes its sidecar", function()
  local test = fixture "popup"
  local first_args = test.launch()
  local first_state_path = assert(argument_after(first_args, "--file-preview-state-file"))
  test.write(first_state_path, "EMPTY1")
  test.poll()

  test.launch()

  helper.assert_equal(#test.killed, 2, "replacement closes preview and picker")
  helper.assert_equal(io.open(first_state_path, "rb"), nil, "replacement sidecar cleanup")
end)
