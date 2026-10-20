// Configuration renderer for immutable, owned target mux fixtures.
import assert from "node:assert/strict"
import { isAbsolute, win32 } from "node:path"
import { luaValue } from "./remote_mux_client.mjs"

export function renderMuxServer({ root, marker, node, helpers = root, defaultProg = ["C:/Program Files/Git/bin/bash.exe", "--login"] }) {
  for (const path of [root, node, helpers]) {
    assert.ok(typeof path === "string" && !path.includes("\0") && (isAbsolute(path) || win32.isAbsolute(path)), "expected an absolute target fixture path")
  }
  assert.match(marker, /^[a-f0-9]{24}$/, "invalid target fixture marker")
  assert.ok(Array.isArray(defaultProg) && defaultProg.length > 0
    && defaultProg.every((arg) => typeof arg === "string" && !arg.includes("\0")), "invalid target default program")
  const config = {
    automatically_reload_config: false,
    unix_domains: [{ name: "test-server", socket_path: root + "/s", no_serve_automatically: true }],
    default_mux_server_domain: "local", default_domain: "local", default_prog: defaultProg,
    ssh_domains: [], wsl_domains: [], exit_behavior: "Hold",
    daemon_options: { stdout: root + "/mux.stdout", stderr: root + "/mux.stderr" },
    set_environment_variables: { CHERE_INVOKING: "1", WISP_MUX_PROBE_MARKER: marker, WISP_TEST_TARGET_ENV: marker,
      WISP_TEST_EMISSION_DIAGNOSTICS: "1", HOME: root, USERPROFILE: root },
  }
  const directory = helpers.replace(/[\\/]$/, "")
  return `local wezterm = require 'wezterm'
assert(loadfile(${luaValue(directory + "/target-spawn.lua")}))(wezterm,${luaValue(root)},${luaValue(marker)},${luaValue(node)})
assert(loadfile(${luaValue(directory + "/target-observe.lua")}))(wezterm,${luaValue(root)},${luaValue(marker)})
return ${luaValue(config)}
`
}
