local Status = {}
Status.__index = Status

function Status.new(wezterm, options, client)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    client = client,
    counts = { waiting = 0, running = 0, retrying = 0, idle = 0, error = 0 },
    cooling_down = false,
    refreshing = false,
    last_error = nil,
  }, Status)
end

function Status:report_error(message)
  if message ~= self.last_error then
    self.wezterm.log_error(message)
    self.last_error = message
  end
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
  self.counts = {
    waiting = counts.waiting,
    running = counts.running,
    retrying = counts.retrying,
    idle = counts.idle,
    error = counts.error,
  }
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
  local checked_leader, leader_is_active = pcall(function()
    return window:leader_is_active()
  end)
  local colors = self.options:get().status_colors
  local workspace_color = checked_leader and leader_is_active and colors.active_workspace_background
    or colors.workspace_background
  local items = {
    { Background = { Color = workspace_color } },
    { Foreground = { Color = colors.foreground } },
    { Attribute = { Intensity = "Bold" } },
    { Text = " " .. workspace .. " " },
  }
  local cells = {
    { "wait", self.counts.waiting, colors.waiting_background },
    { "run", self.counts.running, colors.running_background },
    { "retry", self.counts.retrying, colors.retrying_background },
    { "idle", self.counts.idle, colors.idle_background },
    { "err", self.counts.error, colors.error_background },
  }
  for _, cell in ipairs(cells) do
    table.insert(items, { Background = { Color = cell[3] } })
    table.insert(items, { Foreground = { Color = colors.foreground } })
    table.insert(items, { Text = " " .. cell[1] .. " " .. cell[2] .. " " })
  end
  window:set_right_status(self.wezterm.format(items))
end

function Status:install(safely)
  self.wezterm.on("update-status", function(window, pane)
    safely(function()
      self:refresh()
      self:render(window, pane)
    end)
  end)
end

return Status
