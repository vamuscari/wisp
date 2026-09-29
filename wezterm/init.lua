local wezterm = require "wezterm"
local deployed_wisp_path, deployment_token, module_directory = ...
local WISP_VERSION = 8

if
  type(deployed_wisp_path) ~= "string"
  or deployed_wisp_path == ""
  or deployment_token ~= "wisp-deployment-v" .. WISP_VERSION
then
  error "Wisp's WezTerm adapter must be loaded by the deployed bootstrap"
end
if type(module_directory) ~= "string" or module_directory == "" then
  error "Wisp's WezTerm adapter requires its deployed module directory"
end

local function load_module(name)
  return assert(loadfile(module_directory .. "/" .. name .. ".lua"))()
end

local Options = load_module "options"
local Client = load_module "client"
local Workspace = load_module "workspace"
local Popup = load_module "popup"
local Picker = load_module "picker"
local Powerline = load_module "powerline"
local Status = load_module "status"
local StatusItems = load_module "status_items"

local function report_error(window, message)
  wezterm.log_error(message)
  pcall(function()
    window:toast_notification("Wisp", message, nil, 5000)
  end)
end

local function safely(callback)
  local completed, callback_error = pcall(callback)
  if not completed then
    wezterm.log_error("wisp adapter failed: " .. tostring(callback_error))
  end
end

local options = Options.new(deployed_wisp_path, wezterm.column_width)
local powerline = Powerline.new(wezterm)
local client = Client.new(wezterm, options, WISP_VERSION)
local workspace = Workspace.new(wezterm, options, client, report_error)
local popup = Popup.new(wezterm, options, workspace)
local picker = Picker.new(wezterm, options, client, workspace, popup, report_error)
local status = Status.new(wezterm, options, client, StatusItems.new(), powerline, function(window, pane, action, scope)
  picker:launch_popup(window, pane, action, false, scope)
end)
local wisp = {}

function wisp.project_picker_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      picker:launch(window, pane, "projects")
    end)
  end)
end

function wisp.window_picker_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      picker:launch(window, pane, "windows")
    end)
  end)
end

function wisp.opencode_picker_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      picker:launch(window, pane, "sessions")
    end)
  end)
end

function wisp.next_opencode_session_action(configured)
  configured = configured or {}
  if type(configured) ~= "table" then
    error "wisp next OpenCode session options must be a table"
  end
  for field in pairs(configured) do
    if field ~= "scope" and field ~= "status" then
      error("wisp next OpenCode session has an unknown option " .. tostring(field))
    end
  end
  local scope = configured.scope or "all"
  local status = configured.status or "any"
  if scope ~= "all" and scope ~= "current" then
    error "wisp next OpenCode session scope must be all or current"
  end
  if
    status ~= "any"
    and status ~= "error"
    and status ~= "permission"
    and status ~= "finished"
    and status ~= "running"
    and status ~= "priority"
  then
    error "wisp next OpenCode session status must be any, error, permission, finished, running, or priority"
  end
  return wezterm.action_callback(function(window, pane)
    safely(function()
      local project_path
      if scope == "current" then
        local projects, project_error = client:query_projects()
        if not projects then
          report_error(window, project_error)
          return
        end
        local current_workspace = window:mux_window():get_workspace()
        for _, project in ipairs(projects) do
          if workspace:workspace_for(project) == current_workspace then
            project_path = project.path
            break
          end
        end
        if not project_path then
          report_error(window, "Current workspace is not a Wisp project")
          return
        end
      end
      local result, next_error = client:query_next_opencode_session(status, pane:pane_id(), project_path)
      if not result then
        report_error(window, next_error)
        return
      end
      if result.status == "cancelled" then
        window:toast_notification("Wisp", "No matching live OpenCode sessions", nil, 2500)
        return
      end
      local selection = result.selection
      local activated, activate_error =
        workspace:activate_opencode_host_item(window, pane, selection.project, selection.host_item_id)
      if not activated then
        report_error(window, activate_error)
      end
    end)
  end)
end

function wisp.popup_action(initial_view)
  if initial_view ~= "projects" and initial_view ~= "windows" and initial_view ~= "sessions" then
    error "wisp popup action must be projects, windows, or sessions"
  end
  return wezterm.action_callback(function(window, pane)
    safely(function()
      picker:launch_popup(window, pane, initial_view)
    end)
  end)
end

function wisp.refresh_cache_action()
  return wezterm.action_callback(function()
    safely(function()
      local _, refresh_error = client:run "refresh"
      if refresh_error then
        wezterm.log_error(refresh_error)
      end
    end)
  end)
end

function wisp.switch_to_project_action(project_id)
  return wezterm.action_callback(function(window, pane)
    safely(function()
      local projects, project_error = client:query_projects()
      if not projects then
        wezterm.log_error(project_error)
        return
      end
      for _, project in ipairs(projects) do
        if project.id == project_id then
          workspace:switch_to_project(window, pane, project)
          return
        end
      end
      wezterm.log_error("wisp could not find configured project " .. tostring(project_id))
    end)
  end)
end

function wisp.new_tab_action()
  return wezterm.action_callback(function(window, pane)
    safely(function()
      window:perform_action(wezterm.action.SpawnCommandInNewTab(workspace:current_spawn_command(window, pane)), pane)
    end)
  end)
end

function wisp.split_pane_action(direction, top_level)
  return wezterm.action_callback(function(window, pane)
    safely(function()
      window:perform_action(
        wezterm.action.SplitPane {
          command = workspace:current_spawn_command(window, pane),
          direction = direction,
          top_level = top_level,
        },
        pane
      )
    end)
  end)
end

function wisp.apply_to_config(config, configured_options)
  options:configure(configured_options or {})
  local values = options:get()
  powerline:configure(values.powerline)
  powerline:apply_tab_config(config)

  if values.status_bar then
    status:install(safely)
  end

  if values.opencode_tab_colors or powerline:get().tabs then
    status:install_tab_formatter()
  end

  if values.tab_button_pickers then
    config.show_new_tab_button_in_tab_bar = true
    wezterm.on("new-tab-button-click", function(window, pane, button)
      if button == "Left" or button == "Right" then
        safely(function()
          picker:launch_popup(window, pane, button == "Left" and "projects" or "sessions", button == "Left")
        end)
        return false
      end
    end)
  end

  if values.picker_binding then
    local binding = {}
    for key, value in pairs(values.picker_binding) do
      binding[key] = value
    end
    binding.action = wisp.project_picker_action()
    config.keys = config.keys or {}
    table.insert(config.keys, binding)
  end
end

return wisp
