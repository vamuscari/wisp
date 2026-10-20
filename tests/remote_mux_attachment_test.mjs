import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { createHash, randomBytes } from "node:crypto"
import { once } from "node:events"
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import test from "node:test"
import * as support from "./remote_mux_support.mjs"
import { luaValue, renderMuxClient } from "./remote_mux_client.mjs"
import { withProbeCleanup } from "./remote_mux_lifecycle.mjs"
import { prepareMuxProxy, readMuxProxy, stopMuxProxy } from "./remote_mux_proxy.mjs"
import { createPublicationCoordinator, requireRunningChannel } from "./remote_mux_scenarios.mjs"

const marker = "a".repeat(24)
const generation = "b".repeat(24)
const request = { probe_version: 1, action: "publish", marker, generation, cookie: `${marker}:42:123`, sequence: 1 }
const ack = { ...request, status: "published" }
const pane = { pane_id: 19, domain: "remote-test", vars: {
  WISP_TEST_ATTACHMENT: request.cookie, WISP_TEST_ATTACHMENT_ACK: JSON.stringify(ack),
} }
const snapshot = { probe_version: 1, marker, generation, stage: "ready", panes: [pane] }

test("token recovery requires a fresh acknowledgement in the current connection", () => {
  assert.equal(typeof support.attachmentRecovered, "function")
  assert.equal(support.attachmentRecovered(snapshot, request).pane_id, 19)
  assert.equal(support.attachmentRecovered({ ...snapshot, generation: "c".repeat(24) }, request), false)
  assert.equal(support.attachmentRecovered({ ...snapshot, marker: "foreign" }, request), false)
  assert.equal(support.attachmentRecovered(snapshot, { ...request, sequence: 2 }), false)
  assert.equal(support.attachmentRecovered({ ...snapshot, panes: [{ ...pane, domain: "local" }] }, request), false)
  assert.equal(support.attachmentRecovered({ ...snapshot, panes: [{ ...pane, vars: { WISP_TEST_ATTACHMENT: request.cookie } }] }, request), false)
  assert.equal(support.attachmentRecovered(snapshot, { ...request, cookie: "different" }), false)
  const oldAck = { ...pane, vars: { ...pane.vars, WISP_TEST_ATTACHMENT_ACK: JSON.stringify({ ...ack, generation: "c".repeat(24) }) } }
  assert.equal(support.attachmentRecovered({ ...snapshot, panes: [oldAck] }, request), false)
  assert.throws(() => support.attachmentRecovered(snapshot, { ...request, action: "barrier" }), /publication action/)
  const wrongAction = { ...pane, vars: { ...pane.vars, WISP_TEST_ATTACHMENT_ACK: JSON.stringify({ ...ack, action: "barrier" }) } }
  assert.equal(support.attachmentRecovered({ ...snapshot, panes: [wrongAction] }, request), false)
})

test("keeper association rejects a client stage that is not ready", () => {
  const incomplete = { ...snapshot, stage: "attached" }
  assert.equal(support.attachmentReady(incomplete, marker, generation), false)
  assert.equal(support.attachmentRecovered(incomplete, request), false)
})

test("a raw-descriptor diagnostic acknowledgement cannot authorize an attachment", () => {
  const diagnostic = { ...pane, vars: { WISP_TEST_RAW_ACK: JSON.stringify(ack) } }
  assert.equal(support.attachmentRecovered({ ...snapshot, panes: [diagnostic] }, request), false)
})

