-- Feasibility example only: Wisp does not support remote projects yet.
-- "workpc" must be a configured SSH host for a native Windows account.
local wezterm = require "wezterm"
local config = wezterm.config_builder()

config.ssh_domains = {
  {
    name = "workpc",
    remote_address = "workpc",
    multiplexing = "WezTerm",
  },
}
config.default_domain = "workpc"
config.default_gui_startup_args = { "connect", "workpc" }

-- These programs run on Windows, not on the computer running this GUI.
-- Keep default_prog unset so unspecified launches use the server's default.
config.launch_menu = {
  {
    label = "workpc: PowerShell 7",
    domain = { DomainName = "workpc" },
    args = { "pwsh.exe", "-NoLogo" },
  },
  {
    label = "workpc: Windows PowerShell",
    domain = { DomainName = "workpc" },
    args = { "powershell.exe", "-NoLogo" },
  },
  {
    label = "workpc: Git Bash",
    domain = { DomainName = "workpc" },
    args = { "C:/Program Files/Git/bin/bash.exe", "--login" },
    set_environment_variables = { CHERE_INVOKING = "1" },
  },
}

return config
