local helper = require "tests.test_helper"

helper.test("sample WezTerm config loads Wisp and defines consumer bindings", function()
  local wezterm = helper.fake_wezterm()
  wezterm.config_dir = "/Users/test/.config/wezterm"
  wezterm.config_builder = function()
    return {}
  end
  local wisp = helper.load_wezterm_adapter(wezterm)
  local original_dofile = dofile
  local loaded_path
  _G.dofile = function(path)
    loaded_path = path
    return wisp
  end

  local chunk, load_error = loadfile "examples/wezterm.lua"
  assert(chunk, load_error)
  local ok, config = pcall(chunk)
  _G.dofile = original_dofile
  assert(ok, config)

  helper.assert_equal(loaded_path, "/Users/test/.config/wezterm/wisp/init.lua", "deployed loader path")
  helper.assert_equal(config.leader.key, "Space", "leader key")
  helper.assert_equal(config.leader.mods, "CTRL", "leader modifiers")
  helper.assert_equal(#config.keys, 6, "sample binding count")
  helper.assert_equal(config.keys[1].key, "p", "project popup binding")
  helper.assert_equal(config.keys[2].key, "w", "window picker binding")
  helper.assert_equal(config.keys[3].key, "o", "OpenCode picker binding")
  helper.assert_equal(config.keys[4].key, "r", "refresh binding")
  helper.assert_equal(config.keys[5].key, "t", "new tab binding")
  helper.assert_equal(config.keys[6].key, "\\", "split binding")
end)
