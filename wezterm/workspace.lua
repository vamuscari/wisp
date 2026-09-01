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

function Workspace:normalize_path(path)
  if type(path) ~= "string" then
    return nil, nil
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
  local identity = normalized
  if windows_drive or windows_unc then
    identity = identity:gsub("[A-Z]", function(character)
      return string.char(character:byte() + 32)
    end)
  end
  if type(self.wezterm.target_triple) == "string" and self.wezterm.target_triple:match "windows" then
    normalized = normalized:gsub("/", "\\")
  end
  return normalized, identity
end

function Workspace:path_from_file_url(url)
  if not url or url.scheme ~= "file" or type(url.file_path) ~= "string" then
    return nil
  end
  if not (type(self.wezterm.target_triple) == "string" and self.wezterm.target_triple:match "windows") then
    return url.file_path
  end

  local path = url.file_path
  local drive_path = path:match "^[/\\]%a:[/\\]"
  if drive_path then
    path = path:sub(2)
  elseif type(url.host) == "string" and url.host ~= "" then
    path = "\\\\" .. url.host .. "\\" .. path:gsub("^[/\\]+", "")
  end
  return self:normalize_path(path)
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
  local workspace = self:workspace_for(project)

  if selection.reuse_existing and selection.host_target then
    local pane_id = tonumber(selection.host_target.pane_id)
    local tab_id = tonumber(selection.host_target.window_id)
    local inspected, target = pcall(function()
      local live_target = self.wezterm.mux.get_pane(pane_id)
      if not live_target or not pane_id or not tab_id then
        return nil
      end
      local tab = live_target:tab()
      local mux_window = live_target:window()
      local contains_file = false
      local _, selected_identity = self:normalize_path(selection.path)
      for _, view in ipairs(self.client:nvim_views(live_target) or {}) do
        local _, view_identity = self:normalize_path(view.path)
        if selected_identity ~= nil and view_identity == selected_identity then
          contains_file = true
          break
        end
      end
      if
        tab
        and tab:tab_id() == tab_id
        and mux_window
        and mux_window:get_workspace() == workspace
        and contains_file
      then
        return live_target
      end
      return nil
    end)
    if inspected and target then
      local activated = pcall(function()
        target:activate()
        window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
      end)
      if activated then
        return true
      end
    end
  end

  if not self.client:valid_argv(selection.opener) then
    return nil, "wisp selected file has no valid opener; configure openers.file in Wisp TOML"
  end
  local command = self.client:args("open", self.wezterm.json_encode(result))

  local function create_first_window()
    local switched, switch_error = pcall(function()
      window:perform_action(
        self.wezterm.action.SwitchToWorkspace {
          name = workspace,
          spawn = self:spawn_command(project, command),
        },
        pane
      )
    end)
    if not switched then
      return nil, "wisp could not open file in workspace " .. workspace .. ": " .. tostring(switch_error)
    end
    return true
  end

  if not self:is_open(workspace) then
    return create_first_window()
  end

  for _, mux_window in ipairs(self.wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      local opened, open_error = pcall(function()
        if selection.open_target == "window" then
          mux_window:spawn_tab(self:spawn_command(project, command))
        else
          local tab = mux_window:active_tab()
          local active_pane = tab and tab:active_pane() or nil
          if not active_pane then
            error "project mux window has no active pane"
          end
          local split = self:spawn_command(project, command)
          split.direction = selection.open_target == "right_pane" and "Right" or "Bottom"
          local opened_pane = active_pane:split(split)
          if not opened_pane then
            error "pane split returned no pane"
          end
          opened_pane:activate()
        end
        window:perform_action(self.wezterm.action.SwitchToWorkspace { name = workspace }, pane)
      end)
      if not opened then
        return nil, "wisp could not open file in workspace " .. workspace .. ": " .. tostring(open_error)
      end
      return true
    end
  end
  return create_first_window()
end

function Workspace:wezterm_executable()
  local name = type(self.wezterm.target_triple) == "string"
      and self.wezterm.target_triple:match "windows"
      and "wezterm.exe"
    or "wezterm"
  return self.wezterm.executable_dir .. "/" .. name
end

function Workspace:close_pane(pane_id)
  local success, stdout, stderr = self.wezterm.run_child_process {
    self:wezterm_executable(),
    "cli",
    "kill-pane",
    "--pane-id",
    tostring(pane_id),
  }
  if not success then
    local inspected, pane = pcall(self.wezterm.mux.get_pane, pane_id)
    if inspected and not pane then
      return true
    end
    local message = stderr ~= "" and stderr or stdout
    return nil, "wisp could not close pane " .. tostring(pane_id) .. ": " .. tostring(message)
  end
  return true
end

function Workspace:close_workspace(workspace, ignored_pane_id)
  if type(workspace) ~= "string" or workspace == "" then
    return nil, "wisp result contains an invalid workspace"
  end
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
    local success, close_error = self:close_pane(pane_id)
    if not success then
      table.insert(failures, close_error)
    end
  end
  if #failures > 0 then
    return nil, "wisp could not close every pane in " .. workspace .. ": " .. table.concat(failures, "; ")
  end
  return true
end

function Workspace:close_project(project, ignored_pane_id)
  return self:close_workspace(self:workspace_for(project), ignored_pane_id)
end

function Workspace:activate_workspace(workspace)
  if type(workspace) ~= "string" or workspace == "" then
    return nil, "wisp result contains an invalid workspace"
  end
  local activated, activate_error = pcall(self.wezterm.mux.set_active_workspace, workspace)
  if not activated then
    return nil, "wisp could not activate workspace " .. workspace .. ": " .. tostring(activate_error)
  end
  return true
end

function Workspace:activate_pane_target(workspace, window_id, pane_id)
  if type(workspace) ~= "string" or workspace == "" then
    return nil, "wisp result contains an invalid workspace"
  end
  local tab_id = type(window_id) == "string" and tonumber(window_id) or nil
  local target_pane_id = type(pane_id) == "string" and tonumber(pane_id) or nil
  if not tab_id or tab_id % 1 ~= 0 or not target_pane_id or target_pane_id % 1 ~= 0 then
    return nil, "wisp result contains an invalid host pane target"
  end
  local found, target = pcall(self.wezterm.mux.get_pane, target_pane_id)
  if not found or not target then
    return nil, "wisp selected pane " .. tostring(pane_id) .. " no longer exists"
  end
  local inspected_tab, tab = pcall(function()
    return target:tab()
  end)
  if not inspected_tab or not tab or tab:tab_id() ~= tab_id then
    return nil, "wisp selected pane " .. tostring(pane_id) .. " no longer belongs to window " .. tostring(window_id)
  end
  local inspected_window, mux_window = pcall(function()
    return target:window()
  end)
  if not inspected_window or not mux_window or mux_window:get_workspace() ~= workspace then
    return nil, "wisp selected pane " .. tostring(pane_id) .. " no longer belongs to workspace " .. workspace
  end
  local activated, activate_error = pcall(function()
    target:activate()
  end)
  if not activated then
    return nil, "wisp could not activate pane " .. tostring(pane_id) .. ": " .. tostring(activate_error)
  end
  return true
end

function Workspace:activate_workspace_pane(workspace, window_id, pane_id)
  local activated, activate_error = self:activate_pane_target(workspace, window_id, pane_id)
  if not activated then
    return nil, activate_error
  end
  return self:activate_workspace(workspace)
end

function Workspace:activate_project_tab(window, pane, project, id)
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

function Workspace:activate_host_pane(window, pane, project, window_id, pane_id)
  local workspace = self:workspace_for(project)
  local activated, activate_error = self:activate_pane_target(workspace, window_id, pane_id)
  if not activated then
    return nil, activate_error
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
    return self:activate_project_tab(window, pane, project, value)
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
  local cwd = self:path_from_file_url(pane:get_current_working_dir())
  local same_domain = not project or command.domain.DomainName == pane:get_domain_name()
  if same_domain and cwd then
    command.cwd = cwd
  end
  return command
end

return Workspace
