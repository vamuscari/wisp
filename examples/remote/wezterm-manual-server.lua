-- Isolated, user-run Windows acceptance only; never install as a default config.
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
local config = wezterm.config_builder()
config.automatically_reload_config = false
config.unix_domains = {
  { name = "wisp-manual-target", socket_path = root .. "/s", no_serve_automatically = true },
}
config.default_mux_server_domain = "local"
config.default_domain = "local"
config.default_prog = { "C:/Program Files/Git/bin/bash.exe", "--login" }
config.set_environment_variables = { CHERE_INVOKING = "1", WISP_MANUAL_MARKER = marker }
config.ssh_domains = {}
config.wsl_domains = {}
config.exit_behavior = "Close"
config.daemon_options = { stdout = root .. "/mux.stdout", stderr = root .. "/mux.stderr" }
config.default_gui_startup_args = { "connect", "wisp-manual-target" }
config.launch_menu = {
  {
    label = "Manual: Windows PowerShell",
    domain = { DomainName = "wisp-manual-target" },
    args = { "powershell.exe", "-NoLogo" },
  },
  {
    label = "Manual: Git Bash",
    domain = { DomainName = "wisp-manual-target" },
    args = { "C:/Program Files/Git/bin/bash.exe", "--login" },
    set_environment_variables = { CHERE_INVOKING = "1" },
  },
}
return config
