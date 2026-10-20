local helper = require "tests.test_helper"

local function load_example(path, environment, overrides)
  local chunk, load_error = loadfile(path)
  assert(chunk, load_error)
  local wezterm = helper.fake_wezterm(overrides)
  wezterm.config_builder = function()
    return {}
  end
  local previous = package.loaded.wezterm
  local previous_getenv = os.getenv
  package.loaded.wezterm = wezterm
  if environment then
    os.getenv = function(name)
      return environment[name]
    end
  end
  local ok, config = pcall(chunk)
  package.loaded.wezterm = previous
  os.getenv = previous_getenv
  assert(ok, config)
  return config, wezterm
end

local function assert_profiles(config, domain)
  helper.assert_equal(#config.launch_menu, 3, "Windows shell profile count")
  helper.assert_table_equal(config.launch_menu[1].args, { "pwsh.exe", "-NoLogo" }, "PowerShell 7 argv")
  helper.assert_table_equal(config.launch_menu[2].args, { "powershell.exe", "-NoLogo" }, "Windows PowerShell argv")
  helper.assert_table_equal(
    config.launch_menu[3].args,
    { "C:/Program Files/Git/bin/bash.exe", "--login" },
    "Git Bash argv"
  )
  helper.assert_equal(config.launch_menu[3].set_environment_variables.CHERE_INVOKING, "1", "Git Bash preserves cwd")
  for _, profile in ipairs(config.launch_menu) do
    assert(type(profile.label) == "string" and profile.label ~= "", "profile needs a label")
    helper.assert_equal(profile.domain.DomainName, domain, "explicit shell profile domain")
  end
end

helper.test("remote client example uses a mux SSH domain and target-native shell profiles", function()
  local config = load_example "examples/remote/wezterm-client.lua"

  helper.assert_equal(#config.ssh_domains, 1, "SSH domain count")
  helper.assert_equal(config.ssh_domains[1].name, "workpc", "remote domain name")
  helper.assert_equal(config.ssh_domains[1].remote_address, "workpc", "SSH config host alias")
  helper.assert_equal(config.ssh_domains[1].multiplexing, "WezTerm", "persistent remote mux")
  helper.assert_equal(config.ssh_domains[1].assume_shell, nil, "no POSIX assumption for native Windows")
  helper.assert_equal(config.default_domain, "workpc", "client default domain")
  helper.assert_table_equal(config.default_gui_startup_args, { "connect", "workpc" }, "client startup")
  helper.assert_equal(config.default_prog, nil, "remote server chooses its default program")
  helper.assert_equal(config.keys, nil, "no sample key bindings")
  assert_profiles(config, "workpc")
end)

helper.test("remote server example hosts local processes without GUI-only callbacks", function()
  local config, wezterm = load_example "examples/remote/wezterm-server.lua"

  helper.assert_equal(#config.unix_domains, 1, "server socket domain count")
  helper.assert_equal(config.unix_domains[1].name, "wisp-remote", "server socket domain name")
  helper.assert_equal(config.unix_domains[1].skip_permissions_check, nil, "socket ownership checks remain enabled")
  helper.assert_equal(config.default_domain, "local", "server must not recursively attach to itself")
  helper.assert_table_equal(
    config.default_prog,
    { "C:/Program Files/Git/bin/bash.exe", "--login" },
    "Git Bash is the server default shell"
  )
  helper.assert_equal(config.set_environment_variables.CHERE_INVOKING, "1", "default Git Bash preserves cwd")
  helper.assert_table_equal(
    config.default_gui_startup_args,
    { "connect", "wisp-remote" },
    "target GUI attaches to server"
  )
  helper.assert_equal(#config.ssh_domains, 0, "no implicit SSH domains on server")
  helper.assert_equal(#config.wsl_domains, 0, "no implicit WSL domains on server")
  helper.assert_equal(next(wezterm.events), nil, "headless server has no GUI callbacks")
  helper.assert_equal(config.keys, nil, "no sample key bindings")
  assert_profiles(config, "wisp-remote")
end)

local manual_marker = string.rep("a", 24)
local manual_root = "C:/Users/test/Temp/w-man-" .. manual_marker
local manual_environment = {
  WISP_MANUAL_VERSION = "1",
  WISP_MANUAL_MARKER = manual_marker,
  WISP_MANUAL_ROOT = manual_root,
  WISP_MANUAL_LOCAL_ROOT = "/var/tmp/m-owned",
  WISP_MANUAL_PROXY = "owned proxy description",
}
local manual_proxy = {
  probe_version = 1,
  marker = manual_marker,
  remote_root = manual_root,
  args = { "/usr/bin/node", "/repo/tests/remote_mux_proxy.mjs", "/var/tmp/m-owned/proxy.json" },
}
local function manual_overrides(proxy)
  return {
    json_parse = function(source)
      helper.assert_equal(source, manual_environment.WISP_MANUAL_PROXY, "explicit proxy description")
      return proxy or manual_proxy
    end,
  }
end

helper.test("manual server isolates its socket and defaults to Git Bash without PowerShell 7", function()
  local config, wezterm = load_example("examples/remote/wezterm-manual-server.lua", manual_environment)
  helper.assert_equal(config.automatically_reload_config, false, "manual config is immutable")
  helper.assert_equal(config.unix_domains[1].name, "wisp-manual-target", "target socket domain")
  helper.assert_equal(config.unix_domains[1].socket_path, manual_root .. "/s", "exact owned socket")
  helper.assert_equal(config.unix_domains[1].no_serve_automatically, true, "no implicit mux startup")
  helper.assert_equal(config.default_domain, "local", "server spawns natively")
  helper.assert_table_equal(config.default_prog, { "C:/Program Files/Git/bin/bash.exe", "--login" }, "Git Bash default")
  helper.assert_equal(config.set_environment_variables.CHERE_INVOKING, "1", "preserve requested cwd")
  helper.assert_equal(config.set_environment_variables.WISP_MANUAL_MARKER, manual_marker, "owned shell marker")
  helper.assert_equal(config.exit_behavior, "Close", "exited shells do not leave held panes")
  helper.assert_equal(#config.launch_menu, 2, "no unverified PowerShell 7 launcher")
  helper.assert_table_equal(config.launch_menu[1].args, { "powershell.exe", "-NoLogo" }, "explicit Windows PowerShell")
  helper.assert_equal(config.launch_menu[1].domain.DomainName, "wisp-manual-target", "native GUI targets owned server")
  helper.assert_table_equal(config.default_gui_startup_args, { "connect", "wisp-manual-target" }, "Windows GUI attach")
  helper.assert_equal(#config.ssh_domains, 0, "no other SSH domains")
  helper.assert_equal(#config.wsl_domains, 0, "no WSL substitution")
  helper.assert_equal(config.keys, nil, "no consumer key bindings")
  helper.assert_equal(next(wezterm.events), nil, "no automatic launches or GUI callbacks")
end)

helper.test("manual Mac client uses an owned argv proxy and keeps local launches local", function()
  local config, wezterm =
    load_example("examples/remote/wezterm-manual-client.lua", manual_environment, manual_overrides())
  helper.assert_equal(config.automatically_reload_config, false, "manual config is immutable")
  helper.assert_equal(#config.ssh_domains, 0, "no native libssh route")
  helper.assert_equal(#config.unix_domains, 1, "one explicit owned proxy domain")
  helper.assert_equal(config.unix_domains[1].name, "wisp-manual-remote", "remote domain")
  helper.assert_equal(config.unix_domains[1].socket_path, "/var/tmp/m-owned/p", "separate proxy socket")
  helper.assert_equal(config.unix_domains[1].no_serve_automatically, true, "no implicit server startup")
  helper.assert_table_equal(config.unix_domains[1].proxy_command, manual_proxy.args, "managed proxy argv")
  helper.assert_table_equal(config.default_gui_startup_args, { "connect", "wisp-manual-remote" }, "attachment only")
  helper.assert_equal(config.default_domain, "local", "local picker presentation remains local")
  helper.assert_equal(#config.launch_menu, 1, "no Mac-generated Windows spawn commands")
  helper.assert_equal(config.launch_menu[1].domain.DomainName, "local", "local shell launcher")
  helper.assert_table_equal(config.launch_menu[1].args, { "/bin/zsh", "-l" }, "native Mac shell")
  helper.assert_equal(config.keys, nil, "no consumer key bindings")
  helper.assert_equal(next(wezterm.events), nil, "no automatic launches")
end)

helper.test("manual configs reject incompatible versions before other environment values", function()
  local incompatible = setmetatable({ WISP_MANUAL_VERSION = "2" }, {
    __index = function()
      error "interpreted incompatible manual settings"
    end,
  })
  for _, name in ipairs { "client", "server" } do
    local ok, failure = pcall(load_example, "examples/remote/wezterm-manual-" .. name .. ".lua", incompatible)
    helper.assert_equal(ok, false, "incompatible manual config must fail")
    assert(tostring(failure):find("version", 1, true), "version must be checked first")
  end
end)

helper.test("manual configs reject unsafe roots and foreign proxy identities", function()
  for _, root in ipairs {
    "C:/other",
    "C:/Temp/../w-man-" .. manual_marker,
    "C:/space value/w-man-" .. manual_marker,
    "C:/" .. string.rep("x", 110) .. "/w-man-" .. manual_marker,
  } do
    local environment = {}
    for name, value in pairs(manual_environment) do
      environment[name] = value
    end
    environment.WISP_MANUAL_ROOT = root
    for _, name in ipairs { "client", "server" } do
      local ok =
        pcall(load_example, "examples/remote/wezterm-manual-" .. name .. ".lua", environment, manual_overrides())
      helper.assert_equal(ok, false, "unsafe owned root must fail")
    end
  end
  for _, update in ipairs {
    { probe_version = 2 },
    { marker = string.rep("b", 24) },
    { remote_root = "C:/other" },
    { args = { "relative-node", "/repo/proxy.mjs", "/var/tmp/m-owned/proxy.json" } },
    { args = { "/usr/bin/node", "/repo/proxy.mjs", "/var/tmp/other/proxy.json" } },
  } do
    local proxy = {}
    for name, value in pairs(manual_proxy) do
      proxy[name] = value
    end
    for name, value in pairs(update) do
      proxy[name] = value
    end
    local ok =
      pcall(load_example, "examples/remote/wezterm-manual-client.lua", manual_environment, manual_overrides(proxy))
    helper.assert_equal(ok, false, "foreign proxy settings must fail")
  end
end)

helper.test("manual proxy version is checked before interpreting its fields", function()
  local incompatible = setmetatable({ probe_version = 2 }, {
    __index = function()
      error "interpreted incompatible proxy fields"
    end,
  })
  local ok, failure =
    pcall(load_example, "examples/remote/wezterm-manual-client.lua", manual_environment, manual_overrides(incompatible))
  helper.assert_equal(ok, false, "incompatible proxy must fail")
  assert(tostring(failure):find("version", 1, true), "proxy version must be checked first")
end)

helper.test("manual Mac client rejects relative, oversized, and NUL-bearing local roots", function()
  for _, root in ipairs { "relative", "/" .. string.rep("x", 104), "/tmp/nul\0path" } do
    local environment = {}
    for name, value in pairs(manual_environment) do
      environment[name] = value
    end
    environment.WISP_MANUAL_LOCAL_ROOT = root
    local ok = pcall(load_example, "examples/remote/wezterm-manual-client.lua", environment, manual_overrides())
    helper.assert_equal(ok, false, "invalid local root must fail")
  end
end)
