local wezterm = require "wezterm"

local wisp = {}
local options = {}

local function validate_domain(domain, label)
  if type(domain) ~= "table" or type(domain.DomainName) ~= "string" or domain.DomainName == "" then
    error("wisp " .. label .. " must be a non-empty { DomainName = name } table")
  end
end

local function validate_options(configured)
  if type(configured) ~= "table" then
    error "wisp options must be a table"
  end

  for _, moved in ipairs { "roots", "projects", "cache_ttl_seconds", "open_file" } do
    if configured[moved] ~= nil then
      error("wisp " .. moved .. " moved to shared Wisp TOML configuration")
    end
  end

  local allowed = {
    config_file = true,
    domain_for_project = true,
    picker_binding = true,
    picker_domain = true,
    picker_timeout_seconds = true,
    poll_interval_seconds = true,
    spawn_domain = true,
    wisp_path = true,
    workspace_for_project = true,
    workspace_prefix = true,
  }
  for key in pairs(configured) do
    if not allowed[key] then
      error("wisp unknown option " .. tostring(key))
    end
  end

  if configured.wisp_path ~= nil and (type(configured.wisp_path) ~= "string" or configured.wisp_path == "") then
    error "wisp wisp_path must be a non-empty string"
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
  for _, field in ipairs { "poll_interval_seconds", "picker_timeout_seconds" } do
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
end

local function configure(configured)
  local spawn_domain = configured.spawn_domain or { DomainName = "local" }
  options = {
    config_file = configured.config_file,
    domain_for_project = configured.domain_for_project,
    picker_binding = configured.picker_binding,
    picker_domain = configured.picker_domain or spawn_domain,
    picker_timeout_seconds = configured.picker_timeout_seconds or 3600,
    poll_interval_seconds = configured.poll_interval_seconds or 0.05,
    spawn_domain = spawn_domain,
    wisp_path = configured.wisp_path or "wisp",
    workspace_for_project = configured.workspace_for_project,
    workspace_prefix = configured.workspace_prefix or "wisp:",
  }
end

local function wisp_args(...)
  local args = { options.wisp_path }
  if options.config_file then
    table.insert(args, "--config")
    table.insert(args, options.config_file)
  end
  for index = 1, select("#", ...) do
    local argument = select(index, ...)
    table.insert(args, argument)
  end
  return args
end

local function run_child(...)
  local args = wisp_args(...)
  local success, stdout, stderr = wezterm.run_child_process(args)
  if not success then
    local message = stderr ~= "" and stderr or stdout
    return nil, "wisp command failed: " .. tostring(message)
  end
  return stdout
end

local function valid_project(project)
  return type(project) == "table"
    and type(project.id) == "string"
    and project.id ~= ""
    and type(project.path) == "string"
    and project.path ~= ""
    and type(project.group) == "string"
    and type(project.name) == "string"
end

local function query_projects()
  local stdout, command_error = run_child("projects", "--json")
  if not stdout then
    return nil, command_error
  end
  local parsed, projects = pcall(wezterm.json_parse, stdout)
  if not parsed or type(projects) ~= "table" then
    return nil, "wisp projects returned invalid JSON"
  end
  for index, project in ipairs(projects) do
    if not valid_project(project) then
      return nil, "wisp projects returned an invalid project at index " .. index
    end
  end
  return projects
end

local function workspace_for(project)
  local workspace
  if options.workspace_for_project then
    workspace = options.workspace_for_project(project)
  else
    workspace = options.workspace_prefix .. project.group .. "/" .. project.name
  end
  if type(workspace) ~= "string" or workspace == "" then
    error("wisp workspace_for_project returned an invalid workspace for " .. project.id)
  end
  return workspace
end

local function domain_for(project)
  local domain = options.domain_for_project and options.domain_for_project(project) or options.spawn_domain
  validate_domain(domain, "domain_for_project result")
  return domain
end

