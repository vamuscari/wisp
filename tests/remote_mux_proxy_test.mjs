import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { once } from "node:events"
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { waitForReadiness } from "./remote_mux_support.mjs"
import * as proxy from "./remote_mux_proxy.mjs"

const marker = "a".repeat(24), generation = "b".repeat(24)
const posixOnly = process.platform === "win32" ? "POSIX process-group ownership is required" : false

async function fixture(t, args) {
  assert.equal(typeof proxy.prepareMuxProxy, "function")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-proxy-"))
  const path = join(root, "request.json")
  const request = { probe_version: 1, marker, generation, args: typeof args === "function" ? args(root) : args }
  const prepared = await proxy.prepareMuxProxy(path, request)
  const child = spawn(prepared.args[0], prepared.args.slice(1), { stdio: ["pipe", "pipe", "pipe"] })
  const closed = new Promise((resolve) => child.once("close", (code) => resolve(code)))
  let stdout = "", stderr = ""
  child.stdout.on("data", (bytes) => { stdout = (stdout + bytes).slice(-4096) })
  child.stderr.on("data", (bytes) => { stderr = (stderr + bytes).slice(-4096) })
  child.stdin.on("error", () => {})
  t.after(async () => {
    await proxy.stopMuxProxy(path, request)
    if (child.exitCode === null) child.kill()
    await closed
    await rm(root, { recursive: true, force: true })
  })
  const state = () => proxy.readMuxProxy(path, request)
  const wait = (predicate) => waitForReadiness(null, async () => {
    const value = await state()
    return value && predicate(value) && value
  }, { timeout: 5000 })
  return { path, root, request, prepared, child, closed, state, wait, get stdout() { return stdout }, get stderr() { return stderr } }
}

test("managed proxy validates the exact request version before interpreting argv", async () => {
  assert.equal(typeof proxy.prepareMuxProxy, "function")
  const request = { probe_version: 2 }
  Object.defineProperty(request, "args", { get() { throw new Error("interpreted incompatible argv") } })
  await assert.rejects(proxy.prepareMuxProxy("/unused/request.json", request), /version/)
})

test("managed proxy preserves literal bytes and owns a distinct process group", { skip: posixOnly }, async (t) => {
  const value = "space 'quote' \u96ea\n\u0001"
  const f = await fixture(t, [process.execPath, "-e", "process.stdin.pipe(process.stdout)"])
  const state = await f.wait((value) => value.stage === "running")
  assert.equal(state.parent_pid, process.pid)
  assert.equal(state.group_pid, state.child_pid)
  assert.notEqual(state.child_pid, state.proxy_pid)
  f.child.stdin.write(value)
  await waitForReadiness(null, () => f.stdout === value, { timeout: 2000 })
  f.child.stdin.end()
  await f.wait((value) => value.stage === "exited")
  assert.equal(await f.closed, 0)
})

test("an immediate proxy failure is reported and a repeated invocation cannot relaunch it", { skip: posixOnly }, async (t) => {
  const f = await fixture(t, (root) => [process.execPath, "-e", `require('node:fs').appendFileSync(${JSON.stringify(join(root, "calls"))},'x');process.exit(7)`])
  const state = await f.wait((value) => value.stage === "exited")
  assert.equal(state.child_code, 7)
  assert.equal(state.outcome, "failed")
  const second = spawn(f.prepared.args[0], f.prepared.args.slice(1), { stdio: "ignore" })
  assert.notEqual((await once(second, "close"))[0], 0)
  assert.equal((await f.state()).child_pid, state.child_pid)
  assert.equal((await f.state()).launches, 1)
  assert.equal(await readFile(join(f.root, "calls"), "utf8"), "x", "early-failure retry cannot launch another child")
})

