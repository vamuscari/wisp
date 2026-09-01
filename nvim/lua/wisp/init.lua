local M = {}
local deployed_wisp_path, deployment_token, preview_module_path = ...
local WISP_VERSION = 7

if
  type(deployed_wisp_path) ~= "string"
  or deployed_wisp_path == ""
  or deployment_token ~= "wisp-deployment-v" .. WISP_VERSION
then
  error "Wisp's Neovim adapter must be loaded by the deployed runtime"
end
if type(preview_module_path) ~= "string" or preview_module_path == "" then
  error "Wisp's Neovim adapter requires its deployed preview module"
end

local FilePreview = assert(loadfile(preview_module_path)) "module"

local options = {}
local PROTOCOL_VERSION = WISP_VERSION
local WEZTERM_STATE_VAR = "WISP_NVIM_STATE"
local viewport_generation = 0

local function configure(configured)
  configured = configured or {}
  if type(configured) ~= "table" then
    error "wisp.setup options must be a table"
  end

  local allowed = {
    border = true,
    command = true,
    config_file = true,
    file_open = true,
    file_preview = true,
    height = true,
    keymap = true,
    keymap_options = true,
    width = true,
  }
  for key in pairs(configured) do
    if not allowed[key] then
      error("wisp.setup unknown option " .. tostring(key))
    end
  end
  for _, field in ipairs { "command", "config_file", "keymap" } do
    if configured[field] ~= nil and (type(configured[field]) ~= "string" or configured[field] == "") then
      error("wisp.setup " .. field .. " must be a non-empty string")
    end
  end
  for _, field in ipairs { "height", "width" } do
    if configured[field] ~= nil and (type(configured[field]) ~= "number" or configured[field] <= 0) then
      error("wisp.setup " .. field .. " must be a positive number")
    end
  end
  if configured.keymap_options ~= nil and type(configured.keymap_options) ~= "table" then
    error "wisp.setup keymap_options must be a table"
  end
  if configured.file_open ~= nil then
    if type(configured.file_open) ~= "table" then
      error "wisp.setup file_open must be a table"
    end
    for field in pairs(configured.file_open) do
      if field ~= "default" then
        error("wisp.setup file_open contains unknown field " .. tostring(field))
      end
    end
    if
      configured.file_open.default ~= "window"
      and configured.file_open.default ~= "right_pane"
      and configured.file_open.default ~= "bottom_pane"
    then
      error "wisp.setup file_open default must be window, right_pane, or bottom_pane"
    end
  end
  if configured.file_preview ~= nil then
    if type(configured.file_preview) ~= "table" then
      error "wisp.setup file_preview must be a table"
    end
    for field in pairs(configured.file_preview) do
      if field ~= "width" then
        error("wisp.setup file_preview contains unknown field " .. tostring(field))
      end
    end
    if
      type(configured.file_preview.width) ~= "number"
      or configured.file_preview.width <= 0
      or configured.file_preview.width > 1
    then
      error "wisp.setup file_preview width must be greater than zero and at most one"
    end
  end

  options = {
    border = configured.border or "rounded",
    command = configured.command or "Wisp",
    config_file = configured.config_file,
    file_open = { default = configured.file_open and configured.file_open.default or "window" },
    file_preview = configured.file_preview and { width = configured.file_preview.width } or nil,
    height = configured.height or 0.7,
    keymap = configured.keymap,
    keymap_options = configured.keymap_options or {},
    width = configured.width or 0.8,
    executable_path = deployed_wisp_path,
  }
end

local function notify_error(message)
  vim.notify("wisp: " .. message, vim.log.levels.ERROR)
end

local function dimension(value, available)
  local cells = value <= 1 and math.floor(available * value) or math.floor(value)
  return math.max(1, math.min(cells, math.max(1, available - 2)))
end

local function normal_file_path(buffer)
  if vim.api.nvim_get_option_value("buftype", { buf = buffer }) ~= "" then
    return nil
  end
  local path = vim.api.nvim_buf_get_name(buffer)
  if path == "" then
    return nil
  end
  path = vim.fn.fnamemodify(path, ":p")
  return path ~= "" and path or nil
end

local function current_file()
  return normal_file_path(vim.api.nvim_get_current_buf())
end

local function in_wezterm()
  return type(vim.env.WEZTERM_PANE) == "string" and vim.env.WEZTERM_PANE ~= ""
end

