package.path = "./?.lua;./?/init.lua;./nvim/lua/?.lua;./nvim/lua/?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local project = {
  id = "api",
  path = "/Users/test/Repos/api",
  group = "Repos",
  name = "api",
  display_name = "API",
}

local function preview_view(path)
  return {
    window_id = "1001",
    path = path,
    active = true,
    width = 120,
    height = 40,
    bottomline = 8,
    view = {
      lnum = 6,
      col = 3,
      coladd = 0,
      curswant = 3,
      topline = 3,
      topfill = 0,
      leftcol = 2,
      skipcol = 0,
    },
  }
end

local function fake_vim()
  local state = {
    buffers = { [41] = true },
    decoded = {},
    directories = {},
    lines = {},
    options = {},
    timers = {},
    windows = { [31] = true },
  }
  local vim = {
    api = {},
    env = {},
    filetype = {},
    fn = {},
    json = {},
    uv = {},
  }

  function vim.api.nvim_buf_is_valid(buffer)
    return state.buffers[buffer] == true
  end

  function vim.api.nvim_win_is_valid(window)
    return state.windows[window] == true
  end

  function vim.api.nvim_set_option_value(name, value, options)
    table.insert(state.options, { name = name, value = value, options = options })
  end

  function vim.api.nvim_buf_set_lines(buffer, first, last, strict, lines)
    state.lines[buffer] = lines
  end

  function vim.api.nvim_buf_line_count(buffer)
    return #(state.lines[buffer] or {})
  end

  function vim.api.nvim_win_call(window, callback)
    state.called_window = window
    callback()
  end

  function vim.fn.isdirectory(path)
    return state.directories[path] and 1 or 0
  end

  function vim.fn.winrestview(view)
    state.restored_view = view
  end

  function vim.filetype.match(options)
    state.filetype_path = options.filename
    return options.filename:match "%.rs$" and "rust" or nil
  end

  function vim.json.decode(encoded)
    local decoded = state.decoded[encoded]
    if decoded == nil then
      error "invalid JSON"
    end
    return decoded
  end

  function vim.schedule(callback)
    callback()
  end

  function vim.schedule_wrap(callback)
    return callback
  end

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
    table.insert(state.timers, timer)
    return timer
  end

  return vim, state
end

local function load_preview(vim)
  _G.vim = vim
  return assert(loadfile "nvim/lua/wisp/file_preview.lua")()
end

local function option_value(state, name)
  for index = #state.options, 1, -1 do
    if state.options[index].name == name then
      return state.options[index].value
    end
  end
end

local function write_temporary(contents)
  local path = os.tmpname()
  local file = assert(io.open(path, "wb"))
  assert(file:write(contents))
  assert(file:close())
  return path
end

helper.test("Neovim preview strictly decodes only newer protocol v7 envelopes", function()
  local vim, state = fake_vim()
  local preview = load_preview(vim)
  local valid = [[{"protocol_version":7,"sequence":2,"state":{"state":"empty"}}]]
  state.decoded[valid] = { protocol_version = 7, sequence = 2, state = { state = "empty" } }

  helper.assert_equal(preview.decode(valid, 1).sequence, 2, "valid sequence")
  helper.assert_equal(preview.decode(valid, 2), nil, "stale sequence")

  local duplicate = [[{"protocol_version":7,"sequence":2,"sequence":3,"state":{"state":"empty"}}]]
  state.decoded[duplicate] = { protocol_version = 7, sequence = 3, state = { state = "empty" } }
  helper.assert_equal(preview.decode(duplicate, -1), nil, "duplicate field")

  local missing_active = preview_view "/Users/test/Repos/api/src/main.rs"
  missing_active.active = nil
  state.decoded.WRONG_VERSION = { protocol_version = 6, sequence = 3, state = { state = "empty" } }
  state.decoded.UNKNOWN_FIELD = { protocol_version = 7, sequence = 3, state = { state = "empty" }, future = true }
  state.decoded.INVALID_STATE = { protocol_version = 7, sequence = 3, state = { state = "future" } }
  state.decoded.INVALID_VIEW = {
    protocol_version = 7,
    sequence = 3,
    state = {
      state = "file",
      project = project,
      path = "/Users/test/Repos/api/src/main.rs",
      nvim_view = missing_active,
    },
  }

  for _, encoded in ipairs { "WRONG_VERSION", "UNKNOWN_FIELD", "INVALID_STATE", "INVALID_VIEW" } do
    helper.assert_equal(preview.decode(encoded, -1), nil, encoded)
  end
end)

