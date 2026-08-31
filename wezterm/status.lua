local Status = {}
Status.__index = Status
local FLASH_INTERVAL_SECONDS = 0.25
local FLASH_TRANSITIONS = 6

function Status.new(wezterm, options, client, providers, activate)
  local clickable = pcall(function()
    wezterm.format {
      { Hyperlink = "wisp://status/probe" },
      { Text = "" },
      "EndHyperlink",
    }
  end)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
    providers = providers,
    activate = activate,
    clickable = clickable,
    counts = { waiting = 0, running = 0, retrying = 0, idle = 0, error = 0 },
    cooling_down = false,
    flashes = {
      waiting = { active = false, generation = 0, visible = true },
      failure = { active = false, generation = 0, visible = true },
    },
    refreshing = false,
    last_error = nil,
    targets = setmetatable({}, { __mode = "k" }),
  }, Status)
end

function Status:report_error(message)
  if message ~= self.last_error then
    self.wezterm.log_error(message)
    self.last_error = message
  end
end

function Status:render_targets()
  for window, pane in pairs(self.targets) do
    local rendered, render_error = pcall(function()
      self:render(window, pane)
    end)
    if not rendered then
      self.targets[window] = nil
      self:report_error("wisp status render failed: " .. tostring(render_error))
    end
  end
end

function Status:update_flash(kind, previous, current)
  local flash = self.flashes[kind]
  if current == 0 then
    flash.generation = flash.generation + 1
    flash.active = false
    flash.visible = true
    return
  end
  if previous > 0 then
    return
  end

  flash.generation = flash.generation + 1
  flash.active = true
  flash.visible = true
  local generation = flash.generation
  local transitions = FLASH_TRANSITIONS
  local function advance()
    if flash.generation ~= generation then
      return
    end
    flash.visible = not flash.visible
    transitions = transitions - 1
    if transitions == 0 then
      flash.active = false
      flash.visible = true
    end
    self:render_targets()
    if transitions > 0 then
      self.wezterm.time.call_after(FLASH_INTERVAL_SECONDS, advance)
    end
  end
  self.wezterm.time.call_after(FLASH_INTERVAL_SECONDS, advance)
end

function Status:refresh()
  local enabled = false
  for _, item in ipairs(self.options:get().status_items) do
    enabled = enabled or item.name == "opencode"
  end
  if not enabled then
    return
  end
  if self.refreshing or self.cooling_down then
    return
  end
  self.refreshing = true
  local completed, counts, status_error = pcall(function()
    return self.client:query_opencode_status()
  end)
  self.refreshing = false
  self.cooling_down = true
  self.wezterm.time.call_after(self.options:get().status_interval_seconds, function()
    self.cooling_down = false
  end)
  if not completed then
    self:report_error("wisp opencode status failed: " .. tostring(counts))
    return
  end
  if not counts then
    self:report_error(status_error)
    return
  end
  local next_counts = {
    waiting = counts.waiting,
    running = counts.running,
    retrying = counts.retrying,
    idle = counts.idle,
    error = counts.error,
  }
  local previous_failures = self.counts.retrying + self.counts.error
  local next_failures = next_counts.retrying + next_counts.error
  local previous_waiting = self.counts.waiting
  self.counts = next_counts
  self:update_flash("waiting", previous_waiting, next_counts.waiting)
  self:update_flash("failure", previous_failures, next_failures)
  self.last_error = nil
end

function Status:render(window, pane)
  local pane_window_ok, mux_window = pcall(function()
    return pane:window()
  end)

  if not pane_window_ok or not mux_window then
    local window_ok, window_mux = pcall(function()
      return window:mux_window()
    end)
    mux_window = window_ok and window_mux or nil
  end

  local workspace_ok, workspace = pcall(function()
    return mux_window and mux_window:get_workspace() or window:active_workspace()
  end)
  workspace = workspace_ok and type(workspace) == "string" and workspace or ""
  local project = workspace:match "([^/\\]+)$" or workspace
  local checked_leader, leader_is_active = pcall(function()
    return window:leader_is_active()
  end)
  local colors = self.options:get().status_colors
  local workspace_color = checked_leader and leader_is_active and colors.active_workspace_background
    or colors.workspace_background
  local items = self.providers:render(self.options:get().status_items, {
    colors = colors,
    counts = self.counts,
    flashes = self.flashes,
    project = project,
    workspace_color = workspace_color,
  }, self.clickable)
  window:set_right_status(self.wezterm.format(items))
end

function Status:activate_provider(window, pane, provider_name)
  for _, item in ipairs(self.options:get().status_items) do
    if item.name == provider_name and item.action then
      self.activate(window, pane, item.action)
      return
    end
  end
end

function Status:install(safely)
  self.wezterm.on("update-status", function(window, pane)
    safely(function()
      self.targets[window] = pane
      self:refresh()
      self:render(window, pane)
    end)
  end)
  self.wezterm.on("open-uri", function(window, pane, uri)
    local provider_name = type(uri) == "string" and uri:match "^wisp://status/([%w_-]+)$" or nil
    if not provider_name then
      return
    end
    local activated, activate_error = pcall(function()
      self:activate_provider(window, pane, provider_name)
    end)
    if not activated then
      self:report_error("wisp status action failed: " .. tostring(activate_error))
    end
    return false
  end)
end

return Status