local function pane_context_available()
  return in_wezterm() and #vim.api.nvim_list_uis() > 0
end

local function collect_nvim_views()
  local current_window = vim.api.nvim_get_current_win()
  local views = {}
  for _, window in ipairs(vim.api.nvim_tabpage_list_wins(vim.api.nvim_get_current_tabpage())) do
    local inspected, view = pcall(function()
      if not vim.api.nvim_win_is_valid(window) then
        return nil
      end
      local path = normal_file_path(vim.api.nvim_win_get_buf(window))
      if not path then
        return nil
      end
      local saved, bottomline = vim.api.nvim_win_call(window, function()
        return vim.fn.winsaveview(), vim.fn.line "w$"
      end)
      return {
        window_id = tostring(window),
        path = path,
        active = window == current_window,
        width = vim.api.nvim_win_get_width(window),
        height = vim.api.nvim_win_get_height(window),
        bottomline = bottomline,
        view = {
          lnum = saved.lnum,
          col = saved.col,
          coladd = saved.coladd,
          curswant = saved.curswant,
          topline = saved.topline,
          topfill = saved.topfill,
          leftcol = saved.leftcol,
          skipcol = saved.skipcol,
        },
      }
    end)
    if inspected and view then
      table.insert(views, view)
    end
  end
  return views
end

local function publish_pane_state()
  if not pane_context_available() then
    return
  end
  local views = collect_nvim_views()
  local state = #views == 0 and "" or vim.json.encode { protocol_version = PROTOCOL_VERSION, views = views }
  local encoded = vim.base64.encode(state)
  io.stdout:write("\27]1337;SetUserVar=" .. WEZTERM_STATE_VAR .. "=" .. encoded .. "\27\\")
  io.stdout:flush()
end

local function publish_pane_state_now()
  viewport_generation = viewport_generation + 1
  publish_pane_state()
end

local function publish_pane_state_debounced()
  viewport_generation = viewport_generation + 1
  local generation = viewport_generation
  vim.defer_fn(function()
    if generation == viewport_generation then
      publish_pane_state()
    end
  end, 30)
end

local function clear_pane_state()
  viewport_generation = viewport_generation + 1
  if not pane_context_available() then
    return
  end
  io.stdout:write("\27]1337;SetUserVar=" .. WEZTERM_STATE_VAR .. "=\27\\")
  io.stdout:flush()
end

local function install_pane_context()
  if not in_wezterm() then
    return
  end
  local group = vim.api.nvim_create_augroup("WispPaneContext", { clear = true })
  if #vim.api.nvim_list_uis() == 0 then
    vim.api.nvim_create_autocmd("UIEnter", {
      group = group,
      once = true,
      callback = install_pane_context,
    })
    return
  end
  vim.api.nvim_create_autocmd(
    { "BufEnter", "BufFilePost", "BufWinEnter", "TabEnter", "WinEnter", "WinNew", "WinClosed" },
    {
      group = group,
      callback = publish_pane_state_now,
    }
  )
  vim.api.nvim_create_autocmd({ "CursorMoved", "WinScrolled", "VimResized", "WinResized" }, {
    group = group,
    callback = publish_pane_state_debounced,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = clear_pane_state,
  })
  publish_pane_state_now()
end

local function picker_args(result_path, active_project_path, active_file, preview_state_path)
  local args = { options.executable_path }
  if options.config_file then
    table.insert(args, "--config")
    table.insert(args, options.config_file)
  end
  table.insert(args, "pick")
  table.insert(args, "--disable-sessions")
  table.insert(args, "--file-open-target")
  table.insert(args, (options.file_open.default:gsub("_", "-")))
  if preview_state_path then
    table.insert(args, "--file-preview-state-file")
    table.insert(args, preview_state_path)
    table.insert(args, "--file-preview")
  end
  if active_project_path then
    table.insert(args, "--active-project-path")
    table.insert(args, active_project_path)
  end
  if active_file then
    table.insert(args, "--active-file")
    table.insert(args, active_file)
  end
  table.insert(args, "--result-file")
  table.insert(args, result_path)
  return args
end

local function cleanup(window, buffer)
  if vim.api.nvim_win_is_valid(window) then
    vim.api.nvim_win_close(window, true)
  end
  if vim.api.nvim_buf_is_valid(buffer) then
    vim.api.nvim_buf_delete(buffer, { force = true })
  end
end

