local Picker = {}
Picker.__index = Picker

local function basename(path)
  return type(path) == "string" and path:match "([^/\\]+)$" or nil
end

local function temporary_path()
  local path = os.tmpname()
  os.remove(path)
  return path
end

local function write_file(path, contents)
  local file, open_error = io.open(path, "wb")
  if not file then
    return nil, open_error
  end
  local written, write_error = file:write(contents)
  local closed, close_error = file:close()
  if not written then
    return nil, write_error
  end
  if not closed then
    return nil, close_error
  end
  return true
end

function Picker.new(wezterm, options, client, workspace, report_error)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
    workspace = workspace,
    report_error = report_error,
    opencode_session_tabs = {},
  }, Picker)
end

function Picker:project_relative_cwd(project, pane)
  local cwd = pane and pane:get_current_working_dir()
  if not cwd or cwd.scheme ~= "file" or type(cwd.file_path) ~= "string" then
    return nil
  end
  local root = project.path:gsub("[/\\]+$", "")
  local path = cwd.file_path:gsub("[/\\]+$", "")
  if path == root then
    return "."
  end
  local prefix = root .. "/"
  if path:sub(1, #prefix) == prefix then
    return path:sub(#prefix + 1)
  end
  return nil
end

function Picker:host_item(project, tab_info, current_workspace)
  local tab = tab_info.tab
  local pane = tab:active_pane()
  local label = tab:get_title()
  if type(label) ~= "string" or label == "" then
    label = pane and pane:get_title() or nil
  end
  if type(label) ~= "string" or label == "" then
    label = pane and basename(pane:get_foreground_process_name()) or nil
  end
  if type(label) ~= "string" or label == "" then
    label = "Tab " .. tostring(tab_info.index)
  end
  return {
    active = current_workspace == self.workspace:workspace_for(project) and tab_info.is_active == true,
    detail = self:project_relative_cwd(project, pane),
    id = tostring(tab:tab_id()),
    label = label,
  }
end

function Picker:host_context(window, projects)
  local open = {}
  for _, workspace in ipairs(self.wezterm.mux.get_workspace_names()) do
    open[workspace] = true
  end
  local current = window:mux_window():get_workspace()
  local context = { protocol_version = self.client.protocol_version, projects = {} }
  local project_by_workspace = {}
  for _, project in ipairs(projects) do
    local workspace = self.workspace:workspace_for(project)
    project_by_workspace[workspace] = project
    local labels = {}
    if workspace == current then
      table.insert(labels, "current")
    end
    table.insert(labels, open[workspace] and "open" or "new")
    context.projects[project.id] = { labels = labels }
  end
  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    local project = project_by_workspace[mux_window:get_workspace()]
    if project then
      for _, tab_info in ipairs(mux_window:tabs_with_info()) do
        local project_context = context.projects[project.id]
        project_context.items = project_context.items or {}
        table.insert(project_context.items, self:host_item(project, tab_info, current))
      end
    end
  end
  for session_id, tab_id in pairs(self.opencode_session_tabs) do
    local found, tab = pcall(self.wezterm.mux.get_tab, tab_id)
    if not found or not tab then
      self.opencode_session_tabs[session_id] = nil
    else
      local inspected, mux_window = pcall(function()
        return tab:window()
      end)
      local project = inspected and mux_window and project_by_workspace[mux_window:get_workspace()] or nil
      if project then
        local project_context = context.projects[project.id]
        project_context.session_items = project_context.session_items or {}
        project_context.session_items[session_id] = "tab:" .. tostring(tab_id)
      end
    end
  end
  return context
end

function Picker:close(window, tab, pane)
  local closed, close_error = pcall(function()
    tab:activate()
    window:perform_action(self.wezterm.action.CloseCurrentTab { confirm = false }, pane)
  end)
  if not closed then
    self.wezterm.log_warn("wisp could not close picker tab: " .. tostring(close_error))
  end
end

function Picker:remember_active_opencode_tab(session_id, workspace)
  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      local inspected, tab = pcall(function()
        return mux_window:active_tab()
      end)
      if inspected and tab then
        local identified, tab_id = pcall(function()
          return tab:tab_id()
        end)
        if identified and tab_id then
          self.opencode_session_tabs[session_id] = tab_id
          return
        end
      end
    end
  end
end

function Picker:open_opencode_session(window, pane, result)
  local selection = result.selection
  local project = selection.project
  if selection.host_item_id then
    local activated = self.workspace:activate_opencode_host_item(window, pane, project, selection.host_item_id)
    if activated then
      return true
    end
  end

  local command = self.client:args("open", self.wezterm.json_encode(result))
  local spawn = self.workspace:spawn_command(project, command)
  spawn.set_environment_variables.WISP_OPENCODE_SESSION_ID = selection.session_id
  local workspace = self.workspace:workspace_for(project)
  if not self.workspace:is_open(workspace) then
    window:perform_action(
      self.wezterm.action.SwitchToWorkspace {
        name = workspace,
        spawn = spawn,
      },
      pane
    )
    self.wezterm.time.call_after(0, function()
      self:remember_active_opencode_tab(selection.session_id, workspace)
    end)
    return true
  end
  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      local tab = mux_window:spawn_tab(spawn)
      self.opencode_session_tabs[selection.session_id] = tab:tab_id()
      window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
      return true
    end
  end
  return nil, "wisp could not find a mux window for workspace " .. workspace
end

function Picker:apply_result(window, pane, result, picker_pane_id)
  local valid, validation_error = self.client:validate_result(result)
  if not valid then
    return nil, validation_error
  end
  if result.status == "cancelled" then
    return true
  end
  if result.status == "error" then
    return nil, "wisp picker failed: " .. tostring(result.error)
  end
  if result.status ~= "selected" or type(result.selection) ~= "table" then
    return nil, "wisp result is not a valid selection"
  end

  local selection = result.selection
  if selection.kind == "project" then
    self.workspace:switch_to_project(window, pane, selection.project)
    return true
  end
  if selection.kind == "file" and type(selection.path) == "string" and selection.path ~= "" then
    self.workspace:open_file(window, pane, result)
    return true
  end
  if selection.kind == "close_project" then
    return self.workspace:close_project(selection.project, picker_pane_id)
  end
  if selection.kind == "host_item" then
    return self.workspace:activate_host_item(window, pane, selection.project, selection.id)
  end
  if selection.kind == "open_code_session" then
    return self:open_opencode_session(window, pane, result)
  end
  return nil, "wisp result contains an unknown selection kind"
end

function Picker:poll_result(window, original_pane, picker_tab, picker_pane, result_path, host_context_path)
  local attempts = 0
  local values = self.options:get()
  local maximum_attempts = math.ceil(values.picker_timeout_seconds / values.poll_interval_seconds)
  local picker_pane_id = picker_pane:pane_id()
  local observed_process = false

  local function picker_is_alive()
    local found, live_pane = pcall(self.wezterm.mux.get_pane, picker_pane_id)
    if not found or not live_pane then
      return false
    end
    local inspected, process = pcall(function()
      return live_pane:get_foreground_process_info()
    end)
    if inspected and process then
      observed_process = true
    elseif inspected and observed_process then
      return false
    end
    return true
  end

  local function poll()
    attempts = attempts + 1
    local file = io.open(result_path, "rb")
    if not file then
      if not picker_is_alive() then
        os.remove(host_context_path)
        self:close(window, picker_tab, picker_pane)
        self.report_error(window, "wisp picker exited before producing a result")
        return
      end
      if attempts >= maximum_attempts then
        os.remove(host_context_path)
        self:close(window, picker_tab, picker_pane)
        self.report_error(window, "wisp picker timed out before producing a result")
        return
      end
      self.wezterm.time.call_after(values.poll_interval_seconds, poll)
      return
    end

    local encoded = file:read "*a"
    file:close()
    os.remove(result_path)
    os.remove(host_context_path)
    local parsed, result = pcall(self.wezterm.json_parse, encoded)
    self:close(window, picker_tab, picker_pane)
    if not parsed then
      self.report_error(window, "wisp picker returned invalid JSON: " .. tostring(result))
      return
    end
    local applied, result_error = self:apply_result(window, original_pane, result, picker_pane_id)
    if not applied then
      self.report_error(window, result_error)
    end
  end
  self.wezterm.time.call_after(values.poll_interval_seconds, poll)
end

function Picker:launch(window, pane, initial_view)
  local projects, project_error = self.client:query_projects()
  if not projects then
    self.report_error(window, project_error)
    return
  end

  local result_path = temporary_path()
  local host_context_path = temporary_path()
  local encoded = self.wezterm.json_encode(self:host_context(window, projects))
  local written, write_error = write_file(host_context_path, encoded)
  if not written then
    self.report_error(window, "wisp could not write host context: " .. tostring(write_error))
    return
  end

  local spawned, picker_tab, picker_pane = pcall(function()
    return window:mux_window():spawn_tab {
      args = self.client:args(
        "pick",
        "--result-file",
        result_path,
        "--host-context-file",
        host_context_path,
        "--initial-view",
        initial_view
      ),
      domain = self.options:get().picker_domain,
    }
  end)
  if not spawned then
    os.remove(host_context_path)
    self.report_error(window, "wisp could not launch picker: " .. tostring(picker_tab))
    return
  end
  self:poll_result(window, pane, picker_tab, picker_pane, result_path, host_context_path)
end

return Picker
