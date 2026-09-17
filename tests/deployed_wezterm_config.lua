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

local config_home = assert(os.getenv "XDG_CONFIG_HOME", "XDG_CONFIG_HOME is required")
local wisp = dofile(config_home .. "/wezterm/wisp/init.lua")
wisp.apply_to_config(config, {
  opencode_tab_colors = true,
  picker_binding = { key = "f", mods = "CTRL|SHIFT" },
  spawn_domain = { DomainName = "local" },
})

return config
