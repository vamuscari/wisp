local wezterm = require "wezterm"
local deployed_wisp_path, deployment_token, module_directory = ...
local WISP_VERSION = 4

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
local Picker = load_module "picker"
local Status = load_module "status"

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

local options = Options.new(deployed_wisp_path)
local client = Client.new(wezterm, options, WISP_VERSION)
local workspace = Workspace.new(wezterm, options, client, report_error)
local picker = Picker.new(wezterm, options, client, workspace, report_error)
local status = Status.new(wezterm, options, client)
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

  if values.status_bar then
    status:install(safely)
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
