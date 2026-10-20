import assert from "node:assert/strict"
import { isAbsolute, win32 } from "node:path"

export function luaValue(value) {
  if (value === null) return "nil"
  if (typeof value === "string") {
    const bytes = [...Buffer.from(value, "utf8")].map((byte) => byte >= 32 && byte < 127 && byte !== 34 && byte !== 92
      ? String.fromCharCode(byte) : `\\${String(byte).padStart(3, "0")}`)
    return '"' + bytes.join("") + '"'
  }
  if (typeof value === "number" || typeof value === "boolean") return String(value)
  if (Array.isArray(value)) return `{${value.map(luaValue).join(",")}}`
  return `{${Object.entries(value).map(([key, item]) => `[${luaValue(key)}]=${luaValue(item)}`).join(",")}}`
}

function windowsCwd(value) {
  return typeof value === "string" && !value.includes("\0") && win32.isAbsolute(value)
    && (/^[A-Za-z]:[\\/]/.test(value) || /^\\\\[^\\]+\\[^\\]+/.test(value))
}

export function assertWindowsCwd(actual, expected) {
  assert.ok(windowsCwd(actual) && windowsCwd(expected), "remote process must report an absolute Windows cwd")
  assert.equal(win32.normalize(actual).toLowerCase(), win32.normalize(expected).toLowerCase(), "remote process cwd differs from the requested Windows cwd")
}

export const muxSshOptions = ["-T", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "UpdateHostKeys=no",
  "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=1",
  "-S", "none", "-o", "ControlMaster=no", "-o", "ControlPersist=no", "-o", "ForkAfterAuthentication=no"]

function remoteProxyCommand(remoteRoot) {
  // This fixture explicitly targets the existing Git Bash SSH entry. Pin the
  // socket in that process: CLI selection precedes --config-file loading in the
  // pinned WezTerm, so the config argument alone cannot select an owned server.
  return `WEZTERM_UNIX_SOCKET=${remoteRoot}/s wezterm.exe --config-file ${remoteRoot}/server.lua cli --prefer-mux --no-auto-start proxy`
}

export function openSshMuxArgs({ ssh, host, remoteRoot }) {
  assert.ok(typeof ssh === "string" && isAbsolute(ssh) && !ssh.includes("\0"), "expected an absolute SSH executable")
  assert.match(host, /^[A-Za-z0-9][A-Za-z0-9._-]*$/, "use a configured SSH alias")
  assert.match(remoteRoot, /^[A-Za-z]:\/[A-Za-z0-9_./-]+$/, "unsupported remote bootstrap root")
  return [ssh, "-v", ...muxSshOptions, host, remoteProxyCommand(remoteRoot)]
}

