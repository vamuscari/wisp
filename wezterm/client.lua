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

local function is_integer(value, minimum)
  return type(value) == "number" and value % 1 == 0 and value >= minimum
end

local function has_no_duplicate_fields(encoded, parse_json)
  if type(encoded) ~= "string" then
    return false
  end
  local length = #encoded
  local function skip_space(index)
    while index <= length and encoded:sub(index, index):match "%s" do
      index = index + 1
    end
    return index
  end
  local initial = skip_space(1)
  if encoded:sub(initial, initial) ~= "{" and encoded:sub(initial, initial) ~= "[" then
    return true
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
        local decoded, key = pcall(parse_json, raw_key)
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

  local final = parse_value(initial)
  return final ~= nil and skip_space(final) == length + 1
end

local function is_absolute_path(path)
  return type(path) == "string"
    and path ~= ""
    and (path:sub(1, 1) == "/" or path:match "^%a:[/\\]" ~= nil or path:sub(1, 2) == "\\\\")
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
  local viewport = value.view
  if type(viewport) ~= "table" or not has_only_fields(viewport, VIEWPORT_FIELDS) then
    return false
  end
  for field in pairs(VIEWPORT_FIELDS) do
    local minimum = (field == "lnum" or field == "topline") and 1 or 0
    if not is_integer(viewport[field], minimum) then
      return false
    end
  end
  return value.bottomline >= viewport.topline
end

local function valid_nvim_views(views)
  if not is_array(views) then
    return false
  end
  local seen = {}
  for _, view in ipairs(views) do
    if not valid_nvim_view(view) or seen[view.window_id] then
      return false
    end
    seen[view.window_id] = true
  end
  return true
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
    close_project = { kind = true, project = true },
    host_pane = { kind = true, project = true, window_id = true, pane_id = true },
    workspace = { kind = true, workspace = true },
    workspace_pane = { kind = true, workspace = true, window_id = true, pane_id = true },
    close_workspace = { kind = true, workspace = true },
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

function Client:parse_json(encoded)
  if not has_no_duplicate_fields(encoded, self.wezterm.json_parse) then
    return nil
  end
  local parsed, value = pcall(self.wezterm.json_parse, encoded)
  return parsed and value or nil
end

function Client:parse_nvim_state(encoded)
  if type(encoded) ~= "string" or encoded == "" then
    return nil
  end
  local envelope = self:parse_json(encoded)
  if
    type(envelope) ~= "table"
    or envelope.protocol_version ~= self.protocol_version
    or not has_only_fields(envelope, { protocol_version = true, views = true })
    or not valid_nvim_views(envelope.views)
  then
    return nil
  end
  return envelope.views
end

function Client:nvim_views(pane)
  local inspected_process, process = pcall(function()
    return pane and pane:get_foreground_process_name()
  end)
  if inspected_process and type(process) == "string" and process ~= "" then
    local name = process:match "([^/\\]+)$"
    name = type(name) == "string" and name:lower() or nil
    if name ~= "nvim" and name ~= "nvim.exe" then
      return nil
    end
  end
  local inspected_vars, user_vars = pcall(function()
    return pane and pane:get_user_vars()
  end)
  local encoded = inspected_vars and type(user_vars) == "table" and user_vars.WISP_NVIM_STATE or nil
  return self:parse_nvim_state(encoded)
end

function Client:parse_file_preview(encoded, last_sequence)
  local envelope = self:parse_json(encoded)
  if
    type(envelope) ~= "table"
    or envelope.protocol_version ~= self.protocol_version
    or not has_only_fields(envelope, { protocol_version = true, sequence = true, state = true })
    or not is_integer(envelope.sequence, 0)
    or envelope.sequence <= (last_sequence or -1)
    or type(envelope.state) ~= "table"
  then
    return nil
  end
  local state = envelope.state
  if state.state == "hidden" or state.state == "empty" then
    if not has_only_fields(state, { state = true }) then
      return nil
    end
  elseif state.state == "file" then
    if
      not has_only_fields(state, { state = true, project = true, path = true, nvim_view = true })
      or not self:valid_project(state.project)
      or not is_absolute_path(state.path)
      or (state.nvim_view ~= nil and not valid_nvim_view(state.nvim_view))
    then
      return nil
    end
  else
    return nil
  end
  return envelope
end

function Client:query_projects()
  local stdout, command_error = self:run("projects", "--json")
  if not stdout then
    return nil, command_error
  end
  local envelope = self:parse_json(stdout)
  if type(envelope) ~= "table" then
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
  local envelope = self:parse_json(stdout)
  if type(envelope) ~= "table" then
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
  local uses_project = selection.kind == "project"
    or selection.kind == "file"
    or selection.kind == "close_project"
    or selection.kind == "host_pane"
    or selection.kind == "open_code_session"
  if uses_project then
    if not self:valid_project(selection.project) then
      return nil, "wisp result contains an invalid project"
    end
  elseif type(selection.workspace) ~= "string" or selection.workspace == "" then
    return nil, "wisp result contains an invalid workspace"
  end
  if selection.kind == "host_pane" or selection.kind == "workspace_pane" then
    if
      type(selection.window_id) ~= "string"
      or selection.window_id == ""
      or type(selection.pane_id) ~= "string"
      or selection.pane_id == ""
    then
      return nil, "wisp result contains an invalid host pane"
    end
  end
  if selection.kind == "file" then
    local targets = { window = true, right_pane = true, bottom_pane = true }
    if
      type(selection.path) ~= "string"
      or selection.path == ""
      or not targets[selection.open_target]
      or type(selection.reuse_existing) ~= "boolean"
    then
      return nil, "wisp result contains an invalid file selection"
    end
    if selection.host_target ~= nil then
      local target = selection.host_target
      if
        type(target) ~= "table"
        or not has_only_fields(target, { window_id = true, pane_id = true })
        or type(target.window_id) ~= "string"
        or target.window_id == ""
        or type(target.pane_id) ~= "string"
        or target.pane_id == ""
      then
        return nil, "wisp result contains an invalid file host target"
      end
    end
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
