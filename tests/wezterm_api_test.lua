package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

helper.test("apply_to_config installs no default binding", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  local existing = { key = "x", mods = "CTRL", action = "existing" }
  local config = { keys = { existing } }

  wisp.apply_to_config(config, {})

  helper.assert_equal(#config.keys, 1, "key count")
  helper.assert_equal(config.keys[1], existing, "existing key")
end)

helper.test("apply_to_config appends a configured picker binding", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  local existing = { key = "x", mods = "CTRL", action = "existing" }
  local config = { keys = { existing } }

  wisp.apply_to_config(config, {
    picker_binding = { key = "f", mods = "LEADER" },
  })

  helper.assert_equal(#config.keys, 2, "key count")
  helper.assert_equal(config.keys[1], existing, "existing key")
  helper.assert_equal(config.keys[2].key, "f", "picker key")
  helper.assert_equal(config.keys[2].mods, "LEADER", "picker modifiers")
  helper.assert_equal(config.keys[2].action.kind, "Callback", "picker action")
end)