local function spawn_command(project, args)
  local command = {
    cwd = project.path,
    domain = domain_for(project),
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

local function switch_to_project(window, pane, project)
  window:perform_action(
    wezterm.action.SwitchToWorkspace {
      name = workspace_for(project),
      spawn = spawn_command(project),
    },
    pane
  )
end

local function workspace_is_open(workspace)
  for _, active in ipairs(wezterm.mux.get_workspace_names()) do
    if active == workspace then
      return true
    end
  end
  return false
end

local function valid_argv(argv)
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

local function open_file(window, pane, project, opener)
  if not valid_argv(opener) then
    wezterm.log_error "wisp selected file has no valid opener; configure openers.file in Wisp TOML"
    return
  end

  local workspace = workspace_for(project)
  if not workspace_is_open(workspace) then
    window:perform_action(
      wezterm.action.SwitchToWorkspace {
        name = workspace,
        spawn = spawn_command(project, opener),
      },
      pane
    )
    return
  end

  for _, mux_window in ipairs(wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == workspace then
      mux_window:spawn_tab(spawn_command(project, opener))
      window:perform_action(wezterm.action.SwitchToWorkspace { name = workspace }, pane)
      return
    end
  end
  wezterm.log_error("wisp could not find a mux window for workspace " .. workspace)
end

local function wezterm_executable()
  local name = type(wezterm.target_triple) == "string" and wezterm.target_triple:match "windows" and "wezterm.exe"
    or "wezterm"
  return wezterm.executable_dir .. "/" .. name
end

local function close_project(project, ignored_pane_id)
  local workspace = workspace_for(project)
  local pane_ids = {}
  for _, mux_window in ipairs(wezterm.mux.all_windows()) do
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
    local success, stdout, stderr = wezterm.run_child_process {
      wezterm_executable(),
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

local function activate_host_item(window, pane, project, id)
  local tab_id = type(id) == "string" and tonumber(id) or nil
  if not tab_id or tab_id % 1 ~= 0 then
    return nil, "wisp result contains an invalid host item ID"
  end
  local found, tab = pcall(wezterm.mux.get_tab, tab_id)
  if not found or not tab then
    return nil, "wisp selected tab " .. tostring(id) .. " no longer exists"
  end
  local workspace = workspace_for(project)
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
  window:perform_action(wezterm.action.SwitchToWorkspace { name = workspace }, pane)
  return true
end

local function basename(path)
  return type(path) == "string" and path:match "([^/\\]+)$" or nil
end

local function project_relative_cwd(project, pane)
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

local function host_item(project, tab_info, current_workspace)
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
    active = current_workspace == workspace_for(project) and tab_info.is_active == true,
    detail = project_relative_cwd(project, pane),
    id = tostring(tab:tab_id()),
    label = label,
  }
end

local function host_context(window, projects)
  local open = {}
  for _, workspace in ipairs(wezterm.mux.get_workspace_names()) do
    open[workspace] = true
  end
  local current = window:active_workspace()
  local context = { protocol_version = 2, projects = {} }
  local project_by_workspace = {}
  for _, project in ipairs(projects) do
    local workspace = workspace_for(project)
    project_by_workspace[workspace] = project
    local labels = {}
    if workspace == current then
      table.insert(labels, "current")
    end
    table.insert(labels, open[workspace] and "open" or "new")
    context.projects[project.id] = { labels = labels }
  end
  for _, mux_window in ipairs(wezterm.mux.all_windows()) do
    local project = project_by_workspace[mux_window:get_workspace()]
    if project then
      for _, tab_info in ipairs(mux_window:tabs_with_info()) do
        local project_context = context.projects[project.id]
        project_context.items = project_context.items or {}
        table.insert(project_context.items, host_item(project, tab_info, current))
      end
    end
  end
  return context
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

local function close_picker(window, tab, pane)
  local closed, close_error = pcall(function()
    tab:activate()
    window:perform_action(wezterm.action.CloseCurrentTab { confirm = false }, pane)
  end)
  if not closed then
    wezterm.log_warn("wisp could not close picker tab: " .. tostring(close_error))
  end
end

local function apply_result(window, pane, result, picker_pane_id)
  if type(result) ~= "table" or result.protocol_version ~= 2 then
    return nil, "wisp result has an unsupported protocol version"
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
  if not valid_project(selection.project) then
    return nil, "wisp result contains an invalid project"
  end
  if selection.kind == "project" then
    switch_to_project(window, pane, selection.project)
    return true
  end
  if selection.kind == "file" and type(selection.path) == "string" and selection.path ~= "" then
    open_file(window, pane, selection.project, selection.opener)
    return true
  end
  if selection.kind == "close_project" then
    return close_project(selection.project, picker_pane_id)
  end
  if selection.kind == "host_item" then
    return activate_host_item(window, pane, selection.project, selection.id)
  end
  return nil, "wisp result contains an unknown selection kind"
end

local function poll_result(window, original_pane, picker_tab, picker_pane, result_path, host_context_path)
  local attempts = 0
  local maximum_attempts = math.ceil(options.picker_timeout_seconds / options.poll_interval_seconds)
  local picker_pane_id = picker_pane:pane_id()
  local observed_process = false

  local function picker_is_alive()
    local found, live_pane = pcall(wezterm.mux.get_pane, picker_pane_id)
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
        close_picker(window, picker_tab, picker_pane)
        wezterm.log_error "wisp picker exited before producing a result"
        return
      end
      if attempts >= maximum_attempts then
        os.remove(host_context_path)
        close_picker(window, picker_tab, picker_pane)
        wezterm.log_error "wisp picker timed out before producing a result"
        return
      end
      wezterm.time.call_after(options.poll_interval_seconds, poll)
      return
    end

    local encoded = file:read "*a"
    file:close()
    os.remove(result_path)
    os.remove(host_context_path)
    local parsed, result = pcall(wezterm.json_parse, encoded)
    close_picker(window, picker_tab, picker_pane)
    if not parsed then
      wezterm.log_error("wisp picker returned invalid JSON: " .. tostring(result))
      return
    end
    local applied, result_error = apply_result(window, original_pane, result, picker_pane_id)
    if not applied then
      wezterm.log_error(result_error)
    end
  end
  wezterm.time.call_after(options.poll_interval_seconds, poll)
end

local function launch_picker(window, pane, initial_view)
  local projects, project_error = query_projects()
  if not projects then
    wezterm.log_error(project_error)
    return
  end

  local result_path = temporary_path()
  local host_context_path = temporary_path()
  local encoded = wezterm.json_encode(host_context(window, projects))
  local written, write_error = write_file(host_context_path, encoded)
  if not written then
    wezterm.log_error("wisp could not write host context: " .. tostring(write_error))
    return
  end

  local spawned, picker_tab, picker_pane = pcall(function()
    return window:mux_window():spawn_tab {
      args = wisp_args(
        "pick",
        "--result-file",
        result_path,
        "--host-context-file",
        host_context_path,
        "--initial-view",
        initial_view
      ),
      domain = options.picker_domain,
    }
  end)
  if not spawned then
    os.remove(host_context_path)
    wezterm.log_error("wisp could not launch picker: " .. tostring(picker_tab))
    return
  end
  poll_result(window, pane, picker_tab, picker_pane, result_path, host_context_path)
end

local function safely(callback)
  local completed, callback_error = pcall(callback)
  if not completed then
    wezterm.log_error("wisp adapter failed: " .. tostring(callback_error))
  end
end

function wisp.project_picker_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      launch_picker(window, pane, "projects")
    end)
  end)
