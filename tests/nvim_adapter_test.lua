package.path = "./?.lua;./?/init.lua;./nvim/lua/?.lua;./nvim/lua/?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local project = {
  id = "api",
  path = "/Users/test/Repos/api with spaces",
  group = "Repos",
  name = "api",
  display_name = "API",
}

local function argument_after(args, flag)
  for index, value in ipairs(args) do
    if value == flag then
      return args[index + 1]
    end
  end
end

local function fake_vim(result)
  local state = {
    buffers = {},
    commands = {},
    keymaps = {},
    notifications = {},
    tab_calls = {},
    user_commands = {},
    windows = {},
  }
  local temporary = os.tmpname()
  os.remove(temporary)
  local vim = {
    api = {},
    cmd = {},
    env = {},
    fn = {},
    json = {},
    keymap = {},
    log = { levels = { ERROR = 1 } },
    o = { columns = 100, lines = 40 },
    t = {},
  }

  function vim.api.nvim_create_user_command(name, callback, options)
    state.user_commands[name] = { callback = callback, options = options }
  end

  function vim.api.nvim_get_current_tabpage()
    return 7
  end

  function vim.api.nvim_create_buf()
    return 11
  end

  function vim.api.nvim_open_win(buffer, enter, config)
    table.insert(state.windows, { buffer = buffer, config = config, enter = enter })
    return 13
  end

  function vim.api.nvim_win_is_valid(window)
    return window == 13
  end

  function vim.api.nvim_win_close(window, force)
    state.closed_window = { force = force, window = window }
  end

  function vim.api.nvim_buf_is_valid(buffer)
    return buffer == 11
  end

  function vim.api.nvim_buf_delete(buffer, options)
    state.deleted_buffer = { buffer = buffer, options = options }
  end

  function vim.api.nvim_tabpage_is_valid(tab)
    return tab == 7
  end

  function vim.api.nvim_tabpage_call(tab, callback)
    table.insert(state.tab_calls, tab)
    callback()
  end

  function vim.api.nvim_cmd(command)
    table.insert(state.commands, command)
  end

  function vim.api.nvim_set_option_value(name, value, options)
    table.insert(state.buffers, { name = name, options = options, value = value })
  end

  function vim.keymap.set(mode, lhs, rhs, options)
    table.insert(state.keymaps, { lhs = lhs, mode = mode, options = options, rhs = rhs })
  end

  function vim.cmd.startinsert()
    state.started_insert = true
  end

  function vim.fn.tempname()
    return temporary
  end

  function vim.fn.delete(path)
    os.remove(path)
  end

  function vim.fn.jobstart(args, options)
    state.job = { args = args, options = options }
    local result_path = assert(argument_after(args, "--result-file"))
    local file = assert(io.open(result_path, "wb"))
    file:write "RESULT"
    file:close()
    options.on_exit(42, 0)
    return 42
  end

  function vim.json.decode(encoded)
    assert(encoded == "RESULT")
    return result
  end

  function vim.schedule(callback)
    callback()
  end

  function vim.notify(message, level)
    table.insert(state.notifications, { level = level, message = message })
  end

  return vim, state
end

local function load_adapter(vim)
  _G.vim = vim
  return assert(loadfile "nvim/lua/wisp/init.lua")("/opt/bin/wisp", "wisp-deployment-v1")
end

helper.test("Neovim adapter rejects ordinary runtimepath loading", function()
  local vim = fake_vim { protocol_version = 2, status = "cancelled" }
  _G.vim = vim
  package.loaded.wisp = nil
  package.loaded["wisp.init"] = nil
  local original_path = package.path
  package.path = "./nvim/lua/?.lua;./nvim/lua/?/init.lua;" .. package.path
  local ok, err = pcall(require, "wisp")
  package.path = original_path

  assert(not ok, "checkout runtimepath loading should fail")
  assert(tostring(err):match "deployed runtime", "deployment error should be actionable")
end)

helper.test("Neovim setup registers a command, optional mapping, and inherited metadata", function()
  local vim, state = fake_vim { protocol_version = 2, status = "cancelled" }
  vim.env.WISP_PROJECT_DIR = "/Users/test/Repos/api"
  vim.env.WISP_PROJECT_NAME = "api"
  local wisp = load_adapter(vim)

  wisp.setup { command = "WispPick", keymap = "<leader>p" }

  helper.assert_equal(type(state.user_commands.WispPick.callback), "function", "user command")
  helper.assert_equal(state.user_commands.WispPick.options.force, true, "command replacement")
  helper.assert_equal(state.keymaps[1].lhs, "<leader>p", "picker mapping")
  helper.assert_equal(vim.t.wisp_project_dir, "/Users/test/Repos/api", "inherited project directory")
  helper.assert_equal(vim.t.wisp_project_name, "api", "inherited project name")
end)

