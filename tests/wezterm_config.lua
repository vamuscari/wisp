local wezterm = require "wezterm"
local config = wezterm.config_builder()
config.disable_default_key_bindings = true
assert(type(wezterm.format {
  { Attribute = { Invisible = true } },
  { Text = "status" },
  { Attribute = { Invisible = false } },
}) == "string", "minimum WezTerm must support status visibility attributes")
local tab_refresh_marker = "\u{200b}"
assert(
  wezterm.column_width(tab_refresh_marker) == 0,
  "minimum WezTerm must render the tab refresh marker at zero width"
)
assert(
  wezterm.format { { Attribute = { Intensity = "Bold" } }, { Text = tab_refresh_marker } }
    ~= wezterm.format { { Attribute = { Intensity = "Normal" } }, { Text = tab_refresh_marker } },
  "minimum WezTerm must preserve zero-width status refresh attributes"
)

local root = assert(wezterm.config_dir:match "^(.*)[/\\]tests$", "could not resolve the Wisp test root")
local wisp = assert(loadfile(root .. "/wezterm/init.lua"))("wisp", "wisp-deployment-v7", root .. "/wezterm")

wisp.apply_to_config(config, {
  opencode_tab_colors = true,
  picker_binding = { key = "f", mods = "CTRL|SHIFT" },
  spawn_domain = { DomainName = "local" },
})

table.insert(config.keys, { key = "r", mods = "CTRL|SHIFT", action = wisp.refresh_cache_action() })
table.insert(config.keys, { key = "h", mods = "CTRL|SHIFT", action = wisp.switch_to_project_action "home" })
table.insert(config.keys, { key = "w", mods = "CTRL|SHIFT", action = wisp.window_picker_action() })
table.insert(config.keys, { key = "t", mods = "CTRL|SHIFT", action = wisp.new_tab_action() })
table.insert(config.keys, { key = "s", mods = "CTRL|SHIFT", action = wisp.split_pane_action("Right", false) })

return config
