-- Feasibility example for native Windows, not a remote Wisp configuration.
-- Use as the remote account's default WezTerm configuration so SSH proxy
-- startup and the mux server resolve the same socket and settings.
local wezterm = require "wezterm"
local config = wezterm.config_builder()

config.unix_domains = {
  { name = "wisp-remote" },
}
-- The server must spawn native local processes, not attach to its own socket.
config.default_domain = "local"
config.default_prog = { "C:/Program Files/Git/bin/bash.exe", "--login" }
config.set_environment_variables = { CHERE_INVOKING = "1" }
config.ssh_domains = {}
config.wsl_domains = {}

-- A GUI launched on Windows attaches to the same persistent server.
-- Headless server startup does not require GUI event handlers.
config.default_gui_startup_args = { "connect", "wisp-remote" }
config.launch_menu = {
  {
    label = "PowerShell 7",
    domain = { DomainName = "wisp-remote" },
    args = { "pwsh.exe", "-NoLogo" },
  },
  {
    label = "Windows PowerShell",
    domain = { DomainName = "wisp-remote" },
    args = { "powershell.exe", "-NoLogo" },
  },
  {
    label = "Git Bash",
    domain = { DomainName = "wisp-remote" },
    args = { "C:/Program Files/Git/bin/bash.exe", "--login" },
    set_environment_variables = { CHERE_INVOKING = "1" },
  },
}

return config
