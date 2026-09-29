local Powerline = {}
Powerline.__index = Powerline

local DEFAULT_TAB_COLORS = {
  bar_background = "#E9E2C9",
  active_background = "#583A24",
  active_foreground = "#E9E2C9",
  inactive_background = "#E9E1C3",
  inactive_foreground = "#585148",
  hover_background = "#DAC9AA",
  hover_foreground = "#585148",
}

local function spaces(count)
  return string.rep(" ", count)
end

local function append(target, source)
  for _, item in ipairs(source) do
    table.insert(target, item)
  end
end

function Powerline.new(wezterm)
  return setmetatable({ wezterm = wezterm, values = {} }, Powerline)
end

function Powerline:resolve_shape(configured)
  if type(configured) == "table" then
    return { left = configured.left, right = configured.right }
  end

  local nerdfonts = self.wezterm.nerdfonts
  local presets = {
    arrow = { left = nerdfonts.pl_right_hard_divider, right = nerdfonts.pl_left_hard_divider },
    slash = { left = nerdfonts.ple_backslash_separator, right = nerdfonts.ple_forwardslash_separator },
    slant = { left = nerdfonts.ple_lower_right_triangle, right = nerdfonts.ple_upper_left_triangle },
    rounded = { left = nerdfonts.ple_left_half_circle_thick, right = nerdfonts.ple_right_half_circle_thick },
    plain = { left = "", right = "" },
  }
  return presets[configured or "slant"]
end

function Powerline:surface(configured, include_colors)
  if not configured then
    return
  end
  local shape = self:resolve_shape(configured.shape)
  local surface = {
    gap = configured.gap == nil and 1 or configured.gap,
    left = shape.left,
    left_width = shape.left == "" and 0 or self.wezterm.column_width(shape.left),
    padding = configured.padding == nil and 1 or configured.padding,
    right = shape.right,
    right_width = shape.right == "" and 0 or self.wezterm.column_width(shape.right),
  }
  if include_colors then
    surface.colors = {}
    for field, default in pairs(DEFAULT_TAB_COLORS) do
      surface.colors[field] = configured.colors and configured.colors[field] or default
    end
  end
  return surface
end

function Powerline:configure(configured)
  configured = configured or {}
  self.values = {
    status = self:surface(configured.status, false),
    tabs = self:surface(configured.tabs, true),
  }
end

function Powerline:get()
  return self.values
end

function Powerline:apply_tab_config(config)
  local tabs = self.values.tabs
  if not tabs then
    return
  end
  local colors = tabs.colors
  config.use_fancy_tab_bar = false
  config.colors = config.colors or {}
  config.colors.tab_bar = config.colors.tab_bar or {}
  local tab_bar = config.colors.tab_bar
  tab_bar.background = colors.bar_background
  tab_bar.active_tab = tab_bar.active_tab or {}
  tab_bar.active_tab.bg_color = colors.active_background
  tab_bar.active_tab.fg_color = colors.active_foreground
  tab_bar.inactive_tab = tab_bar.inactive_tab or {}
  tab_bar.inactive_tab.bg_color = colors.inactive_background
  tab_bar.inactive_tab.fg_color = colors.inactive_foreground
  tab_bar.inactive_tab_hover = tab_bar.inactive_tab_hover or {}
  tab_bar.inactive_tab_hover.bg_color = colors.hover_background
  tab_bar.inactive_tab_hover.fg_color = colors.hover_foreground
end

function Powerline:append_gap(items, surface, bar_background)
  if surface.gap == 0 then
    return
  end
  if bar_background then
    table.insert(items, { Background = { Color = bar_background } })
  else
    table.insert(items, "ResetAttributes")
  end
  table.insert(items, { Text = spaces(surface.gap) })
end

function Powerline:segment(
  body,
  first_background,
  last_background,
  surface,
  include_gap,
  bar_background,
  omit_left,
  omit_right
)
  local items = {}
  if surface.left ~= "" and not omit_left then
    if bar_background then
      table.insert(items, { Background = { Color = bar_background } })
    else
      table.insert(items, "ResetAttributes")
    end
    table.insert(items, { Foreground = { Color = first_background } })
    table.insert(items, { Text = surface.left })
  end
  append(items, body)
  if surface.right ~= "" and not omit_right then
    if bar_background then
      table.insert(items, { Background = { Color = bar_background } })
    else
      table.insert(items, "ResetAttributes")
    end
    table.insert(items, { Foreground = { Color = last_background } })
    table.insert(items, { Text = surface.right })
  end
  if include_gap then
    self:append_gap(items, surface, bar_background)
  end
  return items
end

return Powerline
