local Options = {}
Options.__index = Options

local DEFAULT_STATUS_COLORS = {
  foreground = "#E9E2C9",
  opencode_background = "#2A5173",
  workspace_background = "#333F0A",
  active_workspace_background = "#7A1405",
  waiting_background = "#957C16",
  running_background = "#50620F",
  idle_background = "#66615C",
  failure_background = "#5E0F04",
}

local STATUS_COLOR_FIELDS = {
  foreground = true,
  opencode_background = true,
  workspace_background = true,
  active_workspace_background = true,
  waiting_background = true,
  running_background = true,
  idle_background = true,
  failure_background = true,
}

local STATUS_PROVIDERS = { opencode = true, directory = true }
local STATUS_ACTIONS = { projects = true, windows = true, sessions = true }
local POPUP_DIRECTIONS = { Top = true, Bottom = true, Left = true, Right = true }
local FILE_OPEN_TARGETS = { window = true, right_pane = true, bottom_pane = true }
local SINGLE_PANE_BEHAVIORS = { show = true, activate = true }
local DEFAULT_STATUS_ITEMS = {
  { name = "opencode", action = "sessions" },
  { name = "directory", action = "projects" },
}

local function validate_domain(domain, label)
  if type(domain) ~= "table" or type(domain.DomainName) ~= "string" or domain.DomainName == "" then
    error("wisp " .. label .. " must be a non-empty { DomainName = name } table")
  end
end

local function validate(configured)
  if type(configured) ~= "table" then
    error "wisp options must be a table"
  end

  local allowed = {
    config_file = true,
    domain_for_project = true,
    file_open = true,
    file_preview = true,
    picker_binding = true,
    picker_domain = true,
    picker_timeout_seconds = true,
    poll_interval_seconds = true,
    popup = true,
    spawn_domain = true,
    single_pane_behavior = true,
    status_bar = true,
    status_colors = true,
    status_items = true,
    status_interval_seconds = true,
    window_preview = true,
    workspace_for_project = true,
    workspace_prefix = true,
  }
  for key in pairs(configured) do
    if not allowed[key] then
      error("wisp unknown option " .. tostring(key))
    end
  end

  if configured.config_file ~= nil and (type(configured.config_file) ~= "string" or configured.config_file == "") then
    error "wisp config_file must be a non-empty string"
  end
  if configured.workspace_prefix ~= nil then
    if type(configured.workspace_prefix) ~= "string" or configured.workspace_prefix == "" then
      error "wisp workspace_prefix must be a non-empty string"
    end
  end
  if configured.picker_binding ~= nil and type(configured.picker_binding) ~= "table" then
    error "wisp picker_binding must be a key assignment table"
  end
  for _, field in ipairs { "poll_interval_seconds", "picker_timeout_seconds", "status_interval_seconds" } do
    if configured[field] ~= nil and (type(configured[field]) ~= "number" or configured[field] <= 0) then
      error("wisp " .. field .. " must be a positive number")
    end
  end
  for _, field in ipairs { "workspace_for_project", "domain_for_project" } do
    if configured[field] ~= nil and type(configured[field]) ~= "function" then
      error("wisp " .. field .. " must be a function")
    end
  end
  if configured.spawn_domain ~= nil then
    validate_domain(configured.spawn_domain, "spawn_domain")
  end
  if configured.picker_domain ~= nil then
    validate_domain(configured.picker_domain, "picker_domain")
  end
  if configured.status_bar ~= nil and type(configured.status_bar) ~= "boolean" then
    error "wisp status_bar must be a boolean"
  end
  if configured.file_open ~= nil then
    if type(configured.file_open) ~= "table" then
      error "wisp file_open must be a table"
    end
    for field in pairs(configured.file_open) do
      if field ~= "default" then
        error("wisp file_open contains unknown field " .. tostring(field))
      end
    end
    if not FILE_OPEN_TARGETS[configured.file_open.default] then
      error "wisp file_open default must be window, right_pane, or bottom_pane"
    end
  end
  if configured.file_preview ~= nil then
    if type(configured.file_preview) ~= "table" then
      error "wisp file_preview must be a table"
    end
    for field in pairs(configured.file_preview) do
      if field ~= "command" and field ~= "direction" and field ~= "size" then
        error("wisp file_preview contains unknown field " .. tostring(field))
      end
    end
    local command = configured.file_preview.command
    if type(command) ~= "table" or #command == 0 then
      error "wisp file_preview command must be a dense non-empty argv array"
    end
    local count = 0
    for key, argument in pairs(command) do
      if type(key) ~= "number" or key < 1 or key % 1 ~= 0 or type(argument) ~= "string" or argument == "" then
        error "wisp file_preview command must be a dense non-empty argv array"
      end
      count = count + 1
    end
    if count ~= #command then
      error "wisp file_preview command must be a dense non-empty argv array"
    end
    if not POPUP_DIRECTIONS[configured.file_preview.direction] then
      error "wisp file_preview direction must be Top, Bottom, Left, or Right"
    end
    if type(configured.file_preview.size) ~= "number" or configured.file_preview.size <= 0 then
      error "wisp file_preview size must be a positive number"
    end
  end
  if configured.window_preview ~= nil and type(configured.window_preview) ~= "boolean" then
    error "wisp window_preview must be a boolean"
  end
  if configured.single_pane_behavior ~= nil and not SINGLE_PANE_BEHAVIORS[configured.single_pane_behavior] then
    error "wisp single_pane_behavior must be show or activate"
  end
  if configured.status_items ~= nil then
    if type(configured.status_items) ~= "table" then
      error "wisp status_items must be an array"
    end
    local count = 0
    for key in pairs(configured.status_items) do
      if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
        error "wisp status_items must be a dense array"
      end
      count = count + 1
    end
    for index = 1, count do
      if configured.status_items[index] == nil then
        error "wisp status_items must be a dense array"
      end
    end
    local seen = {}
    for index = 1, count do
      local item = configured.status_items[index]
      if type(item) ~= "table" then
        error("wisp status_items entry " .. index .. " must be a table")
      end
      for field in pairs(item) do
        if field ~= "name" and field ~= "action" then
          error("wisp status_items entry " .. index .. " contains unknown field " .. tostring(field))
        end
      end
      if not STATUS_PROVIDERS[item.name] then
        error("wisp status_items entry " .. index .. " has an invalid name")
      end
      if item.action ~= nil and not STATUS_ACTIONS[item.action] then
        error("wisp status_items entry " .. index .. " has an invalid action")
      end
      if seen[item.name] then
        error("wisp status_items contains duplicate provider " .. item.name)
      end
      seen[item.name] = true
    end
  end
  if configured.popup ~= nil then
    if type(configured.popup) ~= "table" then
      error "wisp popup must be a table"
    end
    for field in pairs(configured.popup) do
      if field ~= "direction" and field ~= "size" then
        error("wisp popup contains unknown field " .. tostring(field))
      end
    end
    if configured.popup.direction ~= nil and not POPUP_DIRECTIONS[configured.popup.direction] then
      error "wisp popup direction must be Top, Bottom, Left, or Right"
    end
    if configured.popup.size ~= nil and (type(configured.popup.size) ~= "number" or configured.popup.size <= 0) then
      error "wisp popup size must be a positive number"
    end
  end
  if configured.status_colors ~= nil then
    if type(configured.status_colors) ~= "table" then
      error "wisp status_colors must be a table"
    end
    for field, value in pairs(configured.status_colors) do
      if not STATUS_COLOR_FIELDS[field] then
        error("wisp status_colors contains unknown field " .. tostring(field))
      end
      if type(value) ~= "string" or value == "" then
        error("wisp status_colors " .. field .. " must be a non-empty string")
      end
    end
  end