export function renderMuxClient({ host, sshPort, remoteRoot, socket, metadata, detachFile, marker, generation, spawn, domain = "remote-test", transport = { kind: "native-ssh" } }) {
  assert.match(host, /^[A-Za-z0-9][A-Za-z0-9._-]*$/, "use a configured SSH alias")
  assert.ok(sshPort === undefined || Number.isInteger(sshPort) && sshPort > 0 && sshPort <= 65535, "invalid SSH diagnostic port")
  assert.match(remoteRoot, /^[A-Za-z]:\/[A-Za-z0-9_./-]+$/, "unsupported remote bootstrap root")
  for (const [name, value] of Object.entries({ socket, metadata })) {
    assert.ok(typeof value === "string" && !value.includes("\0") && isAbsolute(value), `${name} must be an absolute client path`)
  }
  if (detachFile !== undefined) assert.ok(isAbsolute(detachFile) && !detachFile.includes("\0"), "invalid owned detach signal path")
  assert.ok(typeof marker === "string" && marker.length > 0, "missing run marker")
  assert.ok(typeof generation === "string" && generation.length > 0, "missing connection generation")
  assert.ok(domain === "remote-test" || domain === "local", "unsupported fixture domain")
  assert.ok(["native-ssh", "local-proxy", "openssh"].includes(transport?.kind), "unsupported fixture transport")
  if (transport.kind !== "native-ssh") {
    assert.ok(domain === "remote-test" && isAbsolute(transport.socket) && transport.socket !== socket
      && !transport.socket.includes("\0"), "argv proxy must use a separate owned client socket")
    assert.ok(Array.isArray(transport.args) && transport.args.length > 0 && isAbsolute(transport.args[0])
      && transport.args.every((arg) => typeof arg === "string" && !arg.includes("\0")), "invalid proxy argv")
  }
  if (spawn) {
    assert.equal(domain, "local", "remote command construction requires the target-side spawn helper")
    assert.ok(isAbsolute(spawn.cwd) && !spawn.cwd.includes("\0"), "expected an absolute native local cwd")
    assert.ok(Array.isArray(spawn.args) && spawn.args.length > 0 && [...spawn.args]
      .every((arg) => typeof arg === "string" && !arg.includes("\0")) && spawn.args[0].length > 0, "invalid native argv")
  }
  const request = {
    probe_version: 1, socket, metadata, marker, generation, host: sshPort === undefined ? host : `${host}:${sshPort}`, domain,
    proxy: remoteProxyCommand(remoteRoot),
    spawn: spawn ? { args: spawn.args, cwd: spawn.cwd, domain: { DomainName: domain } } : null,
    detach_file: detachFile ?? null, transport: transport.kind, argv_proxy: transport.kind === "native-ssh" ? null : { socket: transport.socket, args: transport.args },
  }
  return `local wezterm = require 'wezterm'
local request = ${luaValue(request)}
assert(request.probe_version == 1, "unsupported client probe request version")
local config = {
  automatically_reload_config=false,
  unix_domains = {{name="test-local", socket_path=request.socket, no_serve_automatically=true}},
  default_mux_server_domain="local", default_prog={"/usr/bin/true"}, exit_behavior="Hold", wsl_domains={},
  ssh_domains = {{name="remote-test", remote_address=request.host, multiplexing="WezTerm",
    override_proxy_command=request.proxy,
    ssh_option={BatchMode="yes", StrictHostKeyChecking="yes", UpdateHostKeys="no", ConnectTimeout="10", wezterm_ssh_verbose="true"}}},
}
if request.argv_proxy then
  config.ssh_domains = {}
  table.insert(config.unix_domains, {name="remote-test", socket_path=request.argv_proxy.socket,
    no_serve_automatically=true, proxy_command=request.argv_proxy.args})
end
local stage, spawned_pane_id, failure = "attaching", nil, nil
local function panes_snapshot()
  local panes = {}
  for _, window in ipairs(wezterm.mux.all_windows()) do
    for _, tab in ipairs(window:tabs()) do
      for _, pane in ipairs(tab:panes()) do
        local source = pane:get_user_vars() or {}
        local vars = {WISP_TEST_ATTACHMENT=source.WISP_TEST_ATTACHMENT, WISP_TEST_ATTACHMENT_ACK=source.WISP_TEST_ATTACHMENT_ACK,
          WISP_TEST_BARRIER=source.WISP_TEST_BARRIER, WISP_TEST_RAW_ACK=source.WISP_TEST_RAW_ACK}
        table.insert(panes, {pane_id=pane:pane_id(), domain=pane:get_domain_name(), vars=vars})
      end
    end
  end
  return panes
end
local function snapshot()
  local out = assert(io.open(request.metadata .. ".tmp", "w"))
  out:write(wezterm.json_encode({probe_version=1, marker=request.marker, generation=request.generation,
    transport=request.transport, stage=stage, spawned_pane_id=spawned_pane_id, error=failure, panes=panes_snapshot()}))
  out:close()
  assert(os.rename(request.metadata .. ".tmp", request.metadata))
end
local function fail()
  -- WezTerm spawn errors can include Debug(CommandBuilder) and its entire
  -- environment. Never copy that diagnostic into fixture snapshots/evidence.
  failure, stage = "mux " .. stage .. " failed", "failed"
end
local function spawn_requested()
  stage = "spawning"
  snapshot()
  local _, pane = wezterm.mux.spawn_window(request.spawn)
  spawned_pane_id, stage = pane:pane_id(), "ready"
end
local function tick()
  if stage == "ready" and request.detach_file then
    local file = io.open(request.detach_file, "r")
    if file then
      file:close()
      local ok = pcall(function() wezterm.mux.get_domain(request.domain):detach() end)
      stage = ok and "detached" or "failed"
      if not ok then failure = "mux detach failed" end
    end
  end
  snapshot()
  wezterm.time.call_after(0.1, tick)
end
wezterm.on("mux-startup", function()
  snapshot()
  wezterm.time.call_after(0.1, tick)
  local ok = pcall(function()
    wezterm.mux.get_domain(request.domain):attach()
    if request.spawn then spawn_requested()
    else stage = "ready" end
  end)
  if not ok then fail() end
  snapshot()
end)
return config
`
}
