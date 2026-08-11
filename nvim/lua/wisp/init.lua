local M = {}
local deployed_wisp_path, deployment_token = ...

if type(deployed_wisp_path) ~= "string" or deployed_wisp_path == "" or deployment_token ~= "wisp-deployment-v1" then
  error "Wisp's Neovim adapter must be loaded by the deployed runtime"
end

local options = {}
local PROTOCOL_VERSION = 2

local function configure(configured)
  configured = configured or {}
  if type(configured) ~= "table" then
    error "wisp.setup options must be a table"
  end

  local allowed = {
    border = true,
    command = true,
    config_file = true,
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

  options = {
    border = configured.border or "rounded",
    command = configured.command or "Wisp",
    config_file = configured.config_file,
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

local function picker_args(result_path)
  local args = { options.executable_path }
  if options.config_file then
    table.insert(args, "--config")
    table.insert(args, options.config_file)
  end
  table.insert(args, "pick")
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

local function read_result(path)
  local file = io.open(path, "rb")
  if not file then
    return nil, "picker exited without writing a result"
  end
  local encoded = file:read "*a"
  file:close()
  vim.fn.delete(path)
  local decoded, result = pcall(vim.json.decode, encoded)
  if not decoded then
    return nil, "picker returned invalid JSON: " .. tostring(result)
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
    file = { kind = true, project = true, path = true, opener = true },
  }
  return fields[selection.kind] and has_only_fields(selection, fields[selection.kind])
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

local function apply_result(originating_tab, result)
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
  if
    selection.kind ~= "project"
    and (selection.kind ~= "file" or type(selection.path) ~= "string" or selection.path == "")
  then
    notify_error "result contains an unknown selection kind"
    return
  end
  if not vim.api.nvim_tabpage_is_valid(originating_tab) then
    notify_error "originating tab no longer exists"
    return
  end

  vim.api.nvim_tabpage_call(originating_tab, function()
    vim.api.nvim_cmd({ cmd = "tcd", args = { selection.project.path } }, {})
    vim.t.wisp_project_dir = selection.project.path
    vim.t.wisp_project_name = selection.project.name
    if selection.kind == "file" then
      vim.api.nvim_cmd({ cmd = "edit", args = { selection.path } }, {})
    end
  end)
end

function M.open()
  local originating_tab = vim.api.nvim_get_current_tabpage()
  local result_path = vim.fn.tempname()
  vim.fn.delete(result_path)

  local width = dimension(options.width, vim.o.columns)
  local height = dimension(options.height, vim.o.lines)
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_set_option_value("bufhidden", "wipe", { buf = buffer })
  local window = vim.api.nvim_open_win(buffer, true, {
    border = options.border,
    col = math.floor((vim.o.columns - width) / 2),
    height = height,
    relative = "editor",
    row = math.max(0, math.floor((vim.o.lines - height) / 2) - 1),
    style = "minimal",
    title = " Wisp ",
    title_pos = "center",
    width = width,
  })

  local job = vim.fn.jobstart(picker_args(result_path), {
    on_exit = function(_, exit_code)
      vim.schedule(function()
        cleanup(window, buffer)
        local result, result_error = read_result(result_path)
        if not result then
          notify_error(result_error .. " (exit " .. tostring(exit_code) .. ")")
          return
        end
        apply_result(originating_tab, result)
      end)
    end,
    term = true,
  })
  if job <= 0 then
    cleanup(window, buffer)
    vim.fn.delete(result_path)
    notify_error "could not start the wisp executable"
    return
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