end

function wisp.window_picker_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      launch_picker(window, pane, "windows")
    end)
  end)
end

function wisp.refresh_cache_action()
  return wezterm.action_callback(function()
    safely(function()
      local _, refresh_error = run_child "refresh"
      if refresh_error then
        wezterm.log_error(refresh_error)
      end
    end)
  end)
end

function wisp.switch_to_project_action(project_id)
  return wezterm.action_callback(function(window, pane)
    safely(function()
      local projects, project_error = query_projects()
      if not projects then
        wezterm.log_error(project_error)
        return
      end
      for _, project in ipairs(projects) do
        if project.id == project_id then
          switch_to_project(window, pane, project)
          return
        end
      end
      wezterm.log_error("wisp could not find configured project " .. tostring(project_id))
    end)
  end)
end

local function current_spawn_command(window, pane)
  local projects, project_error = query_projects()
  if not projects then
    wezterm.log_error(project_error)
    projects = {}
  end
  local project
  local workspace = window:active_workspace()
  for _, candidate in ipairs(projects) do
    if workspace_for(candidate) == workspace then
      project = candidate
      break
    end
  end

  local command = project and spawn_command(project) or { domain = "CurrentPaneDomain" }
  local cwd = pane:get_current_working_dir()
  local same_domain = not project or command.domain.DomainName == pane:get_domain_name()
  if same_domain and cwd and cwd.scheme == "file" then
    command.cwd = cwd.file_path
  end
  return command
end

function wisp.new_tab_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      window:perform_action(wezterm.action.SpawnCommandInNewTab(current_spawn_command(window, pane)), pane)
    end)
  end)
end

function wisp.split_pane_action(direction, top_level)
  return wezterm.action_callback(function(window, pane)
    safely(function()
      window:perform_action(
        wezterm.action.SplitPane {
          command = current_spawn_command(window, pane),
          direction = direction,
          top_level = top_level,
        },
        pane
      )
    end)
  end)
end

function wisp.apply_to_config(config, configured_options)
  configured_options = configured_options or {}
  validate_options(configured_options)
  configure(configured_options)

  if options.picker_binding then
    local binding = {}
    for key, value in pairs(options.picker_binding) do
      binding[key] = value
    end
    binding.action = wisp.project_picker_action()
    config.keys = config.keys or {}
    table.insert(config.keys, binding)
  end
end

configure {}

return wisp
