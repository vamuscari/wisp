local Status = {}
Status.__index = Status
local FLASH_INTERVAL_SECONDS = 0.25
local FLASH_TRANSITIONS = 6
local OPENCODE_STATUS_MAX_AGE_SECONDS = 90
local OPENCODE_STATUS_USER_VAR = "WISP_OPENCODE_STATUS"
local TAB_REFRESH_MARKER = "\u{200b}"
local OPENCODE_STATE_PRIORITY = { idle = 1, running = 2, failure = 3, waiting = 4 }
local OPENCODE_STATE_COLOR = {
  idle = "idle_background",
  running = "running_background",
  failure = "failure_background",
  waiting = "waiting_background",
}

local function parse_pane_status(value, now)
  if type(value) ~= "string" then
    return
  end
  local state, updated = value:match "^(%l+):(%d+)$"
  local priority = OPENCODE_STATE_PRIORITY[state]
  updated = tonumber(updated)
  if
    not priority
    or not updated
    or updated ~= math.floor(updated)
    or updated > now
    or now - updated > OPENCODE_STATUS_MAX_AGE_SECONDS
  then
    return
  end
  return state, priority
end

function Status.new(wezterm, options, client, providers, powerline, activate)
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
    powerline = powerline,
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
    tab_refresh_markers = setmetatable({}, { __mode = "k" }),
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
  local powerline_status = self.powerline:get().status
  local items = self.providers:render(self.options:get().status_items, {
    colors = colors,
    counts = self.counts,
    flashes = self.flashes,
    powerline = powerline_status,
    project = project,
    workspace_color = workspace_color,
  }, self.clickable, self.powerline)
  if self.options:get().opencode_tab_colors then
    -- WezTerm has no tab-bar invalidation API, so vary an invisible status marker to force a freshness redraw.
    local tab_refresh_marker = not self.tab_refresh_markers[window]
    self.tab_refresh_markers[window] = tab_refresh_marker
    table.insert(items, { Attribute = { Intensity = tab_refresh_marker and "Bold" or "Normal" } })
    table.insert(items, { Text = TAB_REFRESH_MARKER })
  end
  window:set_right_status(self.wezterm.format(items))
end

function Status:format_tab_title(tab, tabs, hover, max_width)
  local state
  local priority = 0
  if self.options:get().opencode_tab_colors then
    local now = os.time()
    for _, pane in ipairs(tab.panes) do
      local user_vars = pane.user_vars
      local pane_state, pane_priority =
        parse_pane_status(type(user_vars) == "table" and user_vars[OPENCODE_STATUS_USER_VAR] or nil, now)
      if pane_priority and pane_priority > priority then
        state = pane_state
        priority = pane_priority
      end
    end
  end
  local powerline_tabs = self.powerline:get().tabs
  if not state and not powerline_tabs then
    return
  end

  local title = tab.tab_title
  if type(title) ~= "string" or title == "" then
    title = tab.active_pane.title
  end
  local colors = self.options:get().status_colors
  if powerline_tabs then
    local first_tab = tabs[1]
    local flush_left = first_tab and first_tab.tab_id == tab.tab_id
    local background
    local foreground
    local tab_colors = powerline_tabs.colors
    if tab.is_active then
      background = tab_colors.active_background
      foreground = tab_colors.active_foreground
    elseif state then
      background = colors[OPENCODE_STATE_COLOR[state]]
      foreground = colors.foreground
    elseif hover then
      background = tab_colors.hover_background
      foreground = tab_colors.hover_foreground
    else
      background = tab_colors.inactive_background
      foreground = tab_colors.inactive_foreground
    end
    if max_width <= 0 then
      return {}
    end
    local remaining = max_width - (title == "" and 0 or 1)
    local right = powerline_tabs.right
    if powerline_tabs.right_width > remaining then
      right = ""
    else
      remaining = remaining - powerline_tabs.right_width
    end
    local left = flush_left and "" or powerline_tabs.left
    if not flush_left then
      if powerline_tabs.left_width > remaining then
        left = ""
      else
        remaining = remaining - powerline_tabs.left_width
      end
    end
    local padding_width = math.min(powerline_tabs.padding, math.floor(remaining / 2))
    remaining = remaining - padding_width * 2
    local gap = math.min(powerline_tabs.gap, remaining)
    remaining = remaining - gap
    title = self.wezterm.truncate_right(title, remaining + (title == "" and 0 or 1))
    local padding = string.rep(" ", padding_width)
    local body = {
      { Background = { Color = background } },
      { Foreground = { Color = foreground } },
      { Attribute = { Intensity = tab.is_active and "Bold" or "Normal" } },
      { Text = padding .. title .. padding },
    }
    return self.powerline:segment(
      body,
      background,
      background,
      { left = left, right = right, gap = gap },
      true,
      tab_colors.bar_background,
      flush_left,
      false
    )
  end

  title = self.wezterm.truncate_right(title, max_width)
  return {
    { Background = { Color = colors[OPENCODE_STATE_COLOR[state]] } },
    { Foreground = { Color = colors.foreground } },
    { Attribute = { Intensity = tab.is_active and "Bold" or "Normal" } },
    { Text = title },
  }
end

function Status:activate_provider(window, pane, provider_name, button)
  for _, item in ipairs(self.options:get().status_items) do
    if item.name == provider_name and item.action then
      local scope
      if provider_name == "opencode" and item.action == "sessions" then
        scope = button == "Right" and "current" or "all"
      end
      self.activate(window, pane, item.action, scope)
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
  self.wezterm.on("status-action-click", function(window, pane, uri, button)
    local provider_name = type(uri) == "string" and uri:match "^wisp://status/([%w_-]+)$" or nil
    if provider_name ~= "opencode" or button ~= "Right" then
      return
    end
    local activated, activate_error = pcall(function()
      self:activate_provider(window, pane, provider_name, button)
    end)
    if not activated then
      self:report_error("wisp status action failed: " .. tostring(activate_error))
    end
    return false
  end)
end

function Status:install_tab_formatter()
  self.wezterm.on("format-tab-title", function(tab, tabs, _, _, hover, max_width)
    local formatted, result = pcall(function()
      return self:format_tab_title(tab, tabs, hover, max_width)
    end)
    if not formatted then
      self.wezterm.log_error("wisp tab status format failed: " .. tostring(result))
      return
    end
    return result
  end)
end

return Status
