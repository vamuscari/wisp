import assert from "node:assert/strict"
import { spawn as spawnChild } from "node:child_process"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"
import { muxSocketPath, runProbeCommand, waitForReadiness } from "./remote_mux_support.mjs"
import { withProbeCleanup } from "./remote_mux_lifecycle.mjs"

import * as client from "./remote_mux_client.mjs"
const options = {
  host: "workpc", remoteRoot: "C:/Temp/w-ssh-test", socket: "/approved/w-test/s",
  metadata: "/approved/w-test/state.json", marker: "run-a", generation: "connection-1",
}
const spawn = {
  args: ["C:\\Program Files\\nodejs\\node.exe", "C:\\probe.mjs", "", "space value", 'double"quote', "single'quote",
    "snow-\u96ea", "emoji-\ud83d\ude80", "semi;&|<>", "$literal", "line\n\t\u0001", "123"],
}

test("the Mac client rejects remote command construction", () => {
  assert.throws(() => client.renderMuxClient({ ...options, spawn }), /target-side/)
})

test("the fixture Lua encoder preserves boolean configuration values", () => {
  assert.equal(client.luaValue(true), "true")
  assert.equal(client.luaValue(false), "false")
})

async function runConfig(t, request, mode = "ok") {
  assert.equal(typeof client.renderMuxClient, "function")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-client-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const path = join(root, "client.lua")
  const source = client.renderMuxClient(request)
  assert.match(source, /^[\x00-\x7f]*$/, "generated Lua must encode request data, not embed Unicode source")
  await writeFile(path, source)
  const result = await runProbeCommand(process.env.WISP_TEST_LUA ?? "lua",
    [fileURLToPath(new URL("./fixtures/remote-mux/client-config-check.lua", import.meta.url)), path, mode], { input: "" })
  const fields = new Map()
  for (const line of result.stdout.trim().split("\n")) {
    const [key, hex] = line.split("=")
    const values = fields.get(key) ?? []
    values.push(Buffer.from(hex ?? "", "hex").toString("utf8"))
    fields.set(key, values)
  }
  return fields
}

test("local structured Lua spawning preserves every argv byte", async (t) => {
  const fields = await runConfig(t, { ...options, domain: "local", spawn: { ...spawn, cwd: "/owned/local cwd" } }, "local-spawn")
  assert.deepEqual(fields.get("arg"), spawn.args)
  assert.deepEqual(fields.get("cwd"), ["/owned/local cwd"])
  assert.deepEqual(fields.get("domain"), ["local"])
  assert.deepEqual(fields.get("stage"), ["attaching", "spawning", "ready"])
  assert.deepEqual(fields.get("spawned_pane_id"), ["19"])
  assert.deepEqual(fields.get("marker"), [options.marker])
  assert.deepEqual(fields.get("generation"), [options.generation])
})

test("reconnect configuration attaches without spawning a duplicate process", async (t) => {
  const fields = await runConfig(t, options)
  assert.equal(fields.has("cwd"), false)
  assert.equal(fields.has("arg"), false)
  assert.deepEqual(fields.get("stage"), ["attaching", "ready"])
})

test("generated SSH configuration explicitly enables native libssh diagnostics", async (t) => {
  const fields = await runConfig(t, options)
  assert.deepEqual(fields.get("ssh_verbose"), ["true"])
})

test("a loopback diagnostic endpoint uses an explicit validated port", async (t) => {
  const fields = await runConfig(t, { ...options, host: "127.0.0.1", sshPort: 34567 })
  assert.deepEqual(fields.get("ssh_address"), ["127.0.0.1:34567"])
  const defaultPort = await runConfig(t, options)
  assert.deepEqual(defaultPort.get("ssh_address"), [options.host], "ordinary SSH aliases keep their configured port")
})

test("diagnostic SSH ports cannot inject shell syntax or select an invalid port", () => {
  for (const sshPort of [0, -1, 65536, 1.5, "22;echo-injected", null, NaN]) {
    assert.throws(() => client.renderMuxClient({ ...options, sshPort }), /SSH.*port/)
  }
})

test("a local two-mux control uses an argv proxy without SSH or the server's socket listener", async (t) => {
  const transport = { kind: "local-proxy", socket: options.socket + "p", args: ["/usr/bin/env", "WEZTERM_UNIX_SOCKET=/owned/server/s", "/owned/wezterm", "cli", "proxy"] }
  const fields = await runConfig(t, { ...options, transport })
  assert.deepEqual(fields.get("proxy_arg"), transport.args)
  assert.deepEqual(fields.get("proxy_socket"), [transport.socket])
  assert.deepEqual(fields.get("stage"), ["attaching", "ready"])
})

