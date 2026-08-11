local Options = {}
Options.__index = Options

local DEFAULT_STATUS_COLORS = {
  foreground = "#E9E2C9",
  workspace_background = "#333F0A",
  active_workspace_background = "#7A1405",
  waiting_background = "#957C16",
  running_background = "#50620F",
  retrying_background = "#683504",
  idle_background = "#66615C",
  error_background = "#5E0F04",
}

local STATUS_COLOR_FIELDS = {
  foreground = true,
  workspace_background = true,
  active_workspace_background = true,
  waiting_background = true,
  running_background = true,
  retrying_background = true,
  idle_background = true,
  error_background = true,
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
    picker_binding = true,
    picker_domain = true,
    picker_timeout_seconds = true,
    poll_interval_seconds = true,
    spawn_domain = true,
    status_bar = true,
    status_colors = true,
    status_interval_seconds = true,
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

function Options.new(executable_path)
  local self = setmetatable({ executable_path = executable_path }, Options)
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
  self.values = {
    config_file = configured.config_file,
    domain_for_project = configured.domain_for_project,
    picker_binding = configured.picker_binding,
    picker_domain = configured.picker_domain or spawn_domain,
    picker_timeout_seconds = configured.picker_timeout_seconds or 3600,
    poll_interval_seconds = configured.poll_interval_seconds or 0.05,
    spawn_domain = spawn_domain,
    status_bar = configured.status_bar ~= false,
    status_colors = status_colors,
    status_interval_seconds = configured.status_interval_seconds or 2,
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