test("a hanging proxy is cancelled and its owned group has exited before cleanup returns", { skip: posixOnly }, async (t) => {
  const f = await fixture(t, [process.execPath, "-e", "setInterval(()=>{},1000)"])
  const state = await f.wait((value) => value.stage === "running")
  await assert.rejects(waitForReadiness(f.child, async () => false, { timeout: 30 }), /timed out/)
  await proxy.stopMuxProxy(f.path, f.request)
  await f.closed
  assert.throws(() => process.kill(-state.group_pid, 0), { code: "ESRCH" })
  assert.equal((await f.state()).outcome, "cancelled")
})

test("closing proxy stdout is detected even when the child keeps running", { skip: posixOnly }, async (t) => {
  const f = await fixture(t, [process.execPath, "-e", "process.stdout.end();setInterval(()=>{},1000)"])
  const state = await f.wait((value) => value.stage === "exited")
  await f.closed
  assert.equal(state.reason, "proxy-output-ended")
  assert.throws(() => process.kill(-state.group_pid, 0), { code: "ESRCH" })
})

test("proxy cancellation terminates a descendant that ignores SIGTERM", { skip: posixOnly }, async (t) => {
  const script = "const {spawn}=require('node:child_process');const c=spawn(process.execPath,['-e',\"process.on('SIGTERM',()=>{});setInterval(()=>{},1000)\"],{stdio:'inherit'});process.on('SIGTERM',()=>{});process.stdout.write(String(c.pid));setInterval(()=>{},1000)"
  const f = await fixture(t, [process.execPath, "-e", script])
  const state = await f.wait((value) => value.stage === "running")
  await waitForReadiness(null, () => /^\d+$/.test(f.stdout), { timeout: 2000 })
  await proxy.stopMuxProxy(f.path, f.request)
  await f.closed
  assert.throws(() => process.kill(-state.group_pid, 0), { code: "ESRCH" })
  assert.throws(() => process.kill(Number(f.stdout), 0), { code: "ESRCH" })
})

test("a missing proxy executable settles as a failure instead of hanging", { skip: posixOnly }, async (t) => {
  const f = await fixture(t, ["/not/an/owned/proxy"])
  const state = await f.wait((value) => value.stage === "exited")
  await f.closed
  assert.equal(state.outcome, "failed")
  assert.match(state.error, /ENOENT/)
})

test("a stopped request cannot start a proxy even before its first invocation", { skip: posixOnly }, async (t) => {
  assert.equal(typeof proxy.prepareMuxProxy, "function")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-proxy-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const path = join(root, "request.json"), called = join(root, "called")
  const request = { probe_version: 1, marker, generation, args: [process.execPath, "-e", `require('node:fs').writeFileSync(${JSON.stringify(called)},'called')`] }
  const prepared = await proxy.prepareMuxProxy(path, request)
  await proxy.stopMuxProxy(path, request)
  const child = spawn(prepared.args[0], prepared.args.slice(1), { stdio: "ignore" })
  await once(child, "close")
  await assert.rejects(readFile(called), { code: "ENOENT" })
})

test("terminating the proxy's parent leaves neither the proxy nor its process group alive", { skip: posixOnly }, async (t) => {
  assert.equal(typeof proxy.prepareMuxProxy, "function")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-proxy-"))
  const path = join(root, "request.json")
  const request = { probe_version: 1, marker, generation, args: [process.execPath, "-e", "setInterval(()=>{},1000)"] }
  const prepared = await proxy.prepareMuxProxy(path, request)
  const parent = spawn(process.execPath, ["-e", `require('node:child_process').spawn(${JSON.stringify(prepared.args[0])},${JSON.stringify(prepared.args.slice(1))},{stdio:['pipe','ignore','ignore']});setInterval(()=>{},1000)`], { stdio: "ignore" })
  const closed = once(parent, "close")
  t.after(async () => {
    if (parent.exitCode === null && !parent.signalCode) parent.kill("SIGKILL")
    await closed
    await proxy.stopMuxProxy(path, request)
    await rm(root, { recursive: true, force: true })
  })
  const state = await waitForReadiness(null, async () => {
    const value = await proxy.readMuxProxy(path, request)
    return value?.stage === "running" && value
  }, { timeout: 3000 })
  assert.equal(state.parent_pid, parent.pid)
  parent.kill("SIGKILL")
  await closed
  await waitForReadiness(null, async () => (await proxy.readMuxProxy(path, request))?.stage === "exited", { timeout: 5000 })
  await proxy.stopMuxProxy(path, request)
  assert.throws(() => process.kill(state.proxy_pid, 0), { code: "ESRCH" })
  assert.throws(() => process.kill(-state.group_pid, 0), { code: "ESRCH" })
})