test("OpenSSH-backed configuration carries only managed transport argv, not a remote command", async (t) => {
  const transport = { kind: "openssh", socket: options.socket + "p", args: [process.execPath, "/owned/proxy.mjs", "/owned/request with 'quotes'.json"] }
  const fields = await runConfig(t, { ...options, transport })
  assert.deepEqual(fields.get("proxy_arg"), transport.args)
  assert.deepEqual(fields.get("transport"), ["openssh"])
  assert.equal(fields.has("arg"), false)
  assert.equal(fields.has("cwd"), false)
})

test("fixture snapshots exclude unrelated terminal user variables", async (t) => {
  const fields = await runConfig(t, options, "private-data")
  assert.deepEqual(fields.get("private"), ["nil"])
})

test("client snapshots retain the explicitly named raw-route diagnostic", async (t) => {
  const fields = await runConfig(t, options, "raw-diagnostic")
  assert.deepEqual(fields.get("raw"), ["diagnostic-only"])
})

test("mux fixture transport selection is explicit and rejects invalid proxy argv", () => {
  for (const transport of [{ kind: "unknown" }, { kind: "openssh", socket: options.socket, args: ["/owned/ssh"] },
    { kind: "openssh", socket: options.socket + "p", args: ["relative"] },
    { kind: "local-proxy", socket: options.socket + "p", args: ["/owned/ssh", "bad\0arg"] }]) {
    assert.throws(() => client.renderMuxClient({ ...options, transport }), /transport|proxy/)
  }
})

test("an owned detach signal detaches only the selected domain without spawning", async (t) => {
  const fields = await runConfig(t, { ...options, detachFile: options.metadata + ".detach" }, "detach")
  assert.deepEqual(fields.get("stage"), ["attaching", "ready", "detached"])
  assert.equal(fields.has("arg"), false)
})

test("OpenSSH mux argv honors the original alias and keeps request data out of the remote command", () => {
  assert.equal(typeof client.openSshMuxArgs, "function")
  const argv = client.openSshMuxArgs({ ssh: "/owned/Open SSH/ssh", host: "tsd1000-public", remoteRoot: options.remoteRoot })
  assert.equal(argv[0], "/owned/Open SSH/ssh")
  assert.ok(argv.includes("StrictHostKeyChecking=yes"))
  assert.ok(argv.includes("BatchMode=yes"))
  assert.ok(argv.includes("ControlMaster=no"))
  assert.ok(argv.includes("ControlPersist=no"))
  assert.ok(argv.includes("ForkAfterAuthentication=no"))
  assert.deepEqual(argv.slice(-2), ["tsd1000-public", `WEZTERM_UNIX_SOCKET=${options.remoteRoot}/s wezterm.exe --config-file ${options.remoteRoot}/server.lua cli --prefer-mux --no-auto-start proxy`])
  assert.equal(argv.includes("-J"), false, "the existing SSH alias owns jump routing, not a generated replacement")
  for (const update of [{ ssh: "relative" }, { host: "host;injected" }, { remoteRoot: "C:/Temp/path with spaces" }]) {
    assert.throws(() => client.openSshMuxArgs({ ssh: "/owned/ssh", host: "workpc", remoteRoot: options.remoteRoot, ...update }))
  }
})

test("an attachment failure becomes an explicit failed snapshot", async (t) => {
  const fields = await runConfig(t, options, "attach-error")
  assert.equal(fields.has("arg"), false)
  assert.deepEqual(fields.get("stage"), ["attaching", "failed"])
  assert.match(fields.get("error")[0], /attaching failed/)
})

test("a spawn failure never publishes ready state", async (t) => {
  const fields = await runConfig(t, { ...options, domain: "local", spawn: { ...spawn, cwd: "/owned/local cwd" } }, "spawn-error")
  assert.deepEqual(fields.get("stage"), ["attaching", "spawning", "failed"])
  assert.match(fields.get("error")[0], /spawning failed/)
  assert.doesNotMatch(fields.get("error")[0], /FIXTURE_SECRET|do-not-record/)
})

test("local spawn cwd and argv fail validation before rendering", () => {
  assert.equal(typeof client.renderMuxClient, "function")
  for (const cwd of ["relative", "bad\0path"]) {
    assert.throws(() => client.renderMuxClient({ ...options, domain: "local", spawn: { ...spawn, cwd } }), /local cwd/)
  }
  assert.throws(() => client.renderMuxClient({ ...options, domain: "local", spawn: { ...spawn, cwd: "/owned/local", args: ["node.exe", "bad\0arg"] } }), /argv/)
})

test("remote paths never enter proxy shell syntax without bootstrap validation", () => {
  assert.equal(typeof client.renderMuxClient, "function")
  assert.throws(() => client.renderMuxClient({ ...options, remoteRoot: "C:/Temp/path;echo-injected" }), /remote.*root/)
  assert.throws(() => client.renderMuxClient({ ...options, host: "workpc;echo-injected" }), /SSH.*alias/)
})

