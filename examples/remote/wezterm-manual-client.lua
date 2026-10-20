-- Isolated, user-run Mac acceptance only; no remote spawn commands or Wisp adapter.
local wezterm = require "wezterm"
assert(os.getenv "WISP_MANUAL_VERSION" == "1", "unsupported manual fixture version")
local marker = os.getenv "WISP_MANUAL_MARKER"
assert(marker and #marker == 24 and marker:match "^[a-f0-9]+$", "invalid manual marker")
local root = os.getenv "WISP_MANUAL_ROOT"
assert(
  root
    and root:match("^[A-Za-z]:/[A-Za-z0-9_./-]+/w%-man%-" .. marker .. "$")
    and not root:find("/../", 1, true)
    and not root:find("/./", 1, true)
    and not root:find("//", 1, true)
    and #root + 2 <= 107,
  "invalid or overlong owned manual root"
)
local local_root = os.getenv "WISP_MANUAL_LOCAL_ROOT"
assert(
  local_root and local_root:sub(1, 1) == "/" and not local_root:find "%z" and #local_root + 2 <= 103,
  "invalid or overlong local manual root"
)
local source = os.getenv "WISP_MANUAL_PROXY"
assert(source and #source < 16384, "missing or oversized owned proxy description")
local proxy = wezterm.json_parse(source)
assert(type(proxy) == "table" and proxy.probe_version == 1, "unsupported manual proxy version")
assert(proxy.marker == marker and proxy.remote_root == root, "foreign manual proxy identity")
assert(type(proxy.args) == "table" and #proxy.args == 3, "invalid manual proxy argv")
for _, value in ipairs(proxy.args) do
  assert(type(value) == "string" and value:sub(1, 1) == "/" and not value:find "%z", "invalid manual proxy path")
end
assert(proxy.args[3] == local_root .. "/proxy.json", "foreign manual proxy request path")
local config = wezterm.config_builder()
config.automatically_reload_config = false
config.unix_domains = {
  {
    name = "wisp-manual-remote",
    socket_path = local_root .. "/p",
    no_serve_automatically = true,
    proxy_command = proxy.args,
  },
}
config.default_mux_server_domain = "local"
config.default_domain = "local"
config.default_prog = { "/bin/zsh", "-l" }
config.ssh_domains = {}
config.wsl_domains = {}
config.default_gui_startup_args = { "connect", "wisp-manual-remote" }
config.launch_menu = {
  { label = "Manual: Mac local shell", domain = { DomainName = "local" }, args = { "/bin/zsh", "-l" } },
}
return config
