local Popup = {}
Popup.__index = Popup

function Popup.new(wezterm, options, workspace)
  return setmetatable({
    wezterm = wezterm,
    options = options,
    workspace = workspace,
    active = {},
  }, Popup)
end

function Popup:close(handle)
  if handle.closed then
    return true
  end
  local is_active = self.active[handle.window_id] == handle
  local called, closed, close_error = pcall(self.workspace.close_pane, self.workspace, handle.pane_id)
  if not called then
    close_error = closed
    closed = false
  end
  if not closed then
    self.wezterm.log_warn("wisp could not close popup pane: " .. tostring(close_error))
    return nil, close_error
  end
  handle.closed = true
  if is_active then
    self.active[handle.window_id] = nil
  end
  return true
end

function Popup:open(window, source_pane, spec)
  if type(spec) ~= "table" or type(spec.id) ~= "string" or spec.id == "" then
    return nil, "wisp popup requires a non-empty ID"
  end
  if type(spec.args) ~= "table" or type(spec.args[1]) ~= "string" or spec.args[1] == "" then
    return nil, "wisp popup requires a non-empty argv"
  end
  local identified, window_id = pcall(function()
    return window:window_id()
  end)
  if not identified or window_id == nil then
    return nil, "wisp popup requires a stable window ID"
  end
  window_id = tostring(window_id)

  local existing = self.active[window_id]

  local values = self.options:get()
  local command = {
    args = spec.args,
    direction = values.popup.direction,
    size = values.popup.size,
    top_level = true,
  }
  if spec.cwd ~= nil then
    command.cwd = spec.cwd
  end
  if spec.domain ~= nil then
    command.domain = spec.domain
  end
  if spec.set_environment_variables ~= nil then
    command.set_environment_variables = spec.set_environment_variables
  end

  local spawned, popup_pane = pcall(function()
    return source_pane:split(command)
  end)
  if not spawned or not popup_pane then
    return nil, "wisp could not open popup: " .. tostring(popup_pane)
  end

  local handle = {
    id = spec.id,
    pane = popup_pane,
    pane_id = popup_pane:pane_id(),
    window_id = window_id,
    closed = false,
  }
  function handle:close()
    return Popup.close(self.manager, self)
  end
  function handle:is_current()
    return self.manager.active[self.window_id] == self
  end
  handle.manager = self
  if existing then
    local closed, close_error = existing:close()
    if not closed then
      local called, cleaned, cleanup_error = pcall(self.workspace.close_pane, self.workspace, handle.pane_id)
      if not called then
        cleanup_error = cleaned
        cleaned = false
      end
      if not cleaned then
        self.wezterm.log_warn("wisp could not clean up replacement popup pane: " .. tostring(cleanup_error))
        self.active[window_id] = handle
        return handle
      end
      return nil, "wisp could not replace popup: " .. tostring(close_error)
    end
  end
  self.active[window_id] = handle
  return handle
end

return Popup
