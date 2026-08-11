local wezterm = require "wezterm"
local config = wezterm.config_builder()
config.disable_default_key_bindings = true

local config_home = assert(os.getenv "XDG_CONFIG_HOME", "XDG_CONFIG_HOME is required")
local wisp = dofile(config_home .. "/wezterm/wisp/init.lua")
wisp.apply_to_config(config, {
  picker_binding = { key = "f", mods = "CTRL|SHIFT" },
  spawn_domain = { DomainName = "local" },
})

return config
