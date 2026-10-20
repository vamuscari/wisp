import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"
import { luaValue } from "./remote_mux_client.mjs"
import { withProbeCleanup } from "./remote_mux_lifecycle.mjs"
import { muxSocketPath, runProbeCommand, waitForReadiness } from "./remote_mux_support.mjs"
import { renderMuxServer } from "./remote_mux_server.mjs"

const helper = fileURLToPath(new URL("./fixtures/remote-mux/target-observe.lua", import.meta.url))
const marker = "a".repeat(24), generation = "b".repeat(24), root = "C:/Temp/w-ssh-" + marker
const request = { probe_version: 1, action: "observe", marker, generation, cookie: marker + ":42:1", sequence: 1, phase: "before" }

async function observe(t, update = {}) {
  const directory = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-observe-"))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const path = join(directory, "input.lua")
  await writeFile(path, "return " + luaValue({ root, marker, request, next_request: { ...request, phase: "after" },
    before: { WISP_TEST_ATTACHMENT: "initial", PRIVATE_FIXTURE: "do-not-record" },
    after: { WISP_TEST_ATTACHMENT: "fresh", WISP_TEST_RAW_ACK: "descriptor-proof", PRIVATE_FIXTURE: "do-not-record" }, mode: "ok", ...update }))
  const result = await runProbeCommand(process.env.WISP_TEST_LUA ?? "lua", [
    fileURLToPath(new URL("./fixtures/remote-mux/target-observe-check.lua", import.meta.url)), helper, path,
  ], { input: "" })
  const fields = new Map()
  for (const line of result.stdout.trim().split("\n")) {
    const [key, hex] = line.split("="), values = fields.get(key) ?? []
    values.push(Buffer.from(hex ?? "", "hex").toString("utf8")); fields.set(key, values)
  }
  return fields
}

test("the target observer records paired filtered snapshots without spawning", async (t) => {
  const fields = await observe(t)
  assert.deepEqual(fields.get("startup_stage"), ["started"])
  assert.deepEqual(fields.get("timer_stage"), ["ticking"])
  assert.deepEqual(fields.get("before_version"), ["1"])
  assert.deepEqual(fields.get("after_version"), ["1"])
  assert.deepEqual(fields.get("before_token"), ["initial"])
  assert.deepEqual(fields.get("after_token"), ["fresh"])
  assert.deepEqual(fields.get("after_raw"), ["descriptor-proof"])
  assert.deepEqual(fields.get("after_private"), ["nil"])
  assert.deepEqual(fields.get("after_pane"), ["7"])
  assert.ok(Buffer.byteLength(await readFile(helper)) <= 5120)
})

test("target observations reject incompatible or foreign requests before recording panes", async (t) => {
  for (const mode of ["wrong-version", "oversized"]) {
    const fields = await observe(t, { mode })
    assert.equal(fields.has("before_pane"), false)
    assert.deepEqual(fields.get("error_stage"), ["failed"])
  }
  for (const update of [{ marker: "foreign" }, { generation: "invalid" }, { phase: "../escape" }, { sequence: 0 }]) {
    const fields = await observe(t, { request: { ...request, ...update } })
    assert.equal(fields.has("before_pane"), false)
    assert.deepEqual(fields.get("error_stage"), ["failed"])
  }
})

test("a repeated observation phase is not republished over a Windows destination", async (t) => {
  const fields = await observe(t, { next_request: request })
  assert.deepEqual(fields.get("before_token"), ["initial"])
  assert.equal(fields.has("after_token"), false)
  assert.equal(fields.has("error_stage"), false)
})

test("observation reports do not retain unrelated request fields", async (t) => {
  const fields = await observe(t, { request: { ...request, PRIVATE_FIXTURE: "do-not-record" } })
  assert.deepEqual(fields.get("before_private_request"), ["nil"])
})

