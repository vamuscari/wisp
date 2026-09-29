local StatusItems = {}
StatusItems.__index = StatusItems

local function append(target, source)
  for _, item in ipairs(source) do
    table.insert(target, item)
  end
end

local function padded(value, padding)
  local spaces = string.rep(" ", padding)
  return spaces .. value .. spaces
end

local function render_opencode(context)
  local colors = context.colors
  local padding = context.powerline and context.powerline.padding or 1
  local items = {
    { Background = { Color = colors.opencode_background } },
    { Foreground = { Color = colors.foreground } },
    { Attribute = { Intensity = "Bold" } },
    { Text = padded("OC", padding) },
  }
  local cells = {
    { context.counts.idle, colors.idle_background },
    { context.counts.running, colors.running_background },
  }
  local last_background = colors.opencode_background
  if context.counts.waiting > 0 then
    table.insert(cells, { context.counts.waiting, colors.waiting_background, context.flashes.waiting })
  end
  local failures = context.counts.retrying + context.counts.error
  if failures > 0 then
    table.insert(cells, { failures, colors.failure_background, context.flashes.failure })
  end
  for _, cell in ipairs(cells) do
    last_background = cell[2]
    table.insert(items, { Background = { Color = cell[2] } })
    table.insert(items, { Foreground = { Color = colors.foreground } })
    local hidden = cell[3] and cell[3].active and not cell[3].visible
    if hidden then
      table.insert(items, { Attribute = { Invisible = true } })
    end
    table.insert(items, { Text = padded(tostring(cell[1]), padding) })
    if hidden then
      table.insert(items, { Attribute = { Invisible = false } })
    end
  end
  return {
    first_background = colors.opencode_background,
    items = items,
    last_background = last_background,
  }
end

local function render_directory(context)
  local padding = context.powerline and context.powerline.padding or 1
  return {
    first_background = context.workspace_color,
    items = {
      { Background = { Color = context.workspace_color } },
      { Foreground = { Color = context.colors.foreground } },
      { Attribute = { Intensity = "Bold" } },
      { Text = padded(context.project, padding) },
    },
    last_background = context.workspace_color,
  }
end

function StatusItems.new()
  return setmetatable({
    providers = {
      opencode = { render = render_opencode },
      directory = { render = render_directory },
    },
  }, StatusItems)
end

function StatusItems:render(configured, context, clickable, powerline)
  local items = {}
  for index, item in ipairs(configured) do
    local provider = assert(self.providers[item.name], "unknown Wisp status provider " .. item.name)
    local rendered = provider.render(context)
    local provider_items = rendered.items
    if context.powerline then
      provider_items = powerline:segment(
        provider_items,
        rendered.first_background,
        rendered.last_background,
        context.powerline,
        false,
        nil,
        false,
        index == #configured
      )
    end
    if clickable and item.action and #provider_items > 0 then
      table.insert(items, { Hyperlink = "wisp://status/" .. item.name })
    end
    append(items, provider_items)
    if clickable and item.action and #provider_items > 0 then
      table.insert(items, "EndHyperlink")
    end
    if context.powerline and index < #configured then
      powerline:append_gap(items, context.powerline)
    end
  end
  return items
end

return StatusItems
