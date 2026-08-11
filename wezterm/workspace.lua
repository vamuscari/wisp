local Workspace = {}
Workspace.__index = Workspace

function Workspace.new(wezterm, options, client, report_error)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
    report_error = report_error,
  }, Workspace)
end

function Workspace:workspace_for(project)
  return self.options:workspace_for(project)
end

function Workspace:spawn_command(project, args)
  local command = {
    cwd = project.path,
    domain = self.options:domain_for(project),
    set_environment_variables = {
      WISP_PROJECT_DIR = project.path,
      WISP_PROJECT_NAME = project.name,
    },
  }
  if args then
    command.args = args
  end
  return command
end

function Workspace:switch_to_project(window, pane, project)
  window:perform_action(
    self.wezterm.action.SwitchToWorkspace {
      name = self:workspace_for(project),
      spawn = self:spawn_command(project),
    },
    pane
  )
end

function Workspace:is_open(workspace)
  for _, active in ipairs(self.wezterm.mux.get_workspace_names()) do
    if active == workspace then
      return true
    end
  end
  return false
end

function Workspace:open_file(window, pane, result)
  local selection = result.selection
  local project = selection.project
  if not self.client:valid_argv(selection.opener) then
    self.report_error(window, "wisp selected file has no valid opener; configure openers.file in Wisp TOML")
    return
  end
  local command = self.client:args("open", self.wezterm.json_encode(result))

  local workspace = self:workspace_for(project)
  if not self:is_open(workspace) then
    window:perform_action(
      self.wezterm.action.SwitchToWorkspace {
        name = workspace,
        spawn = self:spawn_command(project, command),
      },
      pane
    )
    return
  end

  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      mux_window:spawn_tab(self:spawn_command(project, command))
      window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
      return
    end
  end
  self.report_error(window, "wisp could not find a mux window for workspace " .. workspace)
end

function Workspace:wezterm_executable()
  local name = type(self.wezterm.target_triple) == "string"
      and self.wezterm.target_triple:match "windows"
      and "wezterm.exe"
    or "wezterm"
  return self.wezterm.executable_dir .. "/" .. name
end

function Workspace:close_project(project, ignored_pane_id)
  local workspace = self:workspace_for(project)
  local pane_ids = {}
  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      for _, tab in ipairs(mux_window:tabs()) do
        for _, pane in ipairs(tab:panes()) do
          local pane_id = pane:pane_id()
          if pane_id ~= ignored_pane_id then
            table.insert(pane_ids, pane_id)
          end
        end
      end
    end
  end
  if #pane_ids == 0 then
    return nil, "wisp could not find open panes for workspace " .. workspace
  end

  local failures = {}
  for _, pane_id in ipairs(pane_ids) do
    local success, stdout, stderr = self.wezterm.run_child_process {
      self:wezterm_executable(),
      "cli",
      "kill-pane",
      "--pane-id",
      tostring(pane_id),
    }
    if not success then
      local message = stderr ~= "" and stderr or stdout
      table.insert(failures, tostring(message))
    end
  end
  if #failures > 0 then
    return nil, "wisp could not close every pane in " .. workspace .. ": " .. table.concat(failures, "; ")
  end
  return true
end

function Workspace:activate_host_item(window, pane, project, id)
  local tab_id = type(id) == "string" and tonumber(id) or nil
  if not tab_id or tab_id % 1 ~= 0 then
    return nil, "wisp result contains an invalid host item ID"
  end
  local found, tab = pcall(self.wezterm.mux.get_tab, tab_id)
  if not found or not tab then
    return nil, "wisp selected tab " .. tostring(id) .. " no longer exists"
  end
  local workspace = self:workspace_for(project)
  local inspected, mux_window = pcall(function()
    return tab:window()
  end)
  if not inspected or not mux_window or mux_window:get_workspace() ~= workspace then
    return nil, "wisp selected tab " .. tostring(id) .. " no longer belongs to workspace " .. workspace
  end
  local activated, activate_error = pcall(function()
    tab:activate()
  end)
  if not activated then
    return nil, "wisp could not activate tab " .. tostring(id) .. ": " .. tostring(activate_error)
  end
  window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
  return true
end

function Workspace:activate_opencode_host_item(window, pane, project, id)
  if type(id) ~= "string" or id == "" then
    return nil, "wisp result contains an invalid OpenCode host item ID"
  end
  local kind, value = id:match "^(%a+):(.+)$"
  if kind == "tab" then
    return self:activate_host_item(window, pane, project, value)
  end
  if kind ~= "pane" then
    return nil, "wisp result contains an invalid OpenCode host item ID"
  end
  local pane_id = tonumber(value)
  if not pane_id or pane_id % 1 ~= 0 then
    return nil, "wisp result contains an invalid OpenCode pane ID"
  end
  local found, target = pcall(self.wezterm.mux.get_pane, pane_id)
  if not found or not target then
    return nil, "wisp selected OpenCode pane " .. value .. " no longer exists"
  end
  local workspace = self:workspace_for(project)
  local inspected, mux_window = pcall(function()
    return target:window()
  end)
  if not inspected or not mux_window or mux_window:get_workspace() ~= workspace then
    return nil, "wisp selected OpenCode pane " .. value .. " no longer belongs to workspace " .. workspace
  end
  local activated = pcall(function()
    target:activate()
  end)
  if not activated then
    return nil, "wisp could not activate OpenCode pane " .. value
  end
  window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
  return true
end

function Workspace:current_spawn_command(window, pane)
  local projects, project_error = self.client:query_projects()
  if not projects then
    self.wezterm.log_error(project_error)
    projects = {}
  end
  local project
  local workspace = window:mux_window():get_workspace()
  for _, candidate in ipairs(projects) do
    if self:workspace_for(candidate) == workspace then
      project = candidate
      break
    end
  end

  local command = project and self:spawn_command(project) or { domain = "CurrentPaneDomain" }
  local cwd = pane:get_current_working_dir()
  local same_domain = not project or command.domain.DomainName == pane:get_domain_name()
  if same_domain and cwd and cwd.scheme == "file" then
    command.cwd = cwd.file_path
  end
  return command
end

return Workspace