test("an owned foreground mux survives state-file traffic outside HOME", {
  skip: process.platform === "win32" || !(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
    ? "set both WezTerm binaries for the POSIX foreground startup control" : false,
  timeout: 15_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM, mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm) && mux && isAbsolute(mux), "absolute test binaries required")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX"))
  const directory = await mkdtemp(join(parent, "w-")), socket = muxSocketPath(directory)
  const config = join(directory, "server.lua"), output = join(directory, "keeper.json"), stop = join(directory, "stop")
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("WEZTERM_")))
  Object.assign(env, { HOME: join(directory, "home"), USERPROFILE: join(directory, "home"), WEZTERM_UNIX_SOCKET: socket,
    XDG_CONFIG_HOME: directory, XDG_CACHE_HOME: join(directory, "cache"), XDG_DATA_HOME: join(directory, "data"),
    XDG_RUNTIME_DIR: directory, WISP_MUX_PROBE_MARKER: marker, WEZTERM_LOG: "config=debug" })
  let child, closed, report, stopped = false, logs = ""
  try {
    await withProbeCleanup(async () => {
      await mkdir(env.HOME); await mkdir(env.XDG_CACHE_HOME); await mkdir(env.XDG_DATA_HOME)
      const version = async (executable) => (await runProbeCommand(executable, ["--version"], { input: "", env })).stdout.trim().split(/\s+/).at(-1)
      assert.equal(await version(wezterm), await version(mux))
      await writeFile(config, renderMuxServer({ root: directory, marker, node: process.execPath,
        helpers: fileURLToPath(new URL("./fixtures/remote-mux/", import.meta.url)), defaultProg: ["/usr/bin/true"] }))
      child = spawn(mux, ["--config-file", config, "--cwd", directory, "--", process.execPath,
        fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), output, stop, "keeper"],
      { env, cwd: directory, stdio: ["ignore", "pipe", "pipe"], windowsHide: true })
      closed = new Promise((resolve) => child.once("close", resolve))
      child.on("error", (error) => { logs = error.message })
      for (const stream of [child.stdout, child.stderr]) stream.on("data", (bytes) => { logs = (logs + bytes).slice(-4096) })
      const read = (name, predicate = () => true) => waitForReadiness(child, async () => {
        const value = JSON.parse(await readFile(join(directory, name), "utf8"))
        assert.equal(value.probe_version, 1)
        return predicate(value) && value
      },
        { timeout: 4000, retryError: (error) => error.code === "ENOENT" })
      report = await read("keeper.json")
      assert.equal(report.probe_version, 1); assert.equal(report.marker, marker)
      for (const [phase, stage] of [["startup", "started"], ["timer", "ticking"]]) {
        const value = await read(`observation.${phase}.json`)
        assert.equal(value.probe_version, 1); assert.equal(value.marker, marker); assert.equal(value.stage, stage)
      }
      // Exercise state-file traffic outside HOME as on Windows. Each fresh
      // acknowledgement is a causal barrier, not a guessed sleep for reload.
      for (let sequence = 1; sequence <= 8; sequence++) {
        await writeFile(output + ".request.json.tmp", JSON.stringify({ probe_version: 1, action: "barrier", marker,
          generation, cookie: report.cookie, sequence }))
        await rename(output + ".request.json.tmp", output + ".request.json")
        await read("keeper.json.ack.json", (value) => value.sequence === sequence)
      }
      await writeFile(join(directory, "observation.request.json.tmp"), JSON.stringify({ ...request, cookie: report.cookie, sequence: 8 }))
      await assert.rejects(readFile(join(directory, "observation.request.json")), { code: "ENOENT" },
        "the observer request must remain invisible until atomic publication")
      await rename(join(directory, "observation.request.json.tmp"), join(directory, "observation.request.json"))
      const observed = await read("observation.before.json")
      assert.equal(observed.stage, "observed")
      assert.equal(observed.sequence, 8)
      assert.equal((logs.match(/Reloaded configuration!/g) ?? []).length, 1, "state-file writes must not reload immutable fixture config")
      t.diagnostic("owned foreground mux: startup, timer, and observation survived eight state updates outside HOME")
    }, [
      ["stop startup keeper", async () => {
        await writeFile(stop, "stop")
        if (report) await waitForReadiness(null, () => {
          try { process.kill(report.pid, 0); return false } catch (error) { if (error.code === "ESRCH") return true; throw error }
        }, { timeout: 3000 })
      }],
      ["stop owned foreground mux", async () => {
        if (child && child.exitCode === null && !child.signalCode) child.kill("SIGTERM")
        const timer = setTimeout(() => child?.kill("SIGKILL"), 2000)
        try { await closed; stopped = true } finally { clearTimeout(timer) }
      }],
      ["remove startup files", async () => { assert.ok(stopped); await rm(directory, { recursive: true, force: true }) }],
    ])
  } catch (error) { throw new Error(error.message + "\n" + logs, { cause: error }) }
})