test("token recovery rejects incompatible metadata before reading identity fields", () => {
  assert.equal(typeof support.attachmentRecovered, "function")
  const invalid = { probe_version: 2 }
  Object.defineProperty(invalid, "marker", { get() { throw new Error("interpreted mismatched fields") } })
  assert.throws(() => support.attachmentRecovered(invalid, request), /version/)
  assert.throws(() => support.attachmentRecovered(snapshot, invalid), /version/)
  const incompatible = { ...pane, vars: { ...pane.vars, WISP_TEST_ATTACHMENT_ACK: '{"probe_version":2}' } }
  assert.throws(() => support.attachmentRecovered({ ...snapshot, panes: [incompatible] }, request), /version/)
  assert.throws(() => support.attachmentRecovered({ ...snapshot, panes: [pane, { ...pane, pane_id: 20 }] }, request), /ambiguous/)
})

test("absence is not accepted until an independent current-generation notification barrier arrives", () => {
  assert.equal(typeof support.attachmentAbsentAfterBarrier, "function")
  const barrier = { ...request, action: "barrier" }
  const withoutToken = { ...snapshot, panes: [{ ...pane, vars: {} }] }
  assert.equal(support.attachmentAbsentAfterBarrier(withoutToken, barrier), false)
  const barrierValue = JSON.stringify({ ...barrier, status: "barrier" })
  const arrived = { ...snapshot, panes: [{ ...pane, vars: { WISP_TEST_BARRIER: barrierValue } }] }
  assert.equal(support.attachmentAbsentAfterBarrier(arrived, barrier).pane_id, pane.pane_id)
  assert.equal(support.attachmentAbsentAfterBarrier(arrived, { ...barrier, generation: "c".repeat(24) }), false)
  assert.equal(support.attachmentAbsentAfterBarrier(arrived, { ...barrier, sequence: 2 }), false)
})

test("a delayed replay between ready and the ordered barrier fails the absence assertion", () => {
  assert.equal(typeof support.attachmentAbsentAfterBarrier, "function")
  const barrier = { ...request, action: "barrier" }
  const early = { ...snapshot, panes: [{ ...pane, vars: {} }] }
  const delayed = { ...snapshot, panes: [{ ...pane, vars: { WISP_TEST_ATTACHMENT: request.cookie } }] }
  const settled = { ...snapshot, panes: [{ ...pane, vars: {
    WISP_TEST_ATTACHMENT: request.cookie, WISP_TEST_BARRIER: JSON.stringify({ ...barrier, status: "barrier" }),
  } }] }
  assert.equal(support.attachmentAbsentAfterBarrier(early, barrier), false, "ready without a token is not evidence of absence")
  assert.equal(support.attachmentAbsentAfterBarrier(delayed, barrier), false, "wait for the same-pane barrier before deciding")
  assert.throws(() => support.attachmentAbsentAfterBarrier(settled, barrier), /replayed.*barrier/)
})

test("notification barriers reject unsupported versions and ambiguous pane associations", () => {
  assert.equal(typeof support.attachmentAbsentAfterBarrier, "function")
  const barrier = { ...request, action: "barrier" }
  const invalid = { probe_version: 2 }
  Object.defineProperty(invalid, "marker", { get() { throw new Error("interpreted mismatched fields") } })
  assert.throws(() => support.attachmentAbsentAfterBarrier(snapshot, invalid), /version/)
  assert.throws(() => support.attachmentAbsentAfterBarrier(invalid, barrier), /version/)
  const unsupported = { ...pane, vars: { WISP_TEST_BARRIER: '{"probe_version":2}' } }
  assert.throws(() => support.attachmentAbsentAfterBarrier({ ...snapshot, panes: [unsupported] }, barrier), /version/)
  const arrived = { ...pane, vars: { WISP_TEST_BARRIER: JSON.stringify({ ...barrier, status: "barrier" }) } }
  assert.throws(() => support.attachmentAbsentAfterBarrier({ ...snapshot, panes: [arrived, { ...arrived, pane_id: 20 }] }, barrier), /ambiguous/)
})

