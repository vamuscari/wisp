// Owned POSIX argv-proxy lifecycle for the isolated mux feasibility fixtures.
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { randomUUID } from "node:crypto"
import { existsSync } from "node:fs"
import { link, readFile, rename, rm, writeFile } from "node:fs/promises"
import { isAbsolute } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
import { fileURLToPath, pathToFileURL } from "node:url"
import { runProbeCommand, waitForReadiness } from "./remote_mux_support.mjs"

function identity(value, expected) {
  assert.equal(value?.probe_version, 1, "unsupported mux proxy version")
  assert.match(value.marker, /^[a-f0-9]{24}$/, "invalid mux proxy marker")
  assert.match(value.generation, /^[a-f0-9]{24}$/, "invalid mux proxy generation")
  if (expected) {
    assert.equal(value.marker, expected.marker, "foreign mux proxy marker")
    assert.equal(value.generation, expected.generation, "stale mux proxy generation")
  }
}

function request(value) {
  identity(value)
  assert.ok(Array.isArray(value.args) && value.args.length > 0 && isAbsolute(value.args[0])
    && value.args.every((arg) => typeof arg === "string" && !arg.includes("\0")), "invalid mux proxy argv")
  assert.ok(Buffer.byteLength(JSON.stringify(value)) < 16_384, "oversized mux proxy request")
}

async function readOptional(path) {
  try { return JSON.parse(await readFile(path, "utf8")) } catch (error) { if (error.code === "ENOENT") return; throw error }
}

async function commitClaim(path, value) {
  const candidate = path + ".claim-" + randomUUID()
  await writeFile(candidate, JSON.stringify(value), { flag: "wx", mode: 0o600 })
  try {
    // link is atomic and does not replace an existing claim. Readers cannot
    // observe partially written JSON, and cancellation competes for this slot.
    await link(candidate, path + ".claim.json")
    return true
  } catch (error) {
    if (error.code === "EEXIST") return false
    throw error
  } finally { await rm(candidate) }
}

function validateClaim(value, expected) {
  identity(value, expected)
  assert.ok(["proxy", "cancelled"].includes(value.kind), "invalid mux proxy claim")
  if (value.kind === "proxy") {
    for (const pid of [value.proxy_pid, value.parent_pid]) assert.ok(Number.isSafeInteger(pid) && pid > 1, "invalid proxy PID")
  }
}

function alive(pid) {
  try { process.kill(pid, 0); return true } catch (error) {
    if (error.code === "ESRCH") return false
    if (error.code === "EPERM") return true
    throw error
  }
}

export async function prepareMuxProxy(path, value) {
  request(value)
  assert.ok(isAbsolute(path) && !path.includes("\0"), "expected an absolute mux proxy request path")
  await writeFile(path, JSON.stringify(value), { flag: "wx", mode: 0o600 })
  return { args: [process.execPath, fileURLToPath(import.meta.url), path] }
}

export async function readMuxProxy(path, expected) {
  identity(expected)
  const value = await readOptional(path + ".state.json")
  if (value) identity(value, expected)
  return value
}

export async function stopMuxProxy(path, expected, { allowDefunct = false, abrupt = false } = {}) {
  assert.equal(typeof abrupt, "boolean", "invalid mux proxy interruption mode")
  identity(expected)
  const stored = await readOptional(path)
  request(stored)
  identity(stored, expected)
  let claim = await readOptional(path + ".claim.json")
  if (!claim) {
    const cancelled = { probe_version: 1, marker: expected.marker, generation: expected.generation, kind: "cancelled" }
    if (await commitClaim(path, cancelled)) return { reaped: true }
    claim = await readOptional(path + ".claim.json")
  }
  validateClaim(claim, expected)
  if (claim.kind === "cancelled") return { reaped: true }
  await writeFile(path + (abrupt ? ".interrupt" : ".stop"), "stop", { mode: 0o600 })
  return waitForReadiness(null, async () => {
    const value = await readMuxProxy(path, expected)
    if (!value || value.stage !== "exited") return false
    assert.equal(value.group_gone, true, "owned mux proxy group remains; staging retained")
    if (!alive(claim.proxy_pid)) return { reaped: true }
    if (allowDefunct) {
      // The pinned WezTerm argv-proxy path drops Child without wait(). An
      // exited zombie is not a running proxy; final client cleanup still
      // requires full reaping. Inspect only this exact claimed child PID.
      assert.ok(Number.isSafeInteger(claim.proxy_pid) && claim.proxy_pid > 1, "invalid proxy PID")
      let result
      try {
        result = await runProbeCommand("/bin/ps", ["-o", "ppid=,stat=", "-p", String(claim.proxy_pid)], { input: "", timeout: 1000 })
      } catch (error) { if (error.code === 1 && !alive(claim.proxy_pid)) return { reaped: true }; throw error }
      const status = /^(\d+)\s+(\S+)/.exec(result.stdout.trim())
      if (status && status[2].startsWith("Z") && Number(status[1]) === claim.parent_pid) return { reaped: false }
    }
    return false
  }, { timeout: 6000 })
}

