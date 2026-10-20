import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { randomBytes } from "node:crypto"
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"
import { luaValue, renderMuxClient } from "./remote_mux_client.mjs"
import { withProbeCleanup } from "./remote_mux_lifecycle.mjs"
import { muxSocketPath, runProbeCommand, waitForReadiness } from "./remote_mux_support.mjs"
import * as server from "./remote_mux_server.mjs"

const helper = fileURLToPath(new URL("./fixtures/remote-mux/target-spawn.lua", import.meta.url))
const marker = "a".repeat(24), generation = "b".repeat(24), root = "C:/Temp/w-ssh-" + marker
const node = "C:/Program Files/nodejs/node.exe"
const request = {
  probe_version: 1, action: "spawn", marker, generation, cookie: "owned-cookie", sequence: 1,
  cwd: root + "/work area \u96ea/project's dir",
  args: [node, root + "/probe.mjs", root + "/argv.json", root + "/stop", "", "space value", 'double"quote',
    "single'quote", "snow-\u96ea", "emoji-\ud83d\ude80", "semi;&|<>", "$literal", "line\n\t\u0001"],
}
const ack = { probe_version: 1, action: "publish", status: "published", marker, generation, cookie: request.cookie, sequence: 1 }

test("generated target config disables reload and loads only the two fixed helpers", async () => {
  assert.equal(typeof server.renderMuxServer, "function")
  const source = server.renderMuxServer({ root, marker, node })
  assert.match(source, /^[\x00-\x7f]*$/)
  const script = `package.preload.wezterm=function() return {} end
local helpers={}
loadfile=function(path) helpers[#helpers+1]=path; return function() end end
local config=(function()\n${source}\nend)()
assert(config.automatically_reload_config==false, 'immutable target config must not reload')
assert(config.default_domain=='local' and #config.ssh_domains==0 and #config.wsl_domains==0)
assert(helpers[1]==${luaValue(root + "/target-spawn.lua")})
assert(helpers[2]==${luaValue(root + "/target-observe.lua")})
assert(config.default_prog[1]=='C:/Program Files/Git/bin/bash.exe')
`
  await runProbeCommand(process.env.WISP_TEST_LUA ?? "lua", ["-"], { input: script })
})

async function checkHelper(t, update = {}) {
  const directory = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-server-"))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const path = join(directory, "request.lua")
  await writeFile(path, "return " + luaValue({ root, marker, node, request, ack, mode: "ok", ...update }))
  const { stdout } = await runProbeCommand(process.env.WISP_TEST_LUA ?? "lua", [
    fileURLToPath(new URL("./fixtures/remote-mux/target-spawn-check.lua", import.meta.url)), helper, path,
  ], { input: "" })
  const fields = new Map()
  for (const line of stdout.trim().split("\n")) {
    const [key, hex] = line.split("="), values = fields.get(key) ?? []
    values.push(Buffer.from(hex ?? "", "hex").toString("utf8")); fields.set(key, values)
  }
  return fields
}

test("the target constructs a local native command once with every cwd/argv byte", async (t) => {
  const fields = await checkHelper(t)
  assert.deepEqual(fields.get("arg"), request.args)
  assert.deepEqual(fields.get("cwd"), [request.cwd])
  assert.deepEqual(fields.get("domain"), ["local"])
  assert.deepEqual(fields.get("calls"), ["1"])
  assert.deepEqual(fields.get("stage"), ["ready"])
  assert.deepEqual(fields.get("pane_id"), ["19"])
  assert.ok(Buffer.byteLength(await readFile(helper)) <= 5120, "the helper must fit one bounded stage request")
})

test("target spawn requires a unique keeper acknowledgement with the exact identity", async (t) => {
  for (const mode of ["foreign-server", "duplicate-keeper", "wrong-version", "wrong-ack-version", "oversized"]) {
    const fields = await checkHelper(t, { mode })
    assert.deepEqual(fields.get("calls"), ["0"], mode)
    assert.deepEqual(fields.get("stage"), ["failed"], mode)
  }
  for (const update of [{ marker: "foreign" }, { generation: "c".repeat(24) }, { sequence: 2 }, { cookie: "foreign" }, { action: "barrier" }]) {
    const fields = await checkHelper(t, { ack: { ...ack, ...update } })
    assert.deepEqual(fields.get("calls"), ["0"])
    assert.deepEqual(fields.get("stage"), ["failed"])
  }
})

test("the target cannot replay a claimed request or spawn without a request", async (t) => {
  for (const mode of ["claimed", "missing-request"]) {
    const fields = await checkHelper(t, { mode })
    assert.deepEqual(fields.get("calls"), ["0"])
  }
})

test("target command validation rejects foreign executables, relative cwd, and NUL bytes", async (t) => {
  for (const update of [{ cwd: "C:relative" }, { cwd: "relative" }, { cwd: "C:/bad\0path" },
    { args: ["other.exe", ...request.args.slice(1)] }, { args: [...request.args, "bad\0arg"] },
    { args: [node, root + "/other.mjs", ...request.args.slice(2)] }]) {
    const fields = await checkHelper(t, { request: { ...request, ...update } })
    assert.deepEqual(fields.get("calls"), ["0"])
    assert.deepEqual(fields.get("stage"), ["failed"])
  }
})

test("target spawn failures do not copy command/environment diagnostics", async (t) => {
  const fields = await checkHelper(t, { mode: "spawn-error" })
  assert.deepEqual(fields.get("stage"), ["failed"])
  assert.doesNotMatch(fields.get("error")[0], /FIXTURE_SECRET|do-not-record/)
})