local function copy_table(value)
  local copied = {}
  for key, field in pairs(value) do
    copied[key] = field
  end
  return copied
end

local function read_result(path)
  local file = io.open(path, "rb")
  if not file then
    return nil, "picker exited without writing a result"
  end
  local encoded = file:read "*a"
  file:close()
  vim.fn.delete(path)
  local result = FilePreview.decode_json(encoded)
  if not result then
    return nil, "picker returned invalid JSON"
  end
  return result
end

local function has_only_fields(value, allowed)
  for field in pairs(value) do
    if not allowed[field] then
      return false
    end
  end
  return true
end

local function is_array(value)
  if type(value) ~= "table" then
    return false
  end
  local count = 0
  for key in pairs(value) do
    if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
      return false
    end
    count = count + 1
  end
  return count == #value
end

local function valid_argv(argv)
  if not is_array(argv) or #argv == 0 then
    return false
  end
  for _, argument in ipairs(argv) do
    if type(argument) ~= "string" or argument == "" then
      return false
    end
  end
  return true
end

local function valid_project(project)
  return type(project) == "table"
    and has_only_fields(project, { id = true, path = true, group = true, name = true, display_name = true })
    and type(project.id) == "string"
    and project.id ~= ""
    and type(project.path) == "string"
    and project.path ~= ""
    and type(project.group) == "string"
    and type(project.name) == "string"
    and type(project.display_name) == "string"
end

local function selection_has_only_fields(selection)
  local fields = {
    project = { kind = true, project = true, opener = true },
    file = {
      kind = true,
      project = true,
      path = true,
      opener = true,
      open_target = true,
      reuse_existing = true,
      host_target = true,
    },
  }
  return fields[selection.kind] and has_only_fields(selection, fields[selection.kind])
end

local function valid_file_selection(selection)
  if
    type(selection.path) ~= "string"
    or selection.path == ""
    or (selection.open_target ~= "window" and selection.open_target ~= "right_pane" and selection.open_target ~= "bottom_pane")
    or type(selection.reuse_existing) ~= "boolean"
  then
    return false
  end
  if selection.host_target == nil then
    return true
  end
  return type(selection.host_target) == "table"
    and has_only_fields(selection.host_target, { window_id = true, pane_id = true })
    and type(selection.host_target.window_id) == "string"
    and selection.host_target.window_id ~= ""
    and type(selection.host_target.pane_id) == "string"
    and selection.host_target.pane_id ~= ""
end

local function configure_project_tab(project)
  vim.api.nvim_cmd({ cmd = "tcd", args = { project.path } }, {})
  vim.t.wisp_project_dir = project.path
  vim.t.wisp_project_name = project.name
end

local function path_identity(path)
  if type(path) ~= "string" then
    return nil
  end
  local windows_drive = path:match "^%a:[/\\]" ~= nil
  local windows_unc = path:match "^[/\\][/\\]" ~= nil
  local replaced = path:gsub("\\", "/")
  local collapsed = windows_unc and "//" .. replaced:sub(3):gsub("/+", "/") or replaced:gsub("/+", "/")
  local prefix = ""
  local rest = collapsed
  local protected_components = 0
  if windows_unc then
    prefix = "//"
    rest = collapsed:sub(3)
    protected_components = 2
  elseif windows_drive then
    prefix = collapsed:sub(1, 3)
    rest = collapsed:sub(4)
  elseif collapsed:sub(1, 1) == "/" then
    prefix = "/"
    rest = collapsed:sub(2)
  end
  local components = {}
  for component in rest:gmatch "[^/]+" do
    if component == "." then
      -- Skip current-directory components.
    elseif component == ".." and #components > protected_components and components[#components] ~= ".." then
      table.remove(components)
    elseif component == ".." and prefix == "" then
      table.insert(components, component)
    elseif component ~= ".." then
      table.insert(components, component)
    end
  end
  local normalized = prefix .. table.concat(components, "/")
  if normalized == "" then
    normalized = "."
  end
  if windows_drive or windows_unc then
    normalized = normalized:gsub("[A-Z]", function(character)
      return string.char(character:byte() + 32)
    end)
  end
  return normalized
end

