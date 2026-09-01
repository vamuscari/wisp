local Picker = {}
Picker.__index = Picker
local CLOSE_RETRY_ATTEMPTS = 3

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

function Picker.new(wezterm, options, client, workspace, popup, report_error)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
    workspace = workspace,
    popup = popup,
    report_error = report_error,
    opencode_session_tabs = {},
  }, Picker)
end

function Picker:project_relative_cwd(project, pane)
  local cwd = pane and pane:get_current_working_dir()
  local cwd_path = self.workspace:path_from_file_url(cwd)
  if not cwd_path then
    return nil
  end
  local root, root_identity = self.workspace:normalize_path(project.path)
  local path, path_identity = self.workspace:normalize_path(cwd_path)
  if path_identity == root_identity then
    return "."
  end
  local prefix = root_identity
  if prefix:sub(-1) ~= "/" then
    prefix = prefix .. "/"
  end
  if path_identity:sub(1, #prefix) == prefix then
    return path:sub(#prefix + 1)
  end
  return nil
end

function Picker:active_nvim_file(pane)
  for _, view in ipairs(self.client:nvim_views(pane) or {}) do
    if view.active == true then
      return view.path
    end
  end
  return nil
end

function Picker:host_pane(project, pane_info)
  local pane = pane_info.pane
  local identified, pane_id = pcall(function()
    return pane:pane_id()
  end)
  if not identified or pane_id == nil then
    return nil
  end
  local label = pane:get_title()
  if type(label) ~= "string" or label == "" then
    label = basename(pane:get_foreground_process_name())
  end
  if type(label) ~= "string" or label == "" then
    label = "Pane " .. tostring(pane_info.index)
  end
  local host_pane = {
    active = pane_info.is_active == true,
    detail = project and self:project_relative_cwd(project, pane) or nil,
    id = tostring(pane_id),
    label = label,
  }
  local nvim_views = self.client:nvim_views(pane)
  if nvim_views and #nvim_views > 0 then
    host_pane.nvim_views = nvim_views
  end
  return host_pane
end

function Picker:host_window(project, workspace, tab_info, current_workspace)
  local tab = tab_info.tab
  local active_pane = tab:active_pane()
  local label = tab:get_title()
  if type(label) ~= "string" or label == "" then
    label = active_pane and active_pane:get_title() or nil
  end
  if type(label) ~= "string" or label == "" then
    label = active_pane and basename(active_pane:get_foreground_process_name()) or nil
  end
  if type(label) ~= "string" or label == "" then
    label = "Window " .. tostring(tab_info.index)
  end
  local inspected, pane_infos = pcall(function()
    return tab:panes_with_info()
  end)
  if not inspected or type(pane_infos) ~= "table" then
    pane_infos = active_pane and { { index = 0, is_active = true, pane = active_pane } } or {}
  end
  local panes = {}
  for _, pane_info in ipairs(pane_infos) do
    local pane = self:host_pane(project, pane_info)
    if pane then
      table.insert(panes, pane)
    end
  end
  if #panes == 0 then
    return nil
  end
  return {
    active = current_workspace == workspace and tab_info.is_active == true,
    detail = project and self:project_relative_cwd(project, active_pane) or nil,
    id = tostring(tab:tab_id()),
    label = label,
    panes = panes,
  }
end

function Picker:host_context(window, projects)
  local open = {}
  local workspace_names = self.wezterm.mux.get_workspace_names()
  for _, workspace in ipairs(workspace_names) do
    open[workspace] = true
  end
  local current = window:mux_window():get_workspace()
  local context = { protocol_version = self.client.protocol_version, projects = {}, workspaces = {} }
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
  local context_by_workspace = {}
  for _, workspace in ipairs(workspace_names) do
    if not project_by_workspace[workspace] then
      local workspace_context = {
        current = workspace == current,
      }
      context.workspaces[workspace] = workspace_context
      context_by_workspace[workspace] = workspace_context
    end
  end
  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    local workspace = mux_window:get_workspace()
    local project = project_by_workspace[workspace]
    if project then
      for _, tab_info in ipairs(mux_window:tabs_with_info()) do
        local project_context = context.projects[project.id]
        local host_window = self:host_window(project, workspace, tab_info, current)
        if host_window then
          project_context.windows = project_context.windows or {}
          table.insert(project_context.windows, host_window)
        end
      end
    else
      local workspace_context = context_by_workspace[workspace]
      if workspace_context then
        for _, tab_info in ipairs(mux_window:tabs_with_info()) do
          local host_window = self:host_window(nil, workspace, tab_info, current)
          if host_window then
            workspace_context.windows = workspace_context.windows or {}
            table.insert(workspace_context.windows, host_window)
          end
        end
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
  local identified, tab_id = pcall(function()
    return tab:tab_id()
  end)
  local closed, close_error = pcall(function()
    tab:activate()
    window:perform_action(self.wezterm.action.CloseCurrentTab { confirm = false }, pane)
  end)
  if not closed then
    if identified then
      local inspected, live_tab = pcall(self.wezterm.mux.get_tab, tab_id)
      if inspected and not live_tab then
        return true
      end
    end
    self.wezterm.log_warn("wisp could not close picker tab: " .. tostring(close_error))
    return nil, close_error
  end
  return true
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
    return self.workspace:open_file(window, pane, result)
  end
  if selection.kind == "close_project" then
    return self.workspace:close_project(selection.project, picker_pane_id)
  end
  if selection.kind == "host_pane" then
    return self.workspace:activate_host_pane(window, pane, selection.project, selection.window_id, selection.pane_id)
  end
  if selection.kind == "workspace" then
    return self.workspace:activate_workspace(selection.workspace)
  end
  if selection.kind == "workspace_pane" then
    return self.workspace:activate_workspace_pane(selection.workspace, selection.window_id, selection.pane_id)
  end
  if selection.kind == "close_workspace" then
    return self.workspace:close_workspace(selection.workspace, picker_pane_id)
  end
  if selection.kind == "open_code_session" then
    return self:open_opencode_session(window, pane, result)
  end
  return nil, "wisp result contains an unknown selection kind"
end

function Picker:poll_result(window, original_pane, owner, result_path, host_context_path, preview)
  local attempts = 0
  local values = self.options:get()
  local maximum_attempts = math.ceil(values.picker_timeout_seconds / values.poll_interval_seconds)
  local picker_pane = owner.pane
  local picker_pane_id = picker_pane:pane_id()
  local observed_process = false

  local function remove_paths()
    os.remove(result_path)
    os.remove(host_context_path)
    if preview then
      os.remove(preview.path)
    end
  end

  local function block_preview(message)
    preview.blocked = true
    if not preview.failure_reported then
      preview.failure_reported = true
      self.report_error(window, message .. "; toggle file preview off and on to retry")
    end
  end

  local function preview_is_alive()
    if not preview or not preview.pane_id then
      return false
    end
    local found, live_pane = pcall(self.wezterm.mux.get_pane, preview.pane_id)
    if found and live_pane then
      return true
    end
    preview.pane = nil
    preview.pane_id = nil
    block_preview "wisp file preview exited unexpectedly"
    return false
  end

  local function poll_preview()
    if not preview then
      return true
    end
    if preview.pane_id then
      preview_is_alive()
    end
    local file = io.open(preview.path, "rb")
    if not file then
      return true
    end
    local encoded = file:read "*a"
    file:close()
    local envelope = self.client:parse_file_preview(encoded, preview.sequence)
    if not envelope then
      return true
    end
    preview.sequence = envelope.sequence
    local state = envelope.state
    if state.state == "hidden" then
      preview.visible = false
      preview.blocked = false
      preview.failure_reported = false
      if preview.pane_id then
        local closed, close_error = self.workspace:close_pane(preview.pane_id)
        if not closed then
          return nil, "wisp could not close file preview: " .. tostring(close_error)
        end
        preview.pane = nil
        preview.pane_id = nil
      end
      return true
    end

    preview.visible = true
    if preview.pane_id or preview.blocked then
      return true
    end
    local args = {}
    for _, argument in ipairs(preview.config.command) do
      table.insert(args, argument)
    end
    for _, argument in ipairs { "-n", "-i", "NONE", "-S", preview.config.script } do
      table.insert(args, argument)
    end
    local command = {
      args = args,
      direction = preview.config.direction,
      size = preview.config.size,
      set_environment_variables = { WISP_FILE_PREVIEW_STATE_FILE = preview.path },
    }
    if state.state == "file" then
      command.cwd = state.project.path
    end
    local spawned, preview_pane = pcall(function()
      return owner.pane:split(command)
    end)
    if not spawned or not preview_pane then
      block_preview("wisp could not open file preview: " .. tostring(preview_pane))
      return true
    end
    local identified, pane_id = pcall(function()
      return preview_pane:pane_id()
    end)
    if not identified or pane_id == nil then
      block_preview "wisp file preview has no stable pane ID"
      return true
    end
    preview.pane = preview_pane
    preview.pane_id = pane_id
    return true
  end

  local function after_close(callback)
    local close_attempts = 0
    local function attempt_close()
      close_attempts = close_attempts + 1
      local called, closed, close_error = pcall(owner.close, owner)
      if not called then
        close_error = closed
        closed = false
      end
      if closed then
        callback()
        return
      end
      if close_attempts >= CLOSE_RETRY_ATTEMPTS then
        self.report_error(window, "wisp could not close picker: " .. tostring(close_error))
        return
      end
      self.wezterm.time.call_after(values.poll_interval_seconds, attempt_close)
    end
    attempt_close()
  end

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
    if owner.closed then
      remove_paths()
      return
    end
    if owner.is_current and not owner:is_current() then
      remove_paths()
      after_close(function() end)
      return
    end
    local previewed, preview_error = poll_preview()
    if not previewed then
      remove_paths()
      after_close(function()
        self.report_error(window, preview_error)
      end)
      return
    end
    local file = io.open(result_path, "rb")
    if not file then
      if not picker_is_alive() then
        remove_paths()
        after_close(function()
          self.report_error(window, "wisp picker exited before producing a result")
        end)
        return
      end
      if attempts >= maximum_attempts then
        remove_paths()
        after_close(function()
          self.report_error(window, "wisp picker timed out before producing a result")
        end)
        return
      end
      self.wezterm.time.call_after(values.poll_interval_seconds, poll)
      return
    end

    local encoded = file:read "*a"
    file:close()
    remove_paths()
    local result = self.client:parse_json(encoded)
    after_close(function()
      if not result then
        self.report_error(window, "wisp picker returned invalid JSON")
        return
      end
      local applied, result_error = self:apply_result(window, original_pane, result, picker_pane_id)
      if not applied then
        self.report_error(window, result_error)
      end
    end)
  end
  self.wezterm.time.call_after(values.poll_interval_seconds, poll)
end

function Picker:launch(window, pane, initial_view, surface)
  local projects, project_error = self.client:query_projects()
  if not projects then
    self.report_error(window, project_error)
    return
  end

  local result_path = temporary_path()
  local host_context_path = temporary_path()
  local preview
  local values = self.options:get()
  if values.file_preview then
    preview = {
      blocked = false,
      config = values.file_preview,
      path = temporary_path(),
      sequence = -1,
      visible = false,
    }
  end
  local encoded = self.wezterm.json_encode(self:host_context(window, projects))
  local written, write_error = write_file(host_context_path, encoded)
  if not written then
    os.remove(result_path)
    if preview then
      os.remove(preview.path)
    end
    self.report_error(window, "wisp could not write host context: " .. tostring(write_error))
    return
  end

  local picker_args = self.client:args(
    "pick",
    "--result-file",
    result_path,
    "--host-context-file",
    host_context_path,
    "--initial-view",
    initial_view
  )
  table.insert(picker_args, "--single-pane-behavior")
  table.insert(picker_args, values.single_pane_behavior)
  table.insert(picker_args, "--file-open-target")
  table.insert(picker_args, (values.file_open.default:gsub("_", "-")))
  table.insert(picker_args, "--wezterm-executable")
  table.insert(picker_args, self.workspace:wezterm_executable())
  if values.window_preview then
    table.insert(picker_args, "--window-preview")
  end
  if preview then
    table.insert(picker_args, "--file-preview-state-file")
    table.insert(picker_args, preview.path)
    table.insert(picker_args, "--file-preview")
  end
  local active_file = self:active_nvim_file(pane)
  if active_file then
    table.insert(picker_args, "--active-file")
    table.insert(picker_args, active_file)
  end

  local owner
  local launch_error
  if surface == "popup" then
    owner, launch_error = self.popup:open(window, pane, {
      id = initial_view,
      args = picker_args,
      domain = values.picker_domain,
    })
  else
    local spawned, picker_tab, picker_pane = pcall(function()
      return window:mux_window():spawn_tab {
        args = picker_args,
        domain = values.picker_domain,
      }
    end)
    if spawned then
      owner = { pane = picker_pane, closed = false }
      function owner:close()
        if self.closed then
          return true
        end
        local closed, close_error = Picker.close(self.picker, self.window, self.tab, self.pane)
        if not closed then
          return nil, close_error
        end
        self.closed = true
        return true
      end
      owner.picker = self
      owner.window = window
      owner.tab = picker_tab
    else
      launch_error = picker_tab
    end
  end
  if not owner then
    os.remove(result_path)
    os.remove(host_context_path)
    if preview then
      os.remove(preview.path)
    end
    self.report_error(window, "wisp could not launch picker: " .. tostring(launch_error))
    return
  end
  if preview then
    local close_surface = owner.close
    function owner:close()
      if preview.pane_id then
        local closed, close_error = self.picker.workspace:close_pane(preview.pane_id)
        if not closed then
          return nil, close_error
        end
        preview.pane = nil
        preview.pane_id = nil
      end
      os.remove(preview.path)
      return close_surface(self)
    end
    owner.picker = self
  end
  self:poll_result(window, pane, owner, result_path, host_context_path, preview)
end

function Picker:launch_popup(window, pane, initial_view)
  if initial_view ~= "projects" and initial_view ~= "windows" and initial_view ~= "sessions" then
    error("wisp has no popup action " .. tostring(initial_view))
  end
  self:launch(window, pane, initial_view, "popup")
end

return Picker