helper.test("Neovim picker applies a file result to the originating tab", function()
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = "/Users/test/Repos/api with spaces/README.md",
      opener = { "nvim", "/Users/test/Repos/api with spaces/README.md" },
    },
  }
  local wisp = load_adapter(vim)
  wisp.setup {
    config_file = "/Users/test/.config/wisp/config.toml",
    height = 0.5,
    width = 0.8,
  }

  wisp.open()

  helper.assert_table_equal(state.job.args, {
    "/opt/bin/wisp",
    "--config",
    "/Users/test/.config/wisp/config.toml",
    "pick",
    "--result-file",
    argument_after(state.job.args, "--result-file"),
  }, "picker argv")
  helper.assert_equal(state.windows[1].config.width, 80, "float width")
  helper.assert_equal(state.windows[1].config.height, 20, "float height")
  helper.assert_equal(state.tab_calls[1], 7, "originating tab")
  helper.assert_equal(state.commands[1].cmd, "tcd", "tab cwd command")
  helper.assert_equal(state.commands[1].args[1], project.path, "tab cwd path")
  helper.assert_equal(state.commands[2].cmd, "edit", "file edit command")
  helper.assert_equal(state.commands[2].args[1], "/Users/test/Repos/api with spaces/README.md", "file path")
  helper.assert_equal(vim.t.wisp_project_dir, project.path, "tab project directory")
  helper.assert_equal(vim.t.wisp_project_name, project.name, "tab project name")
  helper.assert_equal(state.closed_window.window, 13, "float closed")
  helper.assert_equal(state.deleted_buffer.buffer, 11, "terminal buffer deleted")
end)

helper.test("Neovim cancellation closes the float without changing the tab", function()
  local vim, state = fake_vim { protocol_version = 2, status = "cancelled" }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.keymaps, 0, "default keymaps")
  helper.assert_equal(#state.commands, 0, "cancel commands")
  helper.assert_equal(#state.tab_calls, 0, "cancel tab calls")
  helper.assert_equal(state.closed_window.window, 13, "cancel float closed")
end)

helper.test("Neovim rejects unsupported result protocols", function()
  local vim, state = fake_vim { protocol_version = 1, status = "cancelled" }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(state.notifications[#state.notifications].level, vim.log.levels.ERROR, "protocol notification")
  assert(state.notifications[#state.notifications].message:match "protocol", "protocol notification message")
end)

helper.test("Neovim requires every project protocol field", function()
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "selected",
    selection = {
      kind = "file",
      project = { path = project.path, name = project.name },
      path = "/Users/test/Repos/api with spaces/README.md",
    },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.tab_calls, 0, "incomplete project tab calls")
  assert(state.notifications[#state.notifications].message:match "valid selection", "incomplete project message")
end)

helper.test("Neovim rejects unknown project protocol fields", function()
  local project_with_extra = {
    id = project.id,
    path = project.path,
    group = project.group,
    name = project.name,
    display_name = project.display_name,
    future_field = true,
  }
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "selected",
    selection = { kind = "project", project = project_with_extra },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.tab_calls, 0, "unknown project field tab calls")
  assert(state.notifications[#state.notifications].message:match "valid selection", "unknown project field message")
end)

helper.test("Neovim rejects unknown result envelope fields", function()
  local vim, state = fake_vim { protocol_version = 2, status = "cancelled", future_field = true }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.notifications, 1, "unknown envelope notification count")
  assert(state.notifications[1].message:match "valid result", "unknown envelope message")
end)

helper.test("Neovim rejects fields inconsistent with result status", function()
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "cancelled",
    selection = { kind = "project", project = project },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.notifications, 1, "inconsistent result notification count")
  assert(state.notifications[1].message:match "valid result", "inconsistent result message")
end)

helper.test("Neovim rejects unknown selection fields", function()
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "selected",
    selection = { kind = "project", project = project, future_field = true },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.tab_calls, 0, "unknown selection field tab calls")
  assert(state.notifications[#state.notifications].message:match "valid selection", "unknown selection field message")
end)

helper.test("Neovim rejects malformed opener fields", function()
  local vim, state = fake_vim {
    protocol_version = 2,
    status = "selected",
    selection = { kind = "project", project = project, opener = "nvim" },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.tab_calls, 0, "malformed opener tab calls")
  assert(state.notifications[#state.notifications].message:match "valid selection", "malformed opener message")
end)