test("proxy metadata rejects stale identities and versions before cleanup", async (t) => {
  assert.equal(typeof proxy.readMuxProxy, "function")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-proxy-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const path = join(root, "request.json")
  const expected = { probe_version: 1, marker, generation }
  await writeFile(path + ".state.json", '{"probe_version":2}')
  await assert.rejects(proxy.readMuxProxy(path, expected), /version/)
  await writeFile(path + ".state.json", JSON.stringify({ ...expected, generation: "c".repeat(24) }))
  await assert.rejects(proxy.readMuxProxy(path, expected), /generation/)
})

test("a stale cleanup cannot write a stop marker or affect the current proxy", { skip: posixOnly }, async (t) => {
  const f = await fixture(t, [process.execPath, "-e", "setInterval(()=>{},1000)"])
  await f.wait((value) => value.stage === "running")
  for (const update of [{ marker: "c".repeat(24) }, { generation: "c".repeat(24) }, { probe_version: 2 }]) {
    for (const abrupt of [false, true]) {
      await assert.rejects(proxy.stopMuxProxy(f.path, { ...f.request, ...update }, { abrupt }), /marker|generation|version/)
      for (const suffix of [".stop", ".interrupt"]) await assert.rejects(readFile(f.path + suffix), { code: "ENOENT" })
    }
  }
  assert.equal((await f.state()).stage, "running")
})

test("cancellation fences a wrapper paused before its atomic claim", { skip: posixOnly, timeout: 10_000 }, async (t) => {
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-proxy-"))
  const path = join(root, "request.json"), called = join(root, "called"), gate = join(root, "gate.mjs")
  const request = { probe_version: 1, marker, generation, args: [process.execPath, "-e", `require('node:fs').writeFileSync(${JSON.stringify(called)},'called')`] }
  const prepared = await proxy.prepareMuxProxy(path, request)
  // Pause the real wrapper at the shared filesystem boundary, not its child.
  // The hook covers both the old direct write and an atomic no-replace commit.
  await writeFile(gate, `import fs from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import { setTimeout as delay } from 'node:timers/promises'
const write = fs.writeFile, link = fs.link, claim = ${JSON.stringify(path + ".claim.json")}
async function pause() {
  await write(${JSON.stringify(path + ".paused")}, 'paused')
  while (!existsSync(${JSON.stringify(path + ".release")})) await delay(5)
}
fs.writeFile = async (path, ...args) => { if (path === claim) await pause(); return write(path, ...args) }
fs.link = async (source, target) => { if (target === claim) await pause(); return link(source, target) }
syncBuiltinESMExports()
`)
  const child = spawn(prepared.args[0], ["--import", gate, ...prepared.args.slice(1)], { stdio: "ignore" })
  const closed = once(child, "close")
  t.after(async () => {
    await writeFile(path + ".release", "release")
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
    await proxy.stopMuxProxy(path, request)
    await rm(root, { recursive: true, force: true })
  })
  await waitForReadiness(child, async () => {
    try { return await readFile(path + ".paused") } catch (error) { if (error.code === "ENOENT") return false; throw error }
  }, { timeout: 3000 })
  await proxy.stopMuxProxy(path, request)
  await writeFile(path + ".release", "release")
  await closed
  await assert.rejects(readFile(called), { code: "ENOENT" })
})