end

function Options.new(executable_path, module_directory)
  local self = setmetatable({ executable_path = executable_path, module_directory = module_directory }, Options)
  self:configure {}
  return self
end

function Options:configure(configured)
  validate(configured)
  local spawn_domain = configured.spawn_domain or { DomainName = "local" }
  local status_colors = {}
  for field, value in pairs(DEFAULT_STATUS_COLORS) do
    status_colors[field] = configured.status_colors and configured.status_colors[field] or value
  end
  local status_items = {}
  for _, item in ipairs(configured.status_items or DEFAULT_STATUS_ITEMS) do
    table.insert(status_items, { name = item.name, action = item.action })
  end
  local popup = configured.popup or {}
  local file_preview
  if configured.file_preview then
    local command = {}
    for _, argument in ipairs(configured.file_preview.command) do
      table.insert(command, argument)
    end
    file_preview = {
      command = command,
      direction = configured.file_preview.direction,
      size = configured.file_preview.size,
      script = self.module_directory .. "/../nvim/lua/wisp/file_preview.lua",
    }
  end
  self.values = {
    config_file = configured.config_file,
    domain_for_project = configured.domain_for_project,
    file_open = { default = configured.file_open and configured.file_open.default or "window" },
    file_preview = file_preview,
    picker_binding = configured.picker_binding,
    picker_domain = configured.picker_domain or spawn_domain,
    picker_timeout_seconds = configured.picker_timeout_seconds or 3600,
    poll_interval_seconds = configured.poll_interval_seconds or 0.05,
    popup = {
      direction = popup.direction or "Bottom",
      size = popup.size or 0.65,
    },
    spawn_domain = spawn_domain,
    single_pane_behavior = configured.single_pane_behavior or "show",
    status_bar = configured.status_bar ~= false,
    status_colors = status_colors,
    status_items = status_items,
    status_interval_seconds = configured.status_interval_seconds or 2,
    window_preview = configured.window_preview == true,
    executable_path = self.executable_path,
    workspace_for_project = configured.workspace_for_project,
    workspace_prefix = configured.workspace_prefix or "wisp:",
  }
end

function Options:get()
  return self.values
end

function Options:workspace_for(project)
  local values = self.values
  local workspace
  if values.workspace_for_project then
    workspace = values.workspace_for_project(project)
  else
    workspace = values.workspace_prefix .. project.group .. "/" .. project.name
  end
  if type(workspace) ~= "string" or workspace == "" then
    error("wisp workspace_for_project returned an invalid workspace for " .. project.id)
  end
  return workspace
end

function Options:domain_for(project)
  local values = self.values
  local domain = values.domain_for_project and values.domain_for_project(project) or values.spawn_domain
  validate_domain(domain, "domain_for_project result")
  return domain
end

return Options
