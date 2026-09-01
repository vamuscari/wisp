local load_mode = ...
local M = {}
local PROTOCOL_VERSION = 7
local BYTE_LIMIT = 1024 * 1024
local LINE_LIMIT = 10000

local function has_only_fields(value, allowed)
  if type(value) ~= "table" then
    return false
  end
  for field in pairs(value) do
    if not allowed[field] then
      return false
    end
  end
  return true
end

local function is_integer(value, minimum)
  return type(value) == "number" and value % 1 == 0 and value >= minimum
end

local function is_absolute_path(path)
  return type(path) == "string"
    and path ~= ""
    and (path:sub(1, 1) == "/" or path:match "^%a:[/\\]" ~= nil or path:sub(1, 2) == "\\\\")
end

local function reject_duplicate_fields(encoded)
  local length = #encoded
  local function skip_space(index)
    while index <= length and encoded:sub(index, index):match "%s" do
      index = index + 1
    end
    return index
  end

  local function parse_string(index)
    if encoded:sub(index, index) ~= '"' then
      return nil
    end
    local start = index
    index = index + 1
    while index <= length do
      local character = encoded:sub(index, index)
      if character == '"' then
        return index + 1, encoded:sub(start, index)
      end
      if character == "\\" then
        index = index + 1
        if index > length then
          return nil
        end
        if encoded:sub(index, index) == "u" then
          if not encoded:sub(index + 1, index + 4):match "^%x%x%x%x$" then
            return nil
          end
          index = index + 4
        end
      end
      index = index + 1
    end
    return nil
  end

  local parse_value
  parse_value = function(index)
    index = skip_space(index)
    local character = encoded:sub(index, index)
    if character == '"' then
      return parse_string(index)
    end
    if character == "{" then
      local seen = {}
      index = skip_space(index + 1)
      if encoded:sub(index, index) == "}" then
        return index + 1
      end
      while index <= length do
        local next_index, raw_key = parse_string(index)
        if not next_index then
          return nil
        end
        local decoded, key = pcall(vim.json.decode, raw_key)
        key = decoded and key or raw_key
        if seen[key] then
          return nil
        end
        seen[key] = true
        index = skip_space(next_index)
        if encoded:sub(index, index) ~= ":" then
          return nil
        end
        index = parse_value(index + 1)
        if not index then
          return nil
        end
        index = skip_space(index)
        local delimiter = encoded:sub(index, index)
        if delimiter == "}" then
          return index + 1
        end
        if delimiter ~= "," then
          return nil
        end
        index = skip_space(index + 1)
      end
      return nil
    end
    if character == "[" then
      index = skip_space(index + 1)
      if encoded:sub(index, index) == "]" then
        return index + 1
      end
      while index <= length do
        index = parse_value(index)
        if not index then
          return nil
        end
        index = skip_space(index)
        local delimiter = encoded:sub(index, index)
        if delimiter == "]" then
          return index + 1
        end
        if delimiter ~= "," then
          return nil
        end
        index = skip_space(index + 1)
      end
      return nil
    end
    local start = index
    while index <= length and not encoded:sub(index, index):match "[%s,%]}]" do
      index = index + 1
    end
    return index > start and index or nil
  end

  local final = parse_value(1)
  return final ~= nil and skip_space(final) == length + 1
end

local function valid_project(value)
  return type(value) == "table"
    and has_only_fields(value, { id = true, path = true, group = true, name = true, display_name = true })
    and type(value.id) == "string"
    and value.id ~= ""
    and type(value.path) == "string"
    and value.path ~= ""
    and type(value.group) == "string"
    and type(value.name) == "string"
    and type(value.display_name) == "string"
end

local VIEW_FIELDS = {
  window_id = true,
  path = true,
  active = true,
  width = true,
  height = true,
  bottomline = true,
  view = true,
}

local VIEWPORT_FIELDS = {
  lnum = true,
  col = true,
  coladd = true,
  curswant = true,
  topline = true,
  topfill = true,
  leftcol = true,
  skipcol = true,
}

