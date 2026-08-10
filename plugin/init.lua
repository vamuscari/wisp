local wezterm = require "wezterm"

local wisp = {}
local options = {}
local listings = {}
local OPEN_WORKSPACE = "__wisp_open_workspace__"
local BROWSE_FILES = "__wisp_browse_files__"
local GO_BACK = "__wisp_go_back__"

local function expand_home(path)
  if path == "~" then
    return wezterm.home_dir
  end
  if path:sub(1, 2) == "~/" or path:sub(1, 2) == "~\\" then
    return wezterm.home_dir .. path:sub(2)
  end
  return path
end

local function path_key(path)
  local windows = path:match "^%a:[/\\]" or path:match "^[/\\][/\\]"
  local normalized = path:gsub("\\", "/")
  local unc = normalized:sub(1, 2) == "//"
  if unc then
    normalized = "//" .. normalized:sub(3):gsub("/+", "/")
  else
    normalized = normalized:gsub("/+", "/")
  end

  local prefix = ""
  local rest = normalized
  local protected_components = 0
  if unc then
    prefix = "//"
    rest = normalized:sub(3)
    protected_components = 2
  elseif normalized:match "^%a:/" then
    prefix = normalized:sub(1, 3)
    rest = normalized:sub(4)
  elseif normalized:sub(1, 1) == "/" then
    prefix = "/"
    rest = normalized:sub(2)
  end

  local components = {}
  for component in rest:gmatch "[^/]+" do
    if component == ".." then
      if #components > protected_components and components[#components] ~= ".." then
        table.remove(components)
      elseif prefix == "" then
        table.insert(components, component)
      end
    elseif component ~= "." then
      table.insert(components, component)
    end
  end

  normalized = prefix .. table.concat(components, "/")
  if normalized == "" then
    normalized = "."
  elseif prefix:match "^%a:/$" and #components == 0 then
    normalized = prefix
  end
  if windows then
    normalized = normalized:lower()
  end
  return normalized
end

local function basename(path)
  local normalized = path:gsub("\\", "/"):gsub("/+$", "")
  return normalized:match "([^/]+)$" or normalized
end

local function read_listing(path)
  local key = path_key(path)
  local now = os.time()
  local cached = listings[key]
  local ttl = options.cache_ttl_seconds or 60
  if cached and now - cached.scanned_at < ttl then
    return cached
  end

  local ok, result = pcall(wezterm.read_dir, path)
  local listing = {
    entries = ok and result or {},
    error = ok and nil or result,
    ok = ok,
    path = path,
    scanned_at = now,
  }
  listings[key] = listing
  return listing
end

local function project_from(project, defaults)
  local path = expand_home(project.path)
  local name = project.name or basename(path)
  local group = project.group or defaults.group or "Projects"
  return {
    display_name = project.display_name or name,
    domain = project.domain or defaults.domain or options.spawn_domain or { DomainName = "local" },
    group = group,
    id = project.id or path,
    name = name,
    path = path,
    workspace = project.workspace or "wisp:" .. group .. "/" .. name,
  }
end

local function discover_projects()
  local projects = {}
  local seen = {}
  local ids = {}
  local workspaces = {}

  local function add(project, defaults)
    local resolved = project_from(project, defaults or {})
    local key = path_key(resolved.path)
    if seen[key] then
      return
    end
    if ids[resolved.id] then
      error(
        string.format(
          "wisp duplicate project id %s for %s and %s",
          tostring(resolved.id),
          ids[resolved.id],
          resolved.path
        )
      )
    end
    if workspaces[resolved.workspace] then
      error(
        string.format(
          "wisp duplicate workspace %s for %s and %s",
          resolved.workspace,
          workspaces[resolved.workspace],
          resolved.path
        )
      )
    end
    seen[key] = true
    ids[resolved.id] = resolved.path
    workspaces[resolved.workspace] = resolved.path
    table.insert(projects, resolved)
  end

  for _, project in ipairs(options.projects or {}) do
    local resolved_path = expand_home(project.path)
    read_listing(resolved_path)
    add(project)
  end

  for _, root_option in ipairs(options.roots or {}) do
    local root = type(root_option) == "table" and root_option or { path = root_option }
    local root_path = expand_home(root.path)
    local listing = read_listing(root_path)
    if not listing.ok then
      wezterm.log_warn("wisp could not read root " .. root_path .. ": " .. tostring(listing.error))
    else
      local group = root.group or basename(root_path)
      for _, path in ipairs(listing.entries) do
        if read_listing(path).ok then
          add({ path = path }, { domain = root.domain, group = group })
        end
      end
    end
  end

  table.sort(projects, function(left, right)
    local left_name = left.display_name:lower()
    local right_name = right.display_name:lower()
    if left_name == right_name then
      return left.path < right.path
    end
    return left_name < right_name
  end)

  return projects