helper.test("Neovim preview renders a themed scratch buffer and restores a captured viewport", function()
  local path = write_temporary "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n"
  local vim, state = fake_vim()
  local preview = load_preview(vim)
  local renderer = preview.new(31, 41)

  renderer:render { state = "file", project = project, path = path .. ".rs", nvim_view = preview_view(path) }

  helper.assert_table_equal(state.lines[41], { "[Wisp preview: file was deleted or is unreadable]" }, "missing path")

  local renamed = path .. ".rs"
  assert(os.rename(path, renamed))
  local view = preview_view(renamed)
  renderer:render { state = "file", project = project, path = renamed, nvim_view = view }

  helper.assert_table_equal(
    state.lines[41],
    { "one", "two", "three", "four", "five", "six", "seven", "eight" },
    "file lines"
  )
  helper.assert_equal(option_value(state, "buftype"), "nofile", "scratch buftype")
  helper.assert_equal(option_value(state, "buflisted"), false, "unlisted scratch")
  helper.assert_equal(option_value(state, "swapfile"), false, "scratch swap")
  helper.assert_equal(option_value(state, "modifiable"), false, "scratch modifiable")
  helper.assert_equal(option_value(state, "readonly"), true, "scratch readonly")
  helper.assert_equal(option_value(state, "filetype"), "rust", "detected filetype")
  helper.assert_table_equal(state.restored_view, view.view, "restored viewport")
  assert(option_value(state, "winbar"):match "3%-8", "source line range")
  assert(option_value(state, "winbar"):match "120x40", "source dimensions")
  os.remove(renamed)
end)

helper.test("Neovim preview reports binary directories unreadable files and both truncation limits", function()
  local vim, state = fake_vim()
  local preview = load_preview(vim)
  local renderer = preview.new(31, 41)

  local binary = write_temporary "text\0binary"
  renderer:render { state = "file", project = project, path = binary }
  assert(state.lines[41][1]:match "binary", "binary message")
  os.remove(binary)

  local directory = "/tmp/wisp-preview-directory"
  state.directories[directory] = true
  renderer:render { state = "file", project = project, path = directory }
  assert(state.lines[41][1]:match "directory", "directory message")

  renderer:render { state = "file", project = project, path = "/tmp/wisp-preview-missing" }
  assert(state.lines[41][1]:match "deleted or is unreadable", "unreadable message")

  local oversized = write_temporary(string.rep("x", 1024 * 1024 + 32))
  renderer:render { state = "file", project = project, path = oversized }
  assert(state.lines[41][#state.lines[41]]:match "1 MiB", "byte truncation message")
  assert(#state.lines[41][1] <= 1024 * 1024, "bounded byte content")
  os.remove(oversized)

  local many_lines = write_temporary(string.rep("line\n", 10001))
  renderer:render { state = "file", project = project, path = many_lines }
  helper.assert_equal(#state.lines[41], 10001, "line limit plus message")
  assert(state.lines[41][10001]:match "10,000 lines", "line truncation message")
  os.remove(many_lines)
end)

helper.test("Neovim preview sidecar watcher emits newer states and stops cleanly", function()
  local path = write_temporary [[{"protocol_version":7,"sequence":1,"state":{"state":"hidden"}}]]
  local vim, state = fake_vim()
  local preview = load_preview(vim)
  local encoded = assert(io.open(path, "rb")):read "*a"
  state.decoded[encoded] = { protocol_version = 7, sequence = 1, state = { state = "hidden" } }
  local observed = {}

  local watcher = preview.watch(path, function(value)
    table.insert(observed, value.sequence)
  end)
  state.timers[1].callback()
  state.timers[1].callback()

  helper.assert_table_equal(observed, { 1 }, "new sidecar sequences")
  watcher:stop()
  helper.assert_equal(state.timers[1].stopped, true, "timer stopped")
  helper.assert_equal(state.timers[1].closed, true, "timer closed")
  os.remove(path)
end)
