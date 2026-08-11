local wezterm = require "wezterm"
local config = wezterm.config_builder()
config.disable_default_key_bindings = true

local root = assert(wezterm.config_dir:match "^(.*)[/\\]tests$", "could not resolve the Wisp test root")
local wisp = assert(loadfile(root .. "/wezterm/init.lua"))("wisp", "wisp-deployment-v3", root .. "/wezterm")

wisp.apply_to_config(config, {
  picker_binding = { key = "f", mods = "CTRL|SHIFT" },
  spawn_domain = { DomainName = "local" },
})

table.insert(config.keys, { key = "r", mods = "CTRL|SHIFT", action = wisp.refresh_cache_action() })
table.insert(config.keys, { key = "h", mods = "CTRL|SHIFT", action = wisp.switch_to_project_action "home" })
table.insert(config.keys, { key = "w", mods = "CTRL|SHIFT", action = wisp.window_picker_action() })
table.insert(config.keys, { key = "t", mods = "CTRL|SHIFT", action = wisp.new_tab_action() })
table.insert(config.keys, { key = "s", mods = "CTRL|SHIFT", action = wisp.split_pane_action("Right", false) })

return config
