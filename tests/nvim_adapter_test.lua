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

local function has_argument(args, expected)
  for _, value in ipairs(args) do
    if value == expected then
      return true
    end
  end
  return false
end

local function fake_vim(result)
  local state = {
    autocmds = {},
    augroups = {},
    buffers = {},
    buffer_names = { [5] = "", [6] = "", [11] = "" },
    buffer_types = { [5] = "", [6] = "", [11] = "terminal" },
    buffer_lines = {},
    commands = {},
    current_buffer = 5,
    current_tab = 7,
    current_window = 21,
    deferred = {},
    decoded_json = {},
    deleted_buffers = {},
    encoded_json = {},
    focused_tabs = {},
    focused_windows = {},
    keymaps = {},
    notifications = {},
    tab_calls = {},
    tab_active_windows = { [7] = 21 },
    tab_windows = { [7] = { 21 } },
    user_commands = {},
    uis = { { chan = 1 } },
    valid_windows = { [21] = true },
    valid_buffers = { [5] = true, [6] = true },
    window_bottomlines = { [21] = 1 },
    window_buffers = { [21] = 5 },
    window_heights = { [21] = 30 },
    window_views = {
      [21] = {
        lnum = 1,
        col = 0,
        coladd = 0,
        curswant = 0,
        topline = 1,
        topfill = 0,
        leftcol = 0,
        skipcol = 0,
      },
    },
    window_widths = { [21] = 80 },
    windows = {},
  }
  local temporary = os.tmpname()
  os.remove(temporary)
  local vim = {
    api = {},
    base64 = {},
    cmd = {},
    env = {},
    filetype = {},
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

  function vim.api.nvim_create_augroup(name, options)
    table.insert(state.augroups, { name = name, options = options })
    return 19
  end

  function vim.api.nvim_create_autocmd(events, options)
    table.insert(state.autocmds, { events = events, options = options })
    return #state.autocmds
  end

  function vim.api.nvim_get_current_tabpage()
    return state.current_tab
  end

  function vim.api.nvim_get_current_buf()
    return state.current_buffer
  end

  function vim.api.nvim_get_current_win()
    return state.current_window
  end

  function vim.api.nvim_list_tabpages()
    local tabs = {}
    for tab in pairs(state.tab_windows) do
      table.insert(tabs, tab)
    end
    table.sort(tabs)
    return tabs
  end

  function vim.api.nvim_tabpage_list_wins(tab)
    return state.tab_windows[tab] or {}
  end

  function vim.api.nvim_tabpage_get_win(tab)
    return state.tab_active_windows[tab] or (state.tab_windows[tab] or {})[1]
  end

  function vim.api.nvim_list_uis()
    return state.uis
  end

  function vim.api.nvim_buf_get_name(buffer)
    return state.buffer_names[buffer] or ""
  end

  function vim.api.nvim_get_option_value(name, options)
    if name == "buftype" then
      return state.buffer_types[options.buf] or ""
    end
    error("unexpected option " .. tostring(name))
  end

  function vim.api.nvim_create_buf()
    local buffer = 11
    while state.valid_buffers[buffer] do
      buffer = buffer + 1
    end
    state.valid_buffers[buffer] = true
    state.buffer_names[buffer] = ""
    state.buffer_types[buffer] = "terminal"
    return buffer
  end

  function vim.api.nvim_open_win(buffer, enter, config)
    table.insert(state.windows, { buffer = buffer, config = config, enter = enter })
    local window = 13
    while state.valid_windows[window] do
      window = window + 1
    end
    if enter then
      state.return_window = state.current_window
      state.current_buffer = buffer
      state.current_window = window
    end
    state.valid_windows[window] = true
    state.window_buffers[window] = buffer
    return window
  end

  function vim.api.nvim_win_is_valid(window)
    return state.valid_windows[window] == true
  end

  function vim.api.nvim_win_close(window, force)
    state.closed_window = { force = force, window = window }
    state.closed_windows = state.closed_windows or {}
    table.insert(state.closed_windows, window)
    state.valid_windows[window] = false
    if state.current_window == window and state.return_window then
      state.current_window = state.return_window
      state.current_buffer = state.window_buffers[state.return_window]
    end
  end

  function vim.api.nvim_win_get_buf(window)
    return state.window_buffers[window]
  end

  function vim.api.nvim_win_get_width(window)
    return state.window_widths[window]
  end

  function vim.api.nvim_win_get_height(window)
    return state.window_heights[window]
  end

  function vim.api.nvim_win_set_config(window, config)
    state.window_configs = state.window_configs or {}
    state.window_configs[window] = config
  end

  function vim.api.nvim_win_get_tabpage(window)
    for tab, windows in pairs(state.tab_windows) do
      for _, candidate in ipairs(windows) do
        if candidate == window then
          return tab
        end
      end
    end
    error "window has no tab"
  end

  function vim.api.nvim_set_current_tabpage(tab)
    assert(state.tab_windows[tab], "invalid tab")
    state.current_tab = tab
    state.current_window = state.tab_active_windows[tab] or state.tab_windows[tab][1]
    state.current_buffer = state.window_buffers[state.current_window]
    table.insert(state.focused_tabs, tab)
  end

  function vim.api.nvim_set_current_win(window)
    assert(state.valid_windows[window], "invalid window")
    state.current_window = window
    state.current_tab = vim.api.nvim_win_get_tabpage(window)
    state.current_buffer = state.window_buffers[window]
    state.tab_active_windows[state.current_tab] = window
    table.insert(state.focused_windows, window)
  end

  function vim.api.nvim_win_call(window, callback)
    local previous_window = state.current_window
    local previous_buffer = state.current_buffer
    state.current_window = window
    state.current_buffer = state.window_buffers[window]
    local values = { callback() }
    state.current_window = previous_window
    state.current_buffer = previous_buffer
    return table.unpack(values)
  end

  function vim.api.nvim_buf_is_valid(buffer)
    return state.valid_buffers[buffer] == true
  end

  function vim.api.nvim_buf_delete(buffer, options)
    state.deleted_buffer = { buffer = buffer, options = options }
    state.valid_buffers[buffer] = false
    table.insert(state.deleted_buffers, buffer)
  end

  function vim.api.nvim_buf_set_lines(buffer, first, last, strict, lines)
    state.buffer_lines[buffer] = lines
  end

  function vim.api.nvim_buf_line_count(buffer)
    return #(state.buffer_lines[buffer] or {})
  end

  function vim.api.nvim_tabpage_is_valid(tab)
    return state.tab_windows[tab] ~= nil
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
    state.tempname_count = (state.tempname_count or 0) + 1
    if state.tempname_count == 1 then
      return temporary
    end
    local path = os.tmpname()
    os.remove(path)
    return path
  end

  function vim.fn.delete(path)
    os.remove(path)
  end

  function vim.fn.fnamemodify(path, modifier)
    assert(modifier == ":p")
    return path
  end

  function vim.fn.has(feature)
    if feature == "win32" or feature == "win64" then
      return state.is_windows and 1 or 0
    end
    return 0
  end

  function vim.fn.isdirectory()
    return 0
  end

  function vim.fn.line(expression)
    assert(expression == "w$")
    return state.window_bottomlines[state.current_window]
  end

  function vim.fn.winsaveview()
    return state.window_views[state.current_window]
  end

  function vim.fn.winrestview(view)
    state.restored_view = view
  end

  function vim.fn.jobstart(args, options)
    state.job = { args = args, options = options }
    local result_path = assert(argument_after(args, "--result-file"))
    local file = assert(io.open(result_path, "wb"))
    file:write(state.result_text or "RESULT")
    file:close()
    if state.before_job_exit then
      state.before_job_exit()
    end
    if not state.defer_job_exit then
      options.on_exit(42, 0)
    end
    return 42
  end

  function vim.json.decode(encoded)
    if encoded == "RESULT" then
      return result
    end
    local decoded = state.decoded_json[encoded]
    if decoded == nil then
      error "invalid JSON"
    end
    return decoded
  end

  function vim.json.encode(value)
    table.insert(state.encoded_json, value)
    return "JSON" .. tostring(#state.encoded_json)
  end

  function vim.base64.encode(value)
    return value == "" and "" or "base64:" .. value
  end

  function vim.schedule(callback)
    callback()
  end

  function vim.schedule_wrap(callback)
    return callback
  end

  function vim.defer_fn(callback, delay)
    table.insert(state.deferred, { callback = callback, delay = delay })
  end

  function vim.notify(message, level)
    table.insert(state.notifications, { level = level, message = message })
  end

  function vim.filetype.match()
    return nil
  end

  vim.uv = {}
  function vim.uv.new_timer()
    local timer = { active = false }
    function timer:start(timeout, repeat_interval, callback)
      self.active = true
      self.timeout = timeout
      self.repeat_interval = repeat_interval
      self.callback = callback
    end
    function timer:stop()
      self.active = false
      self.stopped = true
    end
    function timer:close()
      self.closed = true
    end
    state.timers = state.timers or {}
    table.insert(state.timers, timer)
    return timer
  end

  return vim, state
end

local function load_adapter(vim)
  _G.vim = vim
  return assert(loadfile "nvim/lua/wisp/init.lua")(
    "/opt/bin/wisp",
    "wisp-deployment-v7",
    "nvim/lua/wisp/file_preview.lua"
  )
end

local function capture_stdout(callback)
  local original = io.stdout
  local chunks = {}
  io.stdout = {
    flush = function() end,
    write = function(_, value)
      table.insert(chunks, value)
    end,
  }
  local completed, callback_error = pcall(callback)
  io.stdout = original
  if not completed then
    error(callback_error)
  end
  return table.concat(chunks)
end

local function autocmd_for(state, event)
  for _, autocmd in ipairs(state.autocmds) do
    if autocmd.events == event then
      return autocmd
    end
    if type(autocmd.events) == "table" then
      for _, candidate in ipairs(autocmd.events) do
        if candidate == event then
          return autocmd
        end
      end
    end
  end
  error("missing autocmd for " .. event)
end

helper.test("Neovim adapter rejects ordinary runtimepath loading", function()
  local vim = fake_vim { protocol_version = 7, status = "cancelled" }
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

helper.test("Neovim adapter loads the preview module without auto-starting its external script", function()
  local vim = fake_vim { protocol_version = 7, status = "cancelled" }
  vim.env.WISP_FILE_PREVIEW_STATE_FILE = "/tmp/wisp-preview-state.json"

  local wisp = load_adapter(vim)

  helper.assert_equal(type(wisp.setup), "function", "adapter load")
end)

helper.test("Neovim setup registers a command, optional mapping, and inherited metadata", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
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

helper.test("Neovim file open and preview options are strict", function()
  local vim = fake_vim { protocol_version = 7, status = "cancelled" }
  local wisp = load_adapter(vim)

  wisp.setup {
    file_open = { default = "right_pane" },
    file_preview = { width = 0.4 },
  }

  for _, configured in ipairs {
    { file_open = "window" },
    { file_open = {} },
    { file_open = { default = "tab" } },
    { file_open = { default = "window", future = true } },
    { file_preview = true },
    { file_preview = {} },
    { file_preview = { width = 0 } },
    { file_preview = { width = 1.1 } },
    { file_preview = { width = 0.5, future = true } },
  } do
    local ok, err = pcall(wisp.setup, configured)
    assert(not ok, "invalid file options should fail")
    assert(tostring(err):match "file_", "file option error should be actionable")
  end
end)

helper.test("Neovim picker opens a default file result in a new tab", function()
  local vim, state = fake_vim {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = "/Users/test/Repos/api with spaces/README.md",
      opener = { "nvim", "/Users/test/Repos/api with spaces/README.md" },
      open_target = "window",
      reuse_existing = true,
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
    "--disable-sessions",
    "--file-open-target",
    "window",
    "--result-file",
    argument_after(state.job.args, "--result-file"),
  }, "picker argv")
  helper.assert_equal(state.windows[1].config.width, 80, "float width")
  helper.assert_equal(state.windows[1].config.height, 20, "float height")
  helper.assert_table_equal(state.focused_tabs, { 7 }, "activated originating tab")
  helper.assert_equal(state.commands[1].cmd, "tabnew", "file target command")
  helper.assert_equal(state.commands[1].args[1], "/Users/test/Repos/api with spaces/README.md", "file path")
  helper.assert_equal(state.commands[2].cmd, "tcd", "tab cwd command")
  helper.assert_equal(state.commands[2].args[1], project.path, "tab cwd path")
  helper.assert_equal(vim.t.wisp_project_dir, project.path, "tab project directory")
  helper.assert_equal(vim.t.wisp_project_name, project.name, "tab project name")
  helper.assert_equal(state.closed_window.window, 13, "float closed")
  helper.assert_equal(state.deleted_buffer.buffer, 11, "terminal buffer deleted")
end)

helper.test("Neovim picker maps pane targets to structured right and bottom splits", function()
  for _, expected in ipairs {
    { target = "right_pane", command = "vsplit" },
    { target = "bottom_pane", command = "split" },
  } do
    local vim, state = fake_vim {
      protocol_version = 7,
      status = "selected",
      selection = {
        kind = "file",
        project = project,
        path = "/Users/test/Repos/api with spaces/README.md",
        opener = { "nvim", "/Users/test/Repos/api with spaces/README.md" },
        open_target = expected.target,
        reuse_existing = false,
      },
    }
    local wisp = load_adapter(vim)
    wisp.setup()

    wisp.open()

    helper.assert_table_equal(state.focused_tabs, { 7 }, expected.target .. " originating tab")
    helper.assert_equal(state.commands[1].cmd, expected.command, expected.target .. " command")
    helper.assert_equal(state.commands[1].args[1], "/Users/test/Repos/api with spaces/README.md", "split path")
    helper.assert_equal(state.commands[1].mods.split, "belowright", expected.target .. " placement")
    helper.assert_equal(state.commands[2].cmd, "tcd", expected.target .. " cwd command")
  end
end)

helper.test("Neovim picker reuses the best visible normalized file window", function()
  local selected_path = "C:\\Users\\Test\\Repos\\Api\\src\\Main.rs"
  local vim, state = fake_vim {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = selected_path,
      opener = { "nvim", selected_path },
      open_target = "window",
      reuse_existing = true,
    },
  }
  state.is_windows = true
  state.buffer_names[5] = "C:\\Users\\Test\\other.rs"
  state.buffer_names[6] = "c:/users/test/repos/api/src/generated/../main.rs"
  state.tab_windows[7] = { 21, 22 }
  state.valid_windows[22] = true
  state.window_buffers[22] = 6
  state.window_widths[22] = 70
  state.window_heights[22] = 20
  state.window_bottomlines[22] = 1
  state.window_views[22] = state.window_views[21]
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(state.current_tab, 7, "reused tab")
  helper.assert_equal(state.current_window, 22, "reused window")
  helper.assert_table_equal(state.focused_windows, { 22 }, "focused match")
  helper.assert_equal(#state.commands, 1, "reuse command count")
  helper.assert_equal(state.commands[1].cmd, "tcd", "reuse project cwd")
end)

helper.test("Neovim forced file targets do not reuse a visible match", function()
  local vim, state = fake_vim {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = "/Users/test/Repos/api with spaces/README.md",
      opener = { "nvim", "/Users/test/Repos/api with spaces/README.md" },
      open_target = "right_pane",
      reuse_existing = false,
    },
  }
  state.buffer_names[5] = "/Users/test/Repos/api with spaces/README.md"
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_table_equal(state.focused_windows, { 21 }, "forced target stays in originating window")
  helper.assert_equal(state.commands[1].cmd, "vsplit", "forced split")
end)

helper.test("Neovim can reuse a visible file after the originating tab closes", function()
  local path = "/Users/test/Repos/api with spaces/README.md"
  local vim, state = fake_vim {
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = project,
      path = path,
      opener = { "nvim", path },
      open_target = "bottom_pane",
      reuse_existing = true,
    },
  }
  state.before_job_exit = function()
    state.tab_windows[7] = nil
    state.tab_active_windows[7] = nil
    state.valid_windows[21] = false
    state.tab_windows[8] = { 31 }
    state.tab_active_windows[8] = 31
    state.valid_windows[31] = true
    state.window_buffers[31] = 6
    state.buffer_names[6] = path
  end
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(state.current_tab, 8, "surviving matching tab")
  helper.assert_equal(state.current_window, 31, "surviving matching window")
  helper.assert_equal(state.notifications[1], nil, "reuse should avoid stale-origin error")
end)

helper.test("Neovim picker passes the originating normal file and project to Wisp", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
  state.buffer_names[5] = "/Users/test/Repos/api/src/main.rs"
  vim.t.wisp_project_dir = "/Users/test/Repos/api"
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(
    argument_after(state.job.args, "--active-project-path"),
    "/Users/test/Repos/api",
    "active project path"
  )
  helper.assert_equal(
    argument_after(state.job.args, "--active-file"),
    "/Users/test/Repos/api/src/main.rs",
    "active file"
  )
  helper.assert_equal(state.current_buffer, 5, "picker cleanup restores the originating buffer")
end)

helper.test("Neovim picker drives a companion preview float from the sidecar", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
  state.defer_job_exit = true
  local wisp = load_adapter(vim)
  wisp.setup { file_preview = { width = 0.5 } }

  wisp.open()

  local sidecar = assert(argument_after(state.job.args, "--file-preview-state-file"))
  helper.assert_equal(has_argument(state.job.args, "--file-preview"), true, "initial preview flag")
  helper.assert_equal(#state.timers, 1, "preview watcher")

  local empty = [[{"protocol_version":7,"sequence":1,"state":{"state":"empty"}}]]
  state.decoded_json[empty] = { protocol_version = 7, sequence = 1, state = { state = "empty" } }
  local file = assert(io.open(sidecar, "wb"))
  assert(file:write(empty))
  assert(file:close())
  state.timers[1].callback()

  helper.assert_equal(#state.windows, 2, "companion float count")
  assert(state.window_configs[13].width < 80, "picker should shrink inside its footprint")
  helper.assert_equal(state.windows[2].enter, false, "preview must not take focus")
  assert(state.windows[2].config.col > state.windows[1].config.col, "preview should be right of picker")
  helper.assert_equal(state.current_window, 13, "picker terminal focus")
  assert(state.buffer_lines[12][1]:match "select a file", "empty preview message")

  local hidden = [[{"protocol_version":7,"sequence":2,"state":{"state":"hidden"}}]]
  state.decoded_json[hidden] = { protocol_version = 7, sequence = 2, state = { state = "hidden" } }
  file = assert(io.open(sidecar, "wb"))
  assert(file:write(hidden))
  assert(file:close())
  state.timers[1].callback()

  helper.assert_equal(state.closed_windows[1], 14, "hidden preview window")
  helper.assert_equal(state.window_configs[13].width, 80, "restored picker width")
  state.job.options.on_exit(42, 0)
  helper.assert_equal(state.timers[1].stopped, true, "preview watcher stopped")
  helper.assert_equal(io.open(sidecar, "rb"), nil, "preview sidecar removed")
  helper.assert_table_equal(state.deleted_buffers, { 12, 11 }, "preview and picker buffers deleted")
end)

helper.test("Neovim publishes every normal file view in the current tab", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
  vim.env.WEZTERM_PANE = "9"
  state.buffer_names[5] = "/Users/test/Repos/api/src/main.rs"
  state.buffer_names[6] = "/Users/test/Repos/api/src/lib.rs"
  state.tab_windows[7] = { 21, 22 }
  state.valid_windows[22] = true
  state.window_buffers[22] = 6
  state.window_widths[21] = 120
  state.window_widths[22] = 75
  state.window_heights[21] = 40
  state.window_heights[22] = 24
  state.window_bottomlines[21] = 157
  state.window_bottomlines[22] = 52
  state.window_views[21] = {
    lnum = 132,
    col = 8,
    coladd = 0,
    curswant = 8,
    topline = 118,
    topfill = 0,
    leftcol = 3,
    skipcol = 0,
  }
  state.window_views[22] = {
    lnum = 44,
    col = 2,
    coladd = 1,
    curswant = 7,
    topline = 29,
    topfill = 2,
    leftcol = 0,
    skipcol = 4,
  }
  local wisp = load_adapter(vim)

  local output = capture_stdout(function()
    wisp.setup()
    autocmd_for(state, "BufEnter").options.callback()
    state.buffer_types[5] = "terminal"
    autocmd_for(state, "WinEnter").options.callback()
    state.buffer_types[6] = "quickfix"
    autocmd_for(state, "BufWinEnter").options.callback()
    autocmd_for(state, "VimLeavePre").options.callback()
  end)

  helper.assert_table_equal(state.encoded_json[1], {
    protocol_version = 7,
    views = {
      {
        window_id = "21",
        path = "/Users/test/Repos/api/src/main.rs",
        active = true,
        width = 120,
        height = 40,
        bottomline = 157,
        view = state.window_views[21],
      },
      {
        window_id = "22",
        path = "/Users/test/Repos/api/src/lib.rs",
        active = false,
        width = 75,
        height = 24,
        bottomline = 52,
        view = state.window_views[22],
      },
    },
  }, "pane state envelope")
  helper.assert_equal(#state.encoded_json[2].views, 2, "unchanged file event view count")
  helper.assert_equal(#state.encoded_json[3].views, 1, "remaining file view count")
  helper.assert_equal(
    output,
    "\27]1337;SetUserVar=WISP_NVIM_STATE=base64:JSON1\27\\"
      .. "\27]1337;SetUserVar=WISP_NVIM_STATE=base64:JSON2\27\\"
      .. "\27]1337;SetUserVar=WISP_NVIM_STATE=base64:JSON3\27\\"
      .. "\27]1337;SetUserVar=WISP_NVIM_STATE=\27\\"
      .. "\27]1337;SetUserVar=WISP_NVIM_STATE=\27\\",
    "pane user variable output"
  )
end)

helper.test("Neovim debounces viewport pane-state refreshes to the newest event", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
  vim.env.WEZTERM_PANE = "9"
  state.buffer_names[5] = "/Users/test/Repos/api/src/main.rs"
  local wisp = load_adapter(vim)

  capture_stdout(function()
    wisp.setup()
    local viewport = autocmd_for(state, "CursorMoved")
    viewport.options.callback()
    state.window_views[21].lnum = 12
    viewport.options.callback()
    helper.assert_equal(#state.encoded_json, 1, "no eager viewport publish")
    helper.assert_equal(#state.deferred, 2, "debounce callback count")
    state.deferred[1].callback()
    state.deferred[2].callback()
  end)

  helper.assert_equal(#state.encoded_json, 2, "one debounced viewport publish")
  helper.assert_equal(state.encoded_json[2].views[1].view.lnum, 12, "newest viewport")
end)

helper.test("Neovim defers pane escapes until a UI is attached", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
  vim.env.WEZTERM_PANE = "9"
  state.buffer_names[5] = "/Users/test/Repos/api/src/main.rs"
  state.uis = {}
  local wisp = load_adapter(vim)

  local output = capture_stdout(function()
    wisp.setup()
    helper.assert_equal(#state.autocmds, 1, "deferred pane autocmd count")
    helper.assert_equal(state.autocmds[1].events, "UIEnter", "deferred pane event")
    state.uis = { { chan = 1 } }
    state.autocmds[1].options.callback()
  end)

  helper.assert_equal(output, "\27]1337;SetUserVar=WISP_NVIM_STATE=base64:JSON1\27\\", "deferred pane output")
  helper.assert_equal(#state.autocmds, 4, "attached pane autocmd count")
end)

helper.test("Neovim cancellation closes the float without changing the tab", function()
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled" }
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
    protocol_version = 7,
    status = "selected",
    selection = {
      kind = "file",
      project = { path = project.path, name = project.name },
      path = "/Users/test/Repos/api with spaces/README.md",
      open_target = "window",
      reuse_existing = true,
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
    protocol_version = 7,
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
  local vim, state = fake_vim { protocol_version = 7, status = "cancelled", future_field = true }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.notifications, 1, "unknown envelope notification count")
  assert(state.notifications[1].message:match "valid result", "unknown envelope message")
end)

helper.test("Neovim rejects duplicate raw result fields", function()
  local duplicate = [[{"protocol_version":7,"status":"cancelled","status":"selected"}]]
  local vim, state = fake_vim { protocol_version = 7, status = "selected" }
  state.result_text = duplicate
  state.decoded_json[duplicate] = { protocol_version = 7, status = "selected" }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.commands, 0, "duplicate result commands")
  assert(state.notifications[1].message:match "invalid JSON", "duplicate result message")
end)

helper.test("Neovim rejects fields inconsistent with result status", function()
  local vim, state = fake_vim {
    protocol_version = 7,
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
    protocol_version = 7,
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
    protocol_version = 7,
    status = "selected",
    selection = { kind = "project", project = project, opener = "nvim" },
  }
  local wisp = load_adapter(vim)
  wisp.setup()

  wisp.open()

  helper.assert_equal(#state.tab_calls, 0, "malformed opener tab calls")
  assert(state.notifications[#state.notifications].message:match "valid selection", "malformed opener message")
end)