test("the original probe explicitly re-publishes its stable token and rejects stale requests", { timeout: 20_000 }, async (t) => {
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-token-"))
  const output = join(root, "report.json")
  const stop = join(root, "stop")
  let stdout = "", stderr = ""
  const child = spawn(process.execPath, [fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), output, stop], {
    env: { ...process.env, WISP_MUX_PROBE_MARKER: marker, WISP_TEST_EMISSION_DIAGNOSTICS: "1" }, stdio: ["ignore", "pipe", "pipe"],
  })
  const closed = once(child, "close")
  child.stdout.on("data", (data) => { stdout += data })
  child.stderr.on("data", (data) => { stderr += data })
  t.after(async () => {
    await writeFile(stop, "stop")
    const timer = setTimeout(() => child.kill("SIGKILL"), 2000)
    try { await closed } finally { clearTimeout(timer); await rm(root, { recursive: true, force: true }) }
  })
  const readJSON = async (path) => JSON.parse(await readFile(path, "utf8"))
  const retryError = (error) => error.code === "ENOENT"
  const report = await support.waitForReadiness(child, () => readJSON(output), { timeout: 2000, retryError })
  assert.equal(report.probe_version, 1)
  const publish = async (value) => {
    await rm(output + ".ack.json", { force: true })
    await writeFile(output + ".request.json.tmp", JSON.stringify(value))
    await rename(output + ".request.json.tmp", output + ".request.json")
    return support.waitForReadiness(child, () => readJSON(output + ".ack.json"), { timeout: 2000, retryError })
  }
  const current = { ...request, cookie: report.cookie }
  assert.equal((await publish(current)).status, "published", stderr)
  const emitted = await readJSON(output + ".emission.json")
  assert.equal(emitted.probe_version, 1)
  assert.equal(emitted.marker, marker)
  assert.equal(emitted.generation, generation)
  assert.equal(emitted.sequence, 1)
  assert.equal(typeof emitted.stdout_tty, "boolean")
  assert.equal(emitted.node, process.version)
  assert.equal(emitted.uv, process.versions.uv)
  const frame = (name, value) => `\x1b]1337;SetUserVar=${name}=${Buffer.from(value).toString("base64")}\x07`
  const acknowledgement = JSON.stringify({ probe_version: 1, action: "publish", marker, cookie: report.cookie,
    generation, sequence: 1, status: "published" })
  const expected = [frame("WISP_TEST_ATTACHMENT", report.cookie) + frame("WISP_TEST_ATTACHMENT_ACK", acknowledgement),
    frame("WISP_TEST_RAW_ACK", acknowledgement)]
  assert.deepEqual(emitted.routes.map((route) => route.route), ["stream", "descriptor"])
  for (const [index, route] of emitted.routes.entries()) {
    assert.equal(route.status, "completed")
    assert.equal(route.bytes, Buffer.byteLength(expected[index]))
    assert.equal(route.sha256, createHash("sha256").update(expected[index]).digest("hex"))
  }
  await support.waitForReadiness(child, () => stdout.includes("SetUserVar=WISP_TEST_RAW_ACK="), { timeout: 2000 })
  const raw = stdout.match(/SetUserVar=WISP_TEST_RAW_ACK=([^\x07]*)\x07/)[1]
  assert.deepEqual(JSON.parse(Buffer.from(raw, "base64").toString("utf8")), { ...current, status: "published" })
  await support.waitForReadiness(child, () => (stdout.match(/SetUserVar=WISP_TEST_ATTACHMENT=/g) ?? []).length === 2, { timeout: 2000 })
  assert.equal((stdout.match(/SetUserVar=WISP_TEST_ATTACHMENT=/g) ?? []).length, 2, "same token is emitted again, not replaced")
  const foreign = await publish({ ...current, sequence: 2, marker: "foreign" })
  assert.equal(foreign.status, "rejected")
  assert.match(foreign.error, /identity/)
  assert.equal((await publish({ ...current, sequence: 2, cookie: "different" })).status, "rejected")
  assert.equal((await publish({ ...current, sequence: 2, generation: "invalid" })).status, "rejected")
  const invalid = await publish({ ...current, probe_version: 2, sequence: 3 })
  assert.equal(invalid.status, "rejected")
  assert.match(invalid.error, /version/)
  assert.equal((await publish({ ...current, sequence: 4 })).status, "published")
  const stale = await publish({ ...current, sequence: 1 })
  assert.equal(stale.status, "rejected")
  assert.match(stale.error, /stale/)
  const reconnect = { ...current, sequence: 5, generation: randomBytes(12).toString("hex") }
  assert.equal((await publish(reconnect)).status, "published")
  const emissions = () => (stdout.match(/SetUserVar=WISP_TEST_ATTACHMENT(?:_ACK)?=/g) ?? []).length
  await support.waitForReadiness(child, () => emissions() === 7, { timeout: 2000 })
  const barrier = { ...reconnect, action: "barrier", sequence: 6 }
  assert.equal((await publish(barrier)).status, "barrier")
  await support.waitForReadiness(child, () => stdout.includes("SetUserVar=WISP_TEST_BARRIER="), { timeout: 2000 })
  assert.equal(emissions(), 7, "a barrier must not re-publish or acknowledge the attachment token")
  const encodedBarrier = stdout.match(/SetUserVar=WISP_TEST_BARRIER=([^\x07]*)\x07/)[1]
  assert.deepEqual(JSON.parse(Buffer.from(encodedBarrier, "base64").toString("utf8")), { ...barrier, status: "barrier" })
  assert.match((await publish({ ...reconnect, action: "unknown", sequence: 7 })).error, /action/)
  assert.equal((await readJSON(output)).cookie, report.cookie)
  assert.equal((await readJSON(output)).pid, child.pid)
})