local function valid_nvim_view(value)
  if type(value) ~= "table" or not has_only_fields(value, VIEW_FIELDS) then
    return false
  end
  if type(value.window_id) ~= "string" or value.window_id == "" or not is_absolute_path(value.path) then
    return false
  end
  if type(value.active) ~= "boolean" then
    return false
  end
  if not is_integer(value.width, 1) or not is_integer(value.height, 1) or not is_integer(value.bottomline, 1) then
    return false
  end
  if type(value.view) ~= "table" or not has_only_fields(value.view, VIEWPORT_FIELDS) then
    return false
  end
  for field in pairs(VIEWPORT_FIELDS) do
    local minimum = (field == "lnum" or field == "topline") and 1 or 0
    if not is_integer(value.view[field], minimum) then
      return false
    end
  end
  return value.bottomline >= value.view.topline
end

local function valid_state(state)
  if type(state) ~= "table" then
    return false
  end
  if state.state == "hidden" or state.state == "empty" then
    return has_only_fields(state, { state = true })
  end
  if state.state ~= "file" then
    return false
  end
  return has_only_fields(state, { state = true, project = true, path = true, nvim_view = true })
    and valid_project(state.project)
    and is_absolute_path(state.path)
    and (state.nvim_view == nil or valid_nvim_view(state.nvim_view))
end

function M.decode_json(encoded)
  if type(encoded) ~= "string" or encoded == "" or not reject_duplicate_fields(encoded) then
    return nil
  end
  local decoded, value = pcall(vim.json.decode, encoded)
  return decoded and value or nil
end

function M.decode(encoded, last_sequence)
  local envelope = M.decode_json(encoded)
  if
    type(envelope) ~= "table"
    or not has_only_fields(envelope, { protocol_version = true, sequence = true, state = true })
    or envelope.protocol_version ~= PROTOCOL_VERSION
    or not is_integer(envelope.sequence, 0)
    or envelope.sequence <= (last_sequence or -1)
    or not valid_state(envelope.state)
  then
    return nil
  end
  return envelope
end

local Renderer = {}
Renderer.__index = Renderer

local function set_buffer_option(buffer, name, value)
  vim.api.nvim_set_option_value(name, value, { buf = buffer })
end

local function set_window_option(window, name, value)
  vim.api.nvim_set_option_value(name, value, { win = window })
end

local function display_lines(renderer, lines)
  set_buffer_option(renderer.buffer, "readonly", false)
  set_buffer_option(renderer.buffer, "modifiable", true)
  vim.api.nvim_buf_set_lines(renderer.buffer, 0, -1, false, lines)
  set_buffer_option(renderer.buffer, "modified", false)
  set_buffer_option(renderer.buffer, "modifiable", false)
  set_buffer_option(renderer.buffer, "readonly", true)
end

local function read_file(path)
  if vim.fn.isdirectory(path) == 1 then
    return nil, "[Wisp preview: selected path is a directory]"
  end
  local file = io.open(path, "rb")
  if not file then
    return nil, "[Wisp preview: file was deleted or is unreadable]"
  end
  local contents = file:read(BYTE_LIMIT + 1) or ""
  file:close()
  local byte_truncated = #contents > BYTE_LIMIT
  if byte_truncated then
    contents = contents:sub(1, BYTE_LIMIT)
  end
  if contents:find("\0", 1, true) then
    return nil, "[Wisp preview: binary file contains NUL bytes]"
  end

  local lines = {}
  local position = 1
  local line_truncated = false
  while position <= #contents and #lines < LINE_LIMIT do
    local newline = contents:find("\n", position, true)
    local line
    if newline then
      line = contents:sub(position, newline - 1)
      position = newline + 1
    else
      line = contents:sub(position)
      position = #contents + 1
    end
    if line:sub(-1) == "\r" then
      line = line:sub(1, -2)
    end
    table.insert(lines, line)
  end
  if #lines == LINE_LIMIT and position <= #contents then
    line_truncated = true
  end
  if #lines == 0 then
    lines = { "" }
  end
  if byte_truncated or line_truncated then
    local limits = {}
    if byte_truncated then
      table.insert(limits, "1 MiB")
    end
    if line_truncated then
      table.insert(limits, "10,000 lines")
    end
    table.insert(lines, "[Wisp preview truncated at " .. table.concat(limits, " and ") .. "]")
  end
  return lines, nil, #lines - ((byte_truncated or line_truncated) and 1 or 0)
end