end

local function project_choices(projects)
  local active = {}
  for _, workspace in ipairs(wezterm.mux.get_workspace_names()) do
    active[workspace] = true
  end

  local choices = {}
  for _, project in ipairs(projects) do
    table.insert(choices, {
      id = project.path,
      label = string.format(
        "%s / %s [%s]",
        project.group,
        project.display_name,
        active[project.workspace] and "open" or "new"
      ),
    })
  end
  return choices
end

local show_project_menu
local show_directory

local function spawn_command(project, args)
  local command = {
    cwd = project.path,
    domain = project.domain,
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
      name = project.workspace,
      spawn = spawn_command(project),
    },
    pane
  )
end

local function file_command(project, path)
  if type(options.open_file) == "function" then
    return options.open_file(project, path)
  end
  if type(options.open_file) ~= "table" then
    return nil
  end

  local args = {}
  for _, arg in ipairs(options.open_file) do
    table.insert(args, arg)
  end
  table.insert(args, path)
  return args
end

local function open_file(window, pane, project, path)
  local args = file_command(project, path)
  if type(args) ~= "table" or #args == 0 then
    wezterm.log_error "wisp open_file must be configured with argv or an argv callback"
    return
  end

  local workspace_open = false
  for _, workspace in ipairs(wezterm.mux.get_workspace_names()) do
    if workspace == project.workspace then
      workspace_open = true
      break
    end
  end

  if not workspace_open then
    window:perform_action(
      wezterm.action.SwitchToWorkspace {
        name = project.workspace,
        spawn = spawn_command(project, args),
      },
      pane
    )
    return
  end

  for _, mux_window in ipairs(wezterm.mux.all_windows()) do
    if mux_window:get_workspace() == project.workspace then
      mux_window:spawn_tab(spawn_command(project, args))
      window:perform_action(wezterm.action.SwitchToWorkspace { name = project.workspace }, pane)
      return
    end
  end

  wezterm.log_error("wisp could not find a window for workspace " .. project.workspace)
end

show_project_menu = function(window, pane, project)
  window:perform_action(
    wezterm.action.InputSelector {
      action = wezterm.action_callback(function(_, _, id)
        if not id then
          return
        end
        if id == OPEN_WORKSPACE then
          switch_to_project(window, pane, project)
        elseif id == BROWSE_FILES then
          show_directory(window, pane, project, project.path, {})
        end
      end),
      choices = {
        { id = OPEN_WORKSPACE, label = "Open workspace" },
        { id = BROWSE_FILES, label = "Browse files" },
      },
      title = project.group .. " / " .. project.display_name,
    },
    pane
  )
end