local function focus_existing_file(originating_tab, originating_window, selection)
  local selected_identity = path_identity(selection.path)
  local best_window
  local best_tab
  local best_rank = -1
  for _, tab in ipairs(vim.api.nvim_list_tabpages()) do
    local active_window = vim.api.nvim_tabpage_get_win(tab)
    for _, window in ipairs(vim.api.nvim_tabpage_list_wins(tab)) do
      local inspected, path = pcall(function()
        if not vim.api.nvim_win_is_valid(window) then
          return nil
        end
        return normal_file_path(vim.api.nvim_win_get_buf(window))
      end)
      if inspected and path and path_identity(path) == selected_identity then
        local rank = (window == originating_window and 8 or 0)
          + (tab == originating_tab and 4 or 0)
          + (window == active_window and 2 or 0)
        if rank > best_rank then
          best_rank = rank
          best_tab = tab
          best_window = window
        end
      end
    end
  end
  if not best_window then
    return false
  end
  local focused = pcall(function()
    vim.api.nvim_set_current_tabpage(best_tab)
    vim.api.nvim_set_current_win(best_window)
    configure_project_tab(selection.project)
  end)
  return focused
end

local function open_file(originating_tab, originating_window, selection)
  vim.api.nvim_set_current_tabpage(originating_tab)
  pcall(function()
    if
      vim.api.nvim_win_is_valid(originating_window)
      and vim.api.nvim_win_get_tabpage(originating_window) == originating_tab
    then
      vim.api.nvim_set_current_win(originating_window)
    end
  end)
  local command = {
    window = { cmd = "tabnew", args = { selection.path } },
    right_pane = { cmd = "vsplit", args = { selection.path }, mods = { split = "belowright" } },
    bottom_pane = { cmd = "split", args = { selection.path }, mods = { split = "belowright" } },
  }
  vim.api.nvim_cmd(command[selection.open_target], {})
  configure_project_tab(selection.project)
end

local function valid_result_state(result)
  if result.status == "selected" then
    return type(result.selection) == "table" and result.error == nil
  end
  if result.status == "cancelled" then
    return result.selection == nil and result.error == nil
  end
  if result.status == "error" then
    return result.selection == nil and type(result.error) == "string"
  end
  return false
end

local function apply_result(originating_tab, originating_window, result)
  if type(result) ~= "table" or result.protocol_version ~= PROTOCOL_VERSION then
    notify_error "result has an unsupported protocol version"
    return
  end
  if not has_only_fields(result, { protocol_version = true, status = true, selection = true, error = true }) then
    notify_error "result is not a valid result envelope"
    return
  end
  if not valid_result_state(result) then
    notify_error "result is not a valid result envelope"
    return
  end
  if result.status == "cancelled" then
    return
  end
  if result.status == "error" then
    notify_error("picker failed: " .. tostring(result.error))
    return
  end
  local selection = result.selection
  if
    result.status ~= "selected"
    or type(selection) ~= "table"
    or not selection_has_only_fields(selection)
    or (selection.opener ~= nil and not valid_argv(selection.opener))
    or not valid_project(selection.project)
  then
    notify_error "result is not a valid selection"
    return
  end
  if selection.kind ~= "project" and (selection.kind ~= "file" or not valid_file_selection(selection)) then
    notify_error "result contains an unknown selection kind"
    return
  end
  if selection.kind == "file" and selection.reuse_existing then
    if focus_existing_file(originating_tab, originating_window, selection) then
      return
    end
  end
  if not vim.api.nvim_tabpage_is_valid(originating_tab) then
    notify_error "originating tab no longer exists"
    return
  end

  if selection.kind == "file" then
    open_file(originating_tab, originating_window, selection)
  else
    vim.api.nvim_tabpage_call(originating_tab, function()
      configure_project_tab(selection.project)
    end)
  end
end