async function serve(path) {
  assert.notEqual(process.platform, "win32", "managed mux proxy requires POSIX process groups")
  const value = await readOptional(path)
  request(value)
  const parent = process.ppid
  assert.ok(parent > 1, "mux proxy parent already exited")
  const base = { probe_version: 1, marker: value.marker, generation: value.generation,
    kind: "proxy", parent_pid: parent, proxy_pid: process.pid, launches: 1 }
  // WezTerm may invoke an early-failing argv proxy repeatedly. One generation
  // may launch its SSH/process tree only once, even across those invocations.
  if (!await commitClaim(path, base)) {
    const claim = await readOptional(path + ".claim.json")
    validateClaim(claim, value)
    assert.equal(claim.kind, "cancelled", "mux proxy generation already claimed")
    return
  }
  let writes = Promise.resolve()
  function publish(state) {
    writes = writes.then(async () => {
      await writeFile(path + ".state.json.tmp", JSON.stringify({ ...base, ...state }), { mode: 0o600 })
      await rename(path + ".state.json.tmp", path + ".state.json")
    })
    return writes
  }
  await publish({ stage: "starting" })
  if (existsSync(path + ".stop") || existsSync(path + ".interrupt") || process.ppid !== parent) {
    await publish({ stage: "exited", group_gone: true, outcome: "cancelled", reason: "cancelled-before-launch", launches: 0 })
    return
  }
  const child = spawn(value.args[0], value.args.slice(1), { detached: true, stdio: ["pipe", "pipe", "pipe"] })
  const closed = new Promise((resolve) => child.once("close", resolve))
  let failure, childCode, childSignal, stopping, monitor
  let finish
  const finished = new Promise((resolve) => { finish = resolve })
  const groupAlive = () => child.pid && alive(-child.pid)
  const signalGroup = (signal) => {
    if (!child.pid) return
    // A dying macOS group can transiently return EPERM until its child is
    // reaped. That is not proof of exit: cleanup still waits for ESRCH.
    try { process.kill(-child.pid, signal) } catch (error) { if (!["ESRCH", "EPERM"].includes(error.code)) throw error }
  }
  async function stop(reason) {
    if (stopping) return stopping
    stopping = (async () => {
      clearInterval(monitor)
      // The supervisor signals its own live child group; callers never kill a
      // PID read from an unverified state file or bypass descendant cleanup.
      if (reason === "transport-interrupted") {
        try { signalGroup("SIGKILL") } catch (error) { failure ??= error }
      }
      process.stdin.unpipe(child.stdin)
      child.stdin.end()
      // Let an orderly SSH EOF/exit finish before escalating to group signals.
      await delay(50)
      try { if (groupAlive()) signalGroup("SIGTERM") } catch (error) { failure ??= error }
      const kill = setTimeout(() => {
        try { signalGroup("SIGKILL") } catch (error) { failure ??= error }
      }, 400)
      let groupGone = false
      try {
        await waitForReadiness(null, () => !groupAlive(), { timeout: 3000, interval: 25 })
        groupGone = true
        await new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(new Error("proxy stdio did not close")), 1000)
          closed.then(() => { clearTimeout(timer); resolve() })
        })
      } catch (error) { failure ??= error } finally { clearTimeout(kill) }
      const cancelled = ["stop-requested", "transport-interrupted", "parent-exited", "client-eof", "client-closed", "signal"].includes(reason)
      const outcome = failure || !cancelled && childCode !== 0 ? "failed" : cancelled ? "cancelled" : "closed"
      await publish({ stage: "exited", child_pid: child.pid, group_pid: child.pid, child_code: childCode,
        child_signal: childSignal, group_gone: groupGone, reason, outcome, error: failure?.message.slice(-2048) })
      process.stdin.destroy()
      process.exitCode = outcome === "failed" ? 1 : 0
      finish()
    })()
    return stopping
  }
  const cancel = (reason) => { stop(reason).catch((error) => { process.stderr.write(error.message + "\n"); process.exitCode = 1; finish() }) }
  child.on("error", (error) => { failure = error; cancel("spawn-failed") })
  child.on("exit", (code, signal) => { childCode = code; childSignal = signal; cancel("proxy-exited") })
  child.stdout.on("end", () => cancel("proxy-output-ended"))
  child.stdin.on("error", () => cancel("client-closed"))
  process.stdin.on("end", () => cancel("client-eof"))
  process.stdin.on("error", () => cancel("client-closed"))
  process.stdout.on("error", () => cancel("client-closed"))
  process.stderr.on("error", () => {})
  process.on("SIGTERM", () => cancel("signal"))
  process.on("SIGINT", () => cancel("signal"))
  process.stdin.pipe(child.stdin)
  child.stdout.pipe(process.stdout, { end: false })
  child.stderr.pipe(process.stderr, { end: false })
  monitor = setInterval(() => {
    if (process.ppid !== parent) cancel("parent-exited")
    else if (existsSync(path + ".interrupt")) cancel("transport-interrupted")
    else if (existsSync(path + ".stop")) cancel("stop-requested")
  }, 50)
  if (child.pid) await publish({ stage: "running", child_pid: child.pid, group_pid: child.pid })
  await finished
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  serve(process.argv[2]).catch((error) => { process.stderr.write(error.message + "\n"); process.exitCode = 1 })
}