show_directory = function(window, pane, project, path, ancestors)
  local listing = read_listing(path)
  if not listing.ok then
    wezterm.log_warn("wisp could not read directory " .. path .. ": " .. tostring(listing.error))
    return
  end

  local entries = {}
  for _, entry in ipairs(listing.entries) do
    table.insert(entries, entry)
  end
  table.sort(entries, function(left, right)
    local left_name = basename(left):lower()
    local right_name = basename(right):lower()
    if left_name == right_name then
      return left < right
    end
    return left_name < right_name
  end)

  local choices = {
    {
      id = GO_BACK,
      label = #ancestors == 0 and "Project actions" or "..",
    },
  }
  for _, entry in ipairs(entries) do
    table.insert(choices, { id = entry, label = basename(entry) })
  end

  window:perform_action(
    wezterm.action.InputSelector {
      action = wezterm.action_callback(function(_, _, id)
        if not id then
          return
        end
        if id == GO_BACK then
          if #ancestors == 0 then
            show_project_menu(window, pane, project)
            return
          end

          local parent = ancestors[#ancestors]
          local parent_ancestors = {}
          for index = 1, #ancestors - 1 do
            parent_ancestors[index] = ancestors[index]
          end
          show_directory(window, pane, project, parent, parent_ancestors)
          return
        end

        if read_listing(id).ok then
          local child_ancestors = {}
          for index, ancestor in ipairs(ancestors) do
            child_ancestors[index] = ancestor
          end
          table.insert(child_ancestors, path)
          show_directory(window, pane, project, id, child_ancestors)
        else
          open_file(window, pane, project, id)
        end
      end),
      choices = choices,
      fuzzy = true,
      fuzzy_description = "Find entry: ",
      title = "Browse: " .. path,
    },
    pane
  )
end

function wisp.project_picker_action()
  return wezterm.action_callback(function(window, pane)
    local projects = discover_projects()
    local by_path = {}
    for _, project in ipairs(projects) do
      by_path[path_key(project.path)] = project
    end
    window:perform_action(
      wezterm.action.InputSelector {
        action = wezterm.action_callback(function(_, _, id)
          if not id then
            return
          end
          local project = by_path[path_key(id)]
          if not project then
            wezterm.log_error("wisp could not match selected project " .. id)
            return
          end
          show_project_menu(window, pane, project)
        end),
        choices = project_choices(projects),
        fuzzy = true,
        fuzzy_description = "Find project: ",
        title = "Projects",
      },
      pane
    )
  end)
end

function wisp.refresh_cache_action()
  return wezterm.action_callback(function()
    listings = {}
    discover_projects()
  end)
end

function wisp.switch_to_project_action(project_id)
  return wezterm.action_callback(function(window, pane)
    for _, project in ipairs(discover_projects()) do
      if project.id == project_id then
        switch_to_project(window, pane, project)
        return
      end
    end
    wezterm.log_error("wisp could not find configured project " .. tostring(project_id))
  end)
end

local function current_spawn_command(window, pane)
  local project
  local workspace = window:active_workspace()
  for _, candidate in ipairs(discover_projects()) do
    if candidate.workspace == workspace then
      project = candidate
      break
    end
  end

  local command = project and spawn_command(project) or { domain = "CurrentPaneDomain" }
  local cwd = pane:get_current_working_dir()
  local same_domain = not project or project.domain.DomainName == pane:get_domain_name()
  if same_domain and cwd and cwd.scheme == "file" then
    command.cwd = cwd.file_path
  end
  return command
end

function wisp.new_tab_action()
  return wezterm.action_callback(function(window, pane)
    window:perform_action(wezterm.action.SpawnCommandInNewTab(current_spawn_command(window, pane)), pane)
  end)
end

function wisp.split_pane_action(direction, top_level)
  return wezterm.action_callback(function(window, pane)
    window:perform_action(
      wezterm.action.SplitPane {
        command = current_spawn_command(window, pane),
        direction = direction,
        top_level = top_level,
      },
      pane
    )
  end)
end

local function validate_options(configured)
  local function validate_domain(domain, label)
    if domain ~= nil then
      if type(domain) ~= "table" or type(domain.DomainName) ~= "string" or domain.DomainName == "" then
        error("wisp " .. label .. " must be a non-empty { DomainName = name } table")
      end
    end
  end

  if type(configured) ~= "table" then
    error "wisp options must be a table"
  end
  if configured.cache_ttl_seconds ~= nil then
    if type(configured.cache_ttl_seconds) ~= "number" or configured.cache_ttl_seconds < 0 then
      error "wisp cache_ttl_seconds must be a non-negative number"
    end
  end

  if configured.roots ~= nil and type(configured.roots) ~= "table" then
    error "wisp roots must be an array"
  end
  for index, root in ipairs(configured.roots or {}) do
    local path = type(root) == "table" and root.path or root
    if type(path) ~= "string" or path == "" then
      error(string.format("wisp roots[%d].path must be a non-empty string", index))
    end
    if type(root) == "table" then
      validate_domain(root.domain, string.format("roots[%d].domain", index))
    end
  end

  if configured.projects ~= nil and type(configured.projects) ~= "table" then
    error "wisp projects must be an array"
  end
  for index, project in ipairs(configured.projects or {}) do
    if type(project) ~= "table" or type(project.path) ~= "string" or project.path == "" then
      error(string.format("wisp projects[%d].path must be a non-empty string", index))
    end
    validate_domain(project.domain, string.format("projects[%d].domain", index))
  end

  validate_domain(configured.spawn_domain, "spawn_domain")

  if configured.open_file ~= nil and type(configured.open_file) ~= "function" then
    if type(configured.open_file) ~= "table" or #configured.open_file == 0 then
      error "wisp open_file must be a non-empty argv array or function"
    end
    for index, arg in ipairs(configured.open_file) do
      if type(arg) ~= "string" then
        error(string.format("wisp open_file[%d] must be a string", index))
      end
    end
  end
end

function wisp.apply_to_config(config, configured_options)
  configured_options = configured_options or {}
  validate_options(configured_options)
  options = configured_options
  listings = {}

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

return wisp