test("an owned local mux executes the target helper with its native cwd, argv, and environment", {
  skip: process.platform === "win32" || !(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
    ? "set both WezTerm binaries for the POSIX target-native helper control" : false,
  timeout: 20_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM, mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(isAbsolute(wezterm) && isAbsolute(mux), "absolute test binaries required")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX"))
  const directory = await mkdtemp(join(parent, "w-")), run = randomBytes(12).toString("hex")
  const socket = muxSocketPath(directory), stop = join(directory, "stop"), metadata = join(directory, "state.json")
  const cwd = join(directory, "work area \u96ea"), config = join(directory, "server.lua")
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("WEZTERM_")))
  Object.assign(env, { HOME: directory, USERPROFILE: directory, WEZTERM_UNIX_SOCKET: socket,
    XDG_CONFIG_HOME: directory, XDG_CACHE_HOME: join(directory, "cache"), XDG_DATA_HOME: join(directory, "data"),
    XDG_RUNTIME_DIR: directory, WISP_MUX_PROBE_MARKER: run, WISP_TEST_TARGET_ENV: run })
  const readJSON = async (name) => JSON.parse(await readFile(join(directory, name), "utf8"))
  const retryError = (error) => error.code === "ENOENT"
  const wait = (inspect) => waitForReadiness(child, inspect, { timeout: 5000, retryError })
  const publish = async (name, value) => {
    await writeFile(join(directory, name + ".tmp"), JSON.stringify(value))
    await rename(join(directory, name + ".tmp"), join(directory, name))
  }
  let child, closed, keeper, native, stopped = false, logs = ""
  try {
    await withProbeCleanup(async () => {
      for (const path of [cwd, env.XDG_CACHE_HOME, env.XDG_DATA_HOME]) await mkdir(path)
      const version = async (executable) => (await runProbeCommand(executable, ["--version"], { input: "", env })).stdout.trim().split(/\s+/).at(-1)
      assert.equal(await version(wezterm), await version(mux))
      await writeFile(join(directory, "probe.mjs"), await readFile(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)))
      const localConfig = renderMuxClient({ host: "unused", remoteRoot: "C:/unused", socket, metadata, marker: run, generation,
        domain: "local", spawn: { cwd: directory, args: [process.execPath, directory + "/probe.mjs", directory + "/keeper.json", stop] } })
      await writeFile(config, `assert(loadfile(${luaValue(helper)}))(require('wezterm'),${luaValue(directory)},${luaValue(run)},${luaValue(process.execPath)})\n` + localConfig)
      child = spawn(mux, ["--config-file", config], { env, cwd: directory, stdio: ["ignore", "pipe", "pipe"] })
      closed = new Promise((resolve) => child.once("close", resolve))
      child.on("error", (error) => { logs = error.message })
      for (const stream of [child.stdout, child.stderr]) stream.on("data", (bytes) => { logs = (logs + bytes).slice(-4096) })
      keeper = await wait(async () => {
        const value = await readJSON("keeper.json")
        assert.equal(value.probe_version, 1); assert.equal(value.marker, run)
        return value
      })
      const publication = { probe_version: 1, action: "publish", marker: run, generation, cookie: keeper.cookie, sequence: 1 }
      await publish("keeper.json.request.json", publication)
      await wait(async () => (await readJSON("state.json")).panes.some((pane) => {
        if (!pane.vars?.WISP_TEST_ATTACHMENT_ACK) return false
        const ack = JSON.parse(pane.vars.WISP_TEST_ATTACHMENT_ACK)
        assert.equal(ack.probe_version, 1)
        return ack.generation === generation && ack.sequence === 1 && ack.cookie === keeper.cookie
      }))
      const nativeArgs = request.args.slice(4)
      await publish("spawn.request.json", { ...publication, action: "spawn", cwd,
        args: [process.execPath, directory + "/probe.mjs", directory + "/argv.json", stop, ...nativeArgs] })
      const result = await wait(async () => {
        const value = await readJSON("spawn.result.json")
        assert.equal(value.probe_version, 1)
        if (value.stage === "failed") throw new Error(value.error)
        return value.stage === "ready" && value
      })
      native = await wait(() => readJSON("argv.json"))
      assert.equal(native.probe_version, 1)
      assert.equal(native.marker, run)
      assert.equal(native.cwd, cwd)
      assert.deepEqual(native.args, nativeArgs)
      assert.equal(native.target_env, run, "the target-created command inherits the target environment")
      assert.equal(String(native.pane_id), String(result.pane_id))
      t.diagnostic("target-side JSON helper: owned local native spawn and keeper gate passed")
    }, [
      ["stop target probes", async () => {
        await writeFile(stop, "stop")
        for (const report of [keeper, native].filter(Boolean)) await waitForReadiness(null, () => {
          try { process.kill(report.pid, 0); return false } catch (error) { if (error.code === "ESRCH") return true; throw error }
        }, { timeout: 3000 })
      }],
      ["stop target mux", async () => {
        if (child && child.exitCode === null && !child.signalCode) child.kill("SIGTERM")
        const timer = setTimeout(() => child?.kill("SIGKILL"), 2000)
        try { await closed; stopped = true } finally { clearTimeout(timer) }
      }],
      ["remove target staging", async () => { assert.ok(stopped); await rm(directory, { recursive: true, force: true }) }],
    ])
  } catch (error) { throw new Error(error.message + "\n" + logs, { cause: error }) }
})