function M.new(window, buffer)
  local self = setmetatable({ window = window, buffer = buffer }, Renderer)
  set_buffer_option(buffer, "buftype", "nofile")
  set_buffer_option(buffer, "bufhidden", "wipe")
  set_buffer_option(buffer, "buflisted", false)
  set_buffer_option(buffer, "swapfile", false)
  set_buffer_option(buffer, "undofile", false)
  set_buffer_option(buffer, "undolevels", -1)
  set_buffer_option(buffer, "modeline", false)
  return self
end

function Renderer:render(state)
  if not vim.api.nvim_win_is_valid(self.window) or not vim.api.nvim_buf_is_valid(self.buffer) then
    return false
  end
  if state.state ~= "file" then
    local message = state.state == "empty" and "[Wisp preview: select a file]" or "[Wisp preview hidden]"
    display_lines(self, { message })
    set_buffer_option(self.buffer, "filetype", "")
    set_window_option(self.window, "winbar", " Wisp preview ")
    return true
  end

  local lines, read_error, file_line_count = read_file(state.path)
  if not lines then
    display_lines(self, { read_error })
    set_buffer_option(self.buffer, "filetype", "")
    set_window_option(self.window, "winbar", " Wisp preview ")
    return true
  end
  display_lines(self, lines)
  local detected, filetype = pcall(vim.filetype.match, { filename = state.path, buf = self.buffer })
  set_buffer_option(self.buffer, "filetype", detected and filetype or "")

  local source = state.nvim_view
  local source_view = source and source.view or nil
  local restorable = source_view
    and source_view.lnum <= file_line_count
    and source_view.topline <= file_line_count
    and source.bottomline <= file_line_count
  if restorable then
    set_window_option(
      self.window,
      "winbar",
      string.format(
        " Wisp source lines %d-%d | %dx%d ",
        source_view.topline,
        source.bottomline,
        source.width,
        source.height
      )
    )
    vim.api.nvim_win_call(self.window, function()
      vim.fn.winrestview {
        lnum = source_view.lnum,
        col = source_view.col,
        coladd = source_view.coladd,
        curswant = source_view.curswant,
        topline = source_view.topline,
        topfill = source_view.topfill,
        leftcol = source_view.leftcol,
        skipcol = source_view.skipcol,
      }
    end)
  else
    set_window_option(self.window, "winbar", " Wisp preview ")
    vim.api.nvim_win_call(self.window, function()
      vim.fn.winrestview { lnum = 1, col = 0, topline = 1, leftcol = 0 }
    end)
  end
  return true
end

local Watcher = {}
Watcher.__index = Watcher

function Watcher:stop()
  if self.stopped then
    return
  end
  self.stopped = true
  self.timer:stop()
  self.timer:close()
end

function M.watch(path, callback, on_error)
  local uv = vim.uv or vim.loop
  local watcher = setmetatable({ last_sequence = -1, stopped = false, timer = assert(uv.new_timer()) }, Watcher)
  local function poll()
    if watcher.stopped then
      return
    end
    local file = io.open(path, "rb")
    if not file then
      return
    end
    local encoded = file:read "*a"
    file:close()
    local envelope = M.decode(encoded, watcher.last_sequence)
    if not envelope then
      return
    end
    watcher.last_sequence = envelope.sequence
    local rendered, render_error = pcall(callback, envelope)
    if not rendered and on_error then
      on_error(render_error)
    end
  end
  watcher.timer:start(0, 50, vim.schedule_wrap(poll))
  return watcher
end

function M.start(path)
  local buffer = vim.api.nvim_create_buf(false, true)
  local window = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_buf(window, buffer)
  local renderer = M.new(window, buffer)
  local watcher = M.watch(path, function(envelope)
    renderer:render(envelope.state)
  end)
  local group = vim.api.nvim_create_augroup("WispFilePreview", { clear = true })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    once = true,
    callback = function()
      watcher:stop()
    end,
  })
  return watcher
end

if
  load_mode ~= "module"
  and type(vim.env.WISP_FILE_PREVIEW_STATE_FILE) == "string"
  and vim.env.WISP_FILE_PREVIEW_STATE_FILE ~= ""
then
  M.start(vim.env.WISP_FILE_PREVIEW_STATE_FILE)
end

return M
