local wezterm = require "wezterm"
local config = wezterm.config_builder()

config.leader = { key = "Space", mods = "CTRL", timeout_milliseconds = 1000 }

local wisp = dofile(wezterm.config_dir .. "/wisp/init.lua")
wisp.apply_to_config(config, {
  spawn_domain = { DomainName = "local" },
  status_items = {
    { name = "opencode", action = "sessions" },
    { name = "directory", action = "projects" },
  },
  popup = { direction = "Bottom", size = 0.65 },
  window_preview = true,
})

-- Wisp installs no default mappings; these bindings are owned by this config.
config.keys = config.keys or {}
table.insert(config.keys, { key = "p", mods = "LEADER", action = wisp.popup_action "projects" })
table.insert(config.keys, { key = "w", mods = "LEADER", action = wisp.window_picker_action() })
table.insert(config.keys, { key = "o", mods = "LEADER", action = wisp.opencode_picker_action() })
table.insert(config.keys, { key = "r", mods = "LEADER", action = wisp.refresh_cache_action() })
table.insert(config.keys, { key = "t", mods = "LEADER", action = wisp.new_tab_action() })
table.insert(config.keys, { key = "\\", mods = "LEADER", action = wisp.split_pane_action("Right", false) })

return config
