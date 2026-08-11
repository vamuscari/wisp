local Client = {}
Client.__index = Client

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

local function selection_has_only_fields(selection)
  local fields = {
    project = { kind = true, project = true, opener = true },
    file = { kind = true, project = true, path = true, opener = true },
    close_project = { kind = true, project = true },
    host_item = { kind = true, project = true, id = true },
    open_code_session = {
      kind = true,
      project = true,
      session_id = true,
      opener = true,
      host_item_id = true,
    },
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

function Client.new(wezterm, options, protocol_version)
  return setmetatable({ wezterm = wezterm, options = options, protocol_version = protocol_version }, Client)
end

function Client:args(...)
  local values = self.options:get()
  local args = { values.executable_path }
  if values.config_file then
    table.insert(args, "--config")
    table.insert(args, values.config_file)
  end
  for index = 1, select("#", ...) do
    local argument = select(index, ...)
    table.insert(args, argument)
  end
  return args
end

function Client:run(...)
  local success, stdout, stderr = self.wezterm.run_child_process(self:args(...))
  if not success then
    local message = stderr ~= "" and stderr or stdout
    return nil, "wisp command failed: " .. tostring(message)
  end
  return stdout
end

function Client:valid_argv(argv)
  if type(argv) ~= "table" or #argv == 0 then
    return false
  end
  for _, argument in ipairs(argv) do
    if type(argument) ~= "string" or argument == "" then
      return false
    end
  end
  return true
end

function Client:valid_project(project)
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

function Client:query_projects()
  local stdout, command_error = self:run("projects", "--json")
  if not stdout then
    return nil, command_error
  end
  local parsed, envelope = pcall(self.wezterm.json_parse, stdout)
  if not parsed or type(envelope) ~= "table" then
    return nil, "wisp projects returned invalid JSON"
  end
  if envelope.protocol_version ~= self.protocol_version then
    return nil, "wisp projects returned an unsupported protocol version"
  end
  if not has_only_fields(envelope, { protocol_version = true, projects = true }) then
    return nil, "wisp projects returned an invalid envelope"
  end
  local projects = envelope.projects
  if not is_array(projects) then
    return nil, "wisp projects returned an invalid project list"
  end
  for index, project in ipairs(projects) do
    if not self:valid_project(project) then
      return nil, "wisp projects returned an invalid project at index " .. index
    end
  end
  return projects
end

function Client:query_opencode_status()
  local stdout, command_error = self:run("opencode", "status", "--json")
  if not stdout then
    return nil, command_error
  end
  local parsed, envelope = pcall(self.wezterm.json_parse, stdout)
  if not parsed or type(envelope) ~= "table" then
    return nil, "wisp opencode status returned invalid JSON"
  end
  if envelope.protocol_version ~= self.protocol_version then
    return nil, "wisp opencode status returned an unsupported protocol version"
  end
  if not has_only_fields(envelope, { protocol_version = true, sessions = true }) or envelope.sessions == nil then
    return nil, "wisp opencode status returned an invalid envelope"
  end
  local sessions = envelope.sessions
  local fields = { waiting = true, running = true, retrying = true, idle = true, error = true }
  if type(sessions) ~= "table" or not has_only_fields(sessions, fields) then
    return nil, "wisp opencode status returned invalid session counts"
  end
  for field in pairs(fields) do
    local count = sessions[field]
    if type(count) ~= "number" or count < 0 or count % 1 ~= 0 then
      return nil, "wisp opencode status returned invalid session counts"
    end
  end
  return sessions
end

function Client:validate_result(result)
  if type(result) ~= "table" or result.protocol_version ~= self.protocol_version then
    return nil, "wisp result has an unsupported protocol version"
  end
  if not has_only_fields(result, { protocol_version = true, status = true, selection = true, error = true }) then
    return nil, "wisp result is not a valid result envelope"
  end
  if not valid_result_state(result) then
    return nil, "wisp result is not a valid result envelope"
  end
  if result.status ~= "selected" then
    return true
  end

  local selection = result.selection
  if not selection_has_only_fields(selection) then
    return nil, "wisp result is not a valid selection"
  end
  if selection.opener ~= nil and not self:valid_argv(selection.opener) then
    return nil, "wisp result is not a valid selection"
  end
  if not self:valid_project(selection.project) then
    return nil, "wisp result contains an invalid project"
  end
  if selection.kind == "open_code_session" then
    if
      type(selection.session_id) ~= "string"
      or selection.session_id == ""
      or not self:valid_argv(selection.opener)
    then
      return nil, "wisp result contains an invalid OpenCode session"
    end
    if selection.host_item_id ~= nil and (type(selection.host_item_id) ~= "string" or selection.host_item_id == "") then
      return nil, "wisp result contains an invalid OpenCode host item"
    end
  end
  return true
end

return Client