function M.open()
  local originating_tab = vim.api.nvim_get_current_tabpage()
  local originating_window = vim.api.nvim_get_current_win()
  local active_project_path = vim.t.wisp_project_dir
  local active_file = current_file()
  local result_path = vim.fn.tempname()
  vim.fn.delete(result_path)
  local preview_state_path = options.file_preview and vim.fn.tempname() or nil
  if preview_state_path then
    vim.fn.delete(preview_state_path)
  end

  local width = dimension(options.width, vim.o.columns)
  local height = dimension(options.height, vim.o.lines)
  local window_config = {
    border = options.border,
    col = math.floor((vim.o.columns - width) / 2),
    height = height,
    relative = "editor",
    row = math.max(0, math.floor((vim.o.lines - height) / 2) - 1),
    style = "minimal",
    title = " Wisp ",
    title_pos = "center",
    width = width,
  }
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_set_option_value("bufhidden", "wipe", { buf = buffer })
  local window = vim.api.nvim_open_win(buffer, true, window_config)
  local preview = preview_state_path and { path = preview_state_path } or nil
  local cleaned = false

  local function close_companion(restore_picker)
    if not preview then
      return
    end
    if preview.window and vim.api.nvim_win_is_valid(preview.window) then
      vim.api.nvim_win_close(preview.window, true)
    end
    if preview.buffer and vim.api.nvim_buf_is_valid(preview.buffer) then
      vim.api.nvim_buf_delete(preview.buffer, { force = true })
    end
    preview.window = nil
    preview.buffer = nil
    preview.renderer = nil
    if restore_picker and vim.api.nvim_win_is_valid(window) then
      vim.api.nvim_win_set_config(window, window_config)
    end
  end

  local function cleanup_all()
    if cleaned then
      return
    end
    cleaned = true
    if preview and preview.watcher then
      preview.watcher:stop()
      preview.watcher = nil
    end
    close_companion(false)
    if preview then
      vim.fn.delete(preview.path)
    end
    cleanup(window, buffer)
  end

  local function update_preview(envelope)
    if cleaned or not preview then
      return
    end
    if envelope.state.state == "hidden" then
      close_companion(true)
      return
    end
    if not preview.window then
      local available = math.max(2, width - 2)
      local preview_width = math.max(1, math.min(available - 1, math.floor(available * options.file_preview.width)))
      local picker_width = math.max(1, available - preview_width)
      local picker_config = copy_table(window_config)
      picker_config.width = picker_width
      vim.api.nvim_win_set_config(window, picker_config)

      local preview_config = copy_table(window_config)
      preview_config.col = window_config.col + picker_width + 2
      preview_config.title = " Preview "
      preview_config.width = preview_width
      local preview_buffer = vim.api.nvim_create_buf(false, true)
      local opened, preview_window = pcall(vim.api.nvim_open_win, preview_buffer, false, preview_config)
      if not opened then
        if vim.api.nvim_buf_is_valid(preview_buffer) then
          vim.api.nvim_buf_delete(preview_buffer, { force = true })
        end
        vim.api.nvim_win_set_config(window, window_config)
        notify_error("could not open file preview: " .. tostring(preview_window))
        return
      end
      preview.buffer = preview_buffer
      preview.window = preview_window
      preview.renderer = FilePreview.new(preview_window, preview_buffer)
    end
    preview.renderer:render(envelope.state)
  end

  local job_exited = false
  local job = vim.fn.jobstart(picker_args(result_path, active_project_path, active_file, preview_state_path), {
    on_exit = function(_, exit_code)
      vim.schedule(function()
        job_exited = true
        cleanup_all()
        local result, result_error = read_result(result_path)
        if not result then
          notify_error(result_error .. " (exit " .. tostring(exit_code) .. ")")
          return
        end
        apply_result(originating_tab, originating_window, result)
      end)
    end,
    term = true,
  })
  if job <= 0 then
    cleanup_all()
    vim.fn.delete(result_path)
    notify_error "could not start the wisp executable"
    return
  end
  if preview and not job_exited then
    preview.watcher = FilePreview.watch(preview.path, update_preview, function(preview_error)
      notify_error("file preview failed: " .. tostring(preview_error))
    end)
  end
  vim.cmd.startinsert()
end

function M.setup(configured)
  configure(configured)
  if vim.t.wisp_project_dir == nil and vim.env.WISP_PROJECT_DIR and vim.env.WISP_PROJECT_DIR ~= "" then
    vim.t.wisp_project_dir = vim.env.WISP_PROJECT_DIR
  end
  if vim.t.wisp_project_name == nil and vim.env.WISP_PROJECT_NAME and vim.env.WISP_PROJECT_NAME ~= "" then
    vim.t.wisp_project_name = vim.env.WISP_PROJECT_NAME
  end
  install_pane_context()

  vim.api.nvim_create_user_command(options.command, M.open, {
    desc = "Open Wisp project and file picker",
    force = true,
  })
  if options.keymap then
    local keymap_options = {}
    for key, value in pairs(options.keymap_options) do
      keymap_options[key] = value
    end
    keymap_options.desc = keymap_options.desc or "Open Wisp picker"
    vim.keymap.set("n", options.keymap, M.open, keymap_options)
  end
end

configure {}

return M