test("a zero-byte diagnostic descriptor write cannot block publication or cleanup", { timeout: 10_000 }, async (t) => {
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-emission-"))
  const output = join(root, "report.json"), stop = join(root, "stop"), preload = join(root, "zero.mjs")
  await writeFile(preload, `import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
const write = fs.writeSync
fs.writeSync = (fd, bytes, ...args) => fd === 1 && Buffer.from(bytes).includes('WISP_TEST_RAW_ACK') ? 0 : write(fd, bytes, ...args)
syncBuiltinESMExports()
`)
  const child = spawn(process.execPath, ["--import", pathToFileURL(preload).href,
    fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), output, stop], {
    env: { ...process.env, WISP_MUX_PROBE_MARKER: marker, WISP_TEST_EMISSION_DIAGNOSTICS: "1" }, stdio: ["ignore", "pipe", "ignore"],
  })
  child.stdout.resume()
  const closed = once(child, "close")
  t.after(async () => {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
    await rm(root, { recursive: true, force: true })
  })
  const readJSON = async (path) => JSON.parse(await readFile(path, "utf8"))
  const retryError = (error) => error.code === "ENOENT"
  const report = await support.waitForReadiness(child, () => readJSON(output), { timeout: 2000, retryError })
  await writeFile(output + ".request.json", JSON.stringify({ ...request, cookie: report.cookie }))
  const ack = await support.waitForReadiness(child, () => readJSON(output + ".ack.json"), { timeout: 1500, retryError })
  assert.equal(ack.status, "published", "a failed diagnostic is not a canonical write failure")
  const measured = await readJSON(output + ".emission.json")
  assert.equal(measured.routes[1].status, "failed")
  await writeFile(stop, "stop")
  assert.equal((await closed)[0], 0)
})

