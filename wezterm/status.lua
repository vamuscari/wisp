local Status = {}
Status.__index = Status
local FLASH_INTERVAL_SECONDS = 0.25
local FLASH_TRANSITIONS = 6

function Status.new(wezterm, options, client)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
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
  local items = {
    { Background = { Color = colors.opencode_background } },
    { Foreground = { Color = colors.foreground } },
    { Attribute = { Intensity = "Bold" } },
    { Text = " OC " },
  }
  local cells = {
    { self.counts.idle, colors.idle_background },
    { self.counts.running, colors.running_background },
  }
  if self.counts.waiting > 0 then
    table.insert(cells, { self.counts.waiting, colors.waiting_background, self.flashes.waiting })
  end
  local failures = self.counts.retrying + self.counts.error
  if failures > 0 then
    table.insert(cells, { failures, colors.failure_background, self.flashes.failure })
  end
  for _, cell in ipairs(cells) do
    table.insert(items, { Background = { Color = cell[2] } })
    table.insert(items, { Foreground = { Color = colors.foreground } })
    local hidden = cell[3] and cell[3].active and not cell[3].visible
    if hidden then
      table.insert(items, { Attribute = { Invisible = true } })
    end
    table.insert(items, { Text = " " .. cell[1] .. " " })
    if hidden then
      table.insert(items, { Attribute = { Invisible = false } })
    end
  end
  table.insert(items, { Background = { Color = workspace_color } })
  table.insert(items, { Foreground = { Color = colors.foreground } })
  table.insert(items, { Attribute = { Intensity = "Bold" } })
  table.insert(items, { Text = " " .. project .. " " })
  window:set_right_status(self.wezterm.format(items))
end

function Status:install(safely)
  self.wezterm.on("update-status", function(window, pane)
    safely(function()
      self.targets[window] = pane
      self:refresh()
      self:render(window, pane)
    end)
  end)
end

return Status
