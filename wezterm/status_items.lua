local StatusItems = {}
StatusItems.__index = StatusItems

local function append(target, source)
  for _, item in ipairs(source) do
    table.insert(target, item)
  end
end

local function render_opencode(context)
  local colors = context.colors
  local items = {
    { Background = { Color = colors.opencode_background } },
    { Foreground = { Color = colors.foreground } },
    { Attribute = { Intensity = "Bold" } },
    { Text = " OC " },
  }
  local cells = {
    { context.counts.idle, colors.idle_background },
    { context.counts.running, colors.running_background },
  }
  if context.counts.waiting > 0 then
    table.insert(cells, { context.counts.waiting, colors.waiting_background, context.flashes.waiting })
  end
  local failures = context.counts.retrying + context.counts.error
  if failures > 0 then
    table.insert(cells, { failures, colors.failure_background, context.flashes.failure })
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
  return items
end

local function render_directory(context)
  return {
    { Background = { Color = context.workspace_color } },
    { Foreground = { Color = context.colors.foreground } },
    { Attribute = { Intensity = "Bold" } },
    { Text = " " .. context.project .. " " },
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

function StatusItems:render(configured, context, clickable)
  local items = {}
  for _, item in ipairs(configured) do
    local provider = assert(self.providers[item.name], "unknown Wisp status provider " .. item.name)
    local rendered = provider.render(context)
    if clickable and item.action and #rendered > 0 then
      table.insert(items, { Hyperlink = "wisp://status/" .. item.name })
    end
    append(items, rendered)
    if clickable and item.action and #rendered > 0 then
      table.insert(items, "EndHyperlink")
    end
  end
  return items
end

return StatusItems
