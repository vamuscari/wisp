local wezterm = require "wezterm"
local config = wezterm.config_builder()
config.disable_default_key_bindings = true
assert(type(wezterm.format {
  { Attribute = { Invisible = true } },
  { Text = "status" },
  { Attribute = { Invisible = false } },
}) == "string", "minimum WezTerm must support status visibility attributes")

local config_home = assert(os.getenv "XDG_CONFIG_HOME", "XDG_CONFIG_HOME is required")
local wisp = dofile(config_home .. "/wezterm/wisp/init.lua")
wisp.apply_to_config(config, {
  picker_binding = { key = "f", mods = "CTRL|SHIFT" },
  spawn_domain = { DomainName = "local" },
})

return config