test("two owned local muxes separate pre-attachment state, live publication, and fresh-client recovery", {
  skip: process.platform === "win32" || !(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
    ? "set both WezTerm binaries for a POSIX-only local two-mux control" : false,
  timeout: 40_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM, mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm) && mux && isAbsolute(mux), "absolute test binaries required")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  support.muxSocketPath(join(parent, "w-XXXXXX", "c2"))
  const root = await mkdtemp(join(parent, "w-"))
  const output = join(root, "keeper.json"), stop = join(root, "stop")
  const run = randomBytes(12).toString("hex")
  const publications = createPublicationCoordinator(run)
  const children = []
  const retryError = (error) => error.code === "ENOENT"
  let logs = "", report
  async function start(name, generation, serverSocket) {
    const directory = join(root, name)
    await mkdir(directory)
    const socket = support.muxSocketPath(directory)
    const metadata = join(directory, "state.json"), config = join(directory, "client.lua")
    const env = support.muxDiagnosticEnv(Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("WEZTERM_"))))
    Object.assign(env, {
      HOME: directory, USERPROFILE: directory, XDG_CONFIG_HOME: directory, XDG_CACHE_HOME: directory,
      XDG_DATA_HOME: directory, XDG_RUNTIME_DIR: directory, WEZTERM_UNIX_SOCKET: socket, WISP_MUX_PROBE_MARKER: run,
      WISP_TEST_EMISSION_DIAGNOSTICS: "1",
    })
    let transport = serverSocket ? {
      kind: "local-proxy", socket: join(directory, "p"), args: ["/usr/bin/env", `WEZTERM_UNIX_SOCKET=${serverSocket}`, wezterm,
        "--config-file", join(root, "s", "client.lua"), "cli", "--prefer-mux", "--no-auto-start", "proxy"],
    } : undefined
    const proxyPath = join(directory, "proxy.json"), detachFile = join(directory, "detach")
    const proxyRequest = { probe_version: 1, marker: run, generation, args: transport?.args }
    if (transport) {
      const managed = await prepareMuxProxy(proxyPath, proxyRequest)
      transport = { ...transport, args: managed.args }
    }
    const observer = serverSocket ? "" : `assert(loadfile(${luaValue(fileURLToPath(new URL("./fixtures/remote-mux/target-observe.lua", import.meta.url)))}))(require('wezterm'),${luaValue(root)},${luaValue(run)})\n`
    await writeFile(config, observer + renderMuxClient({
      host: "unused", remoteRoot: "C:/unused", socket, metadata, marker: run, generation,
      domain: serverSocket ? "remote-test" : "local", transport, detachFile: serverSocket ? detachFile : undefined,
      spawn: serverSocket ? undefined : { cwd: root, args: [process.execPath,
        fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), output, stop] },
    }))
    const child = spawn(mux, ["--config-file", config], { cwd: directory, env, stdio: ["ignore", "pipe", "pipe"] })
    const closed = new Promise((resolve) => child.once("close", resolve))
    child.on("error", (error) => { logs += String(error) })
    const stages = []
    for (const stream of [child.stdout, child.stderr]) {
      const capture = support.createMuxLogCapture((stage) => stages.push(stage))
      stream.on("data", (bytes) => { capture.write(bytes); logs = (logs + bytes).slice(-16_384) })
      stream.on("end", () => capture.end())
    }
    const handle = { child, closed, socket, metadata, generation, stages, detachFile, proxyPath: serverSocket ? proxyPath : undefined, proxyRequest }
    children.push(handle)
    await support.waitForReadiness(child, async () => {
      const value = JSON.parse(await readFile(metadata, "utf8"))
      assert.equal(value.probe_version, 1)
      if (value.stage === "failed") throw new Error(value.error)
      return value.stage === "ready" && value
    }, { timeout: 5000, retryError })
    if (serverSocket) {
      await support.waitForReadiness(child, () => stages.includes("codec-verified"), { timeout: 2000 })
      t.diagnostic(`${name}: native codec milestone captured from the isolated client's pipe`)
    }
    return handle
  }
  async function stopMux(handle, signal = "SIGTERM") {
    if (handle.child.exitCode === null && !handle.child.signalCode) {
      handle.child.kill(signal)
      const timer = setTimeout(() => handle.child.kill("SIGKILL"), 2000)
      try { await handle.closed } finally { clearTimeout(timer) }
    } else await handle.closed
    if (handle.proxyPath) {
      handle.proxyStopped = false
      await stopMuxProxy(handle.proxyPath, handle.proxyRequest)
      assert.equal((await readMuxProxy(handle.proxyPath, handle.proxyRequest)).group_gone, true)
      handle.proxyStopped = true
    }
  }
  const readSnapshot = async (handle) => JSON.parse(await readFile(handle.metadata, "utf8"))
  try {
    await withProbeCleanup(async () => {
      const version = (await support.runProbeCommand(wezterm, ["--version"], { input: "" })).stdout.trim().split(/\s+/).at(-1)
      const muxVersion = (await support.runProbeCommand(mux, ["--version"], { input: "" })).stdout.trim().split(/\s+/).at(-1)
      assert.equal(version, muxVersion)
      if (process.env.WISP_TEST_WEZTERM_VERSION) assert.equal(version, process.env.WISP_TEST_WEZTERM_VERSION)
      t.diagnostic(`owned local two-mux token control: WezTerm ${version}`)
      const server = await start("s", randomBytes(12).toString("hex"))
      report = await support.waitForReadiness(server.child, async () => JSON.parse(await readFile(output, "utf8")), { timeout: 2000, retryError })
      await support.waitForReadiness(server.child, async () => (await readSnapshot(server)).panes
        .some((pane) => pane.vars?.WISP_TEST_ATTACHMENT === report.cookie), { timeout: 2000 })
      const first = await start("c1", randomBytes(12).toString("hex"), server.socket)
      const submit = (handle, action, acknowledge) => publications.submit({ probe_version: 1, action, marker: run,
        generation: handle.generation, cookie: report.cookie }, async (publication) => {
        await writeFile(output + ".request.json.tmp", JSON.stringify(publication))
        await rename(output + ".request.json.tmp", output + ".request.json")
        return acknowledge(publication)
      })
      const absent = (handle) => submit(handle, "barrier", (barrier) =>
        support.waitForReadiness(handle.child, async () => {
          const snapshot = await readSnapshot(handle)
          return support.attachmentAbsentAfterBarrier(snapshot, barrier) && snapshot
        }, { timeout: 3000 }))
      const publish = (handle) => submit(handle, "publish", async (publication) => {
        const mapped = await support.waitForReadiness(handle.child, async () => support.attachmentRecovered(await readSnapshot(handle), publication),
          { timeout: 3000 })
        assert.ok(Number.isSafeInteger(mapped.pane_id), "association uses this client's pane ID")
        return publication
      })
      await absent(first)
      const intended = { probe_version: 1, action: "publish", marker: run, generation: first.generation, cookie: report.cookie, sequence: 2 }
      const observeTarget = async (phase) => {
        await writeFile(join(root, "observation.request.json.tmp"), JSON.stringify({ ...intended, action: "observe", phase }))
        await rename(join(root, "observation.request.json.tmp"), join(root, "observation.request.json"))
        return support.waitForReadiness(server.child, async () => support.validatePublicationObservation(
          JSON.parse(await readFile(join(root, `observation.${phase}.json`), "utf8")), intended, phase,
        ), { timeout: 3000, retryError })
      }
      const before = await observeTarget("before")
      assert.equal(before.panes[0].vars.WISP_TEST_ATTACHMENT, report.cookie)
      const originalRequest = await publish(first)
      const after = await observeTarget("after")
      assert.equal(JSON.parse(after.panes[0].vars.WISP_TEST_ATTACHMENT_ACK).sequence, 2)
      assert.equal(JSON.parse(after.panes[0].vars.WISP_TEST_RAW_ACK).sequence, 2)
      await support.waitForReadiness(first.child, async () => (await readSnapshot(first)).panes.some((pane) =>
        pane.domain === "remote-test" && pane.vars.WISP_TEST_RAW_ACK && JSON.parse(pane.vars.WISP_TEST_RAW_ACK).sequence === 2), { timeout: 3000 })
      await writeFile(first.detachFile, "detach")
      await support.waitForReadiness(first.child, async () => (await readSnapshot(first)).stage === "detached", { timeout: 3000 })
      const detachedProxy = await stopMuxProxy(first.proxyPath, first.proxyRequest, { allowDefunct: true })
      t.diagnostic(`domain detach: proxy group exited; wrapper reaped=${detachedProxy.reaped} (full reaping required after client exit)`)
      assert.equal(first.child.exitCode, null, "domain detach leaves the client mux running")
      await stopMux(first)
      process.kill(report.pid, 0)
      const second = await start("c2", randomBytes(12).toString("hex"), server.socket)
      const fresh = await absent(second)
      assert.equal(support.attachmentRecovered(fresh, originalRequest), false, "prior generation cannot authorize this connection")
      await publish(second)
      await stopMux(second, "SIGKILL")
      const third = await start("c3", randomBytes(12).toString("hex"), server.socket)
      await absent(third)
      await publish(third)
      const fourth = await start("c4", randomBytes(12).toString("hex"), server.socket)
      const [thirdAck, fourthAck] = await Promise.all([publish(third), publish(fourth)])
      assert.equal(fourthAck.sequence, thirdAck.sequence + 1)
      assert.equal(support.attachmentRecovered(await readSnapshot(third), fourthAck), false, "one client's generation cannot authorize another")
      await stopMux(third, "SIGKILL")
      requireRunningChannel(fourth.child, "surviving simultaneous client")
      await publish(fourth)
      await stopMuxProxy(fourth.proxyPath, fourth.proxyRequest, { allowDefunct: true, abrupt: true })
      const interrupted = await readMuxProxy(fourth.proxyPath, fourth.proxyRequest)
      assert.equal(interrupted.reason, "transport-interrupted")
      assert.equal(interrupted.child_signal, "SIGKILL")
      assert.equal(interrupted.group_gone, true)
      requireRunningChannel(fourth.child, "client after proxy transport interruption")
      await stopMux(fourth)
      const fifth = await start("c5", randomBytes(12).toString("hex"), server.socket)
      await absent(fifth)
      await publish(fifth)
      const unchanged = JSON.parse(await readFile(output, "utf8"))
      assert.equal(unchanged.pid, report.pid)
      assert.equal(unchanged.cookie, report.cookie)
      process.kill(report.pid, 0)
      t.diagnostic("ordered recovery, simultaneous clients, independent disconnect, and abrupt proxy interruption passed; original keeper preserved")
    }, [
      ["stop original probe", async () => {
        await writeFile(stop, "stop")
        if (report) await support.waitForReadiness(null, () => {
          try { process.kill(report.pid, 0); return false } catch (error) { if (error.code === "ESRCH") return true; throw error }
        }, { timeout: 3000 })
      }],
      ["stop owned muxes", async () => {
        const results = await Promise.allSettled(children.map((handle) => stopMux(handle)))
        const failures = results.filter((value) => value.status === "rejected").map((value) => value.reason)
        if (failures.length) throw new AggregateError(failures)
      }],
      ["remove owned files", async () => {
        assert.ok(children.every((handle) => !handle.proxyPath || handle.proxyStopped), `proxy cleanup unconfirmed; staging retained: ${root}`)
        await rm(root, { recursive: true, force: true })
      }],
    ])
    assert.equal(children[0].child.signalCode, "SIGTERM", "cleanup terminates the live server with the default signal")
    assert.equal(children.at(-1).child.signalCode, "SIGTERM", "cleanup never substitutes an array index for the signal")
  } catch (error) { throw new Error(`${error.message}\n${logs}`, { cause: error }) }
})