test("Windows cwd assertions compare with Windows rules, not client path rules", () => {
  assert.equal(typeof client.assertWindowsCwd, "function")
  client.assertWindowsCwd("c:\\work area \u96ea\\project", "C:/work area \u96ea/project")
  assert.throws(() => client.assertWindowsCwd("/Users/mac/C:/work area/project", "C:/work area/project"), /cwd/)
  assert.throws(() => client.assertWindowsCwd("C:/other", "C:/work area/project"), /cwd/)
})

test("generated Lua spawns through a real isolated local mux without CLI cwd decoding", {
  skip: process.platform === "win32" || !(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
    ? "set WISP_TEST_WEZTERM and WISP_TEST_MUX_SERVER for a POSIX-only owned local mux control" : false,
  timeout: 30_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM
  const mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm), "set an absolute WISP_TEST_WEZTERM")
  assert.ok(mux && isAbsolute(mux), "set an absolute WISP_TEST_MUX_SERVER")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX"))
  const root = await mkdtemp(join(parent, "w-"))
  const socket = muxSocketPath(root)
  const cwd = join(root, "work area \u96ea")
  const metadata = join(root, "state.json")
  const output = join(root, "report.json")
  const stop = join(root, "stop")
  const config = join(root, "client.lua")
  const marker = "local-structured-spawn"
  const nativeArgs = spawn.args.slice(2)
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("WEZTERM_")))
  Object.assign(env, {
    HOME: join(root, "home"), USERPROFILE: join(root, "home"), WEZTERM_UNIX_SOCKET: socket,
    XDG_CONFIG_HOME: join(root, "config"), XDG_CACHE_HOME: join(root, "cache"),
    XDG_DATA_HOME: join(root, "data"), XDG_RUNTIME_DIR: root, WISP_MUX_PROBE_MARKER: marker,
  })
  let server, closed, report
  let logs = ""
  try {
    await withProbeCleanup(async () => {
      for (const path of [cwd, env.HOME, env.XDG_CONFIG_HOME, env.XDG_CACHE_HOME, env.XDG_DATA_HOME]) await mkdir(path)
      const cliVersion = (await runProbeCommand(wezterm, ["--version"], { env, input: "" })).stdout.trim().split(/\s+/).at(-1)
      const muxVersion = (await runProbeCommand(mux, ["--version"], { env, input: "" })).stdout.trim().split(/\s+/).at(-1)
      assert.equal(cliVersion, muxVersion, "local CLI/mux versions match")
      t.diagnostic(`owned local structured-spawn control: WezTerm ${cliVersion}`)
      await writeFile(config, client.renderMuxClient({
        ...options, socket, metadata, marker, domain: "local",
        spawn: {
          cwd,
          args: [process.execPath, fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), output, stop, ...nativeArgs],
        },
      }))
      server = spawnChild(mux, ["--config-file", config], { env, cwd: root, stdio: ["ignore", "pipe", "pipe"] })
      closed = new Promise((resolve) => server.once("close", resolve))
      server.on("error", (error) => { logs += error.message })
      for (const stream of [server.stdout, server.stderr]) stream.on("data", (bytes) => { logs = (logs + bytes).slice(-16_384) })
      const snapshot = await waitForReadiness(server, async (signal) => {
        const value = JSON.parse(await readFile(metadata, { encoding: "utf8", signal }))
        assert.equal(value.probe_version, 1)
        if (value.stage === "failed") throw new Error(value.error)
        return value.stage === "ready" && value
      }, { timeout: 10_000, retryError: (error) => error.code === "ENOENT" })
      assert.ok(Number.isSafeInteger(snapshot.spawned_pane_id))
      report = await waitForReadiness(server, async (signal) => {
        const value = JSON.parse(await readFile(output, { encoding: "utf8", signal }))
        assert.equal(value.probe_version, 1)
        return value
      }, { timeout: 5000, retryError: (error) => error.code === "ENOENT" })
      assert.equal(report.marker, marker)
      assert.equal(report.cwd, cwd)
      assert.deepEqual(report.args, nativeArgs)
    }, [
      ["stop owned probe", async () => {
        await writeFile(stop, "stop")
        if (report) await waitForReadiness(null, async () => {
          try { process.kill(report.pid, 0); return false } catch (error) { if (error.code === "ESRCH") return true; throw error }
        }, { timeout: 5000 })
      }],
      ["stop owned mux", async () => {
        if (!server) return
        if (server.exitCode === null) server.kill()
        const timer = setTimeout(() => server.kill("SIGKILL"), 2000)
        try { await closed } finally { clearTimeout(timer) }
      }],
      ["remove owned files", () => rm(root, { recursive: true, force: true })],
    ])
  } catch (error) { throw new Error(`${error.message}\n${logs}`, { cause: error }) }
})
