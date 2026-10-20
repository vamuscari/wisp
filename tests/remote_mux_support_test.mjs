import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { once } from "node:events"
import { setTimeout as delay } from "node:timers/promises"
import test from "node:test"

import * as support from "./remote_mux_support.mjs"

test("connection diagnostics distinguish authentication, proxy request, and verified codec", () => {
  assert.equal(typeof support.muxConnectionStages, "function")
  assert.deepEqual(support.muxConnectionStages("connection pending"), [])
  assert.deepEqual(support.muxConnectionStages("DEBUG wezterm_client::client > going to run wezterm.exe cli proxy"),
    ["ssh-authenticated", "proxy-exec-requested"])
  assert.deepEqual(support.muxConnectionStages("TRACE wezterm_client::client > Server version is wezterm 1 (codec version 45)"),
    ["codec-verified"])
})

test("OpenSSH target authentication is not inferred from jump-host authentication", () => {
  assert.deepEqual(support.muxConnectionStages('Authenticated to jump.example ([192.0.2.1]:22) using "publickey".'), [])
  assert.deepEqual(support.muxConnectionStages("debug1: Sending command: WEZTERM_UNIX_SOCKET=C:/Temp/w-ssh-test/s wezterm.exe --config-file C:/Temp/w-ssh-test/server.lua cli --prefer-mux --no-auto-start proxy"),
    ["ssh-authenticated", "proxy-exec-requested"])
  assert.deepEqual(support.muxConnectionStages("debug1: Sending command: unrelated-command"), [])
})

test("pre-banner diagnostics distinguish config parsing and resolution from TCP connection", () => {
  assert.deepEqual(support.muxConnectionStages("INFO libssh::ssh_config_parse_file > Reading configuration data from /owned/config"),
    ["libssh-callback-observed", "ssh-config-parsing"])
  assert.deepEqual(support.muxConnectionStages("INFO wezterm_ssh::sessioninner > resolved 10.1.100.8:22 -> 10.1.100.8:22"),
    ["ssh-address-resolved"])
})

const effectiveConfig = "hostname 10.1.100.8\nuser test\\account\nport 22\n"

test("native SSH preflight accepts a direct route or an existing ProxyCommand without rewriting it", () => {
  assert.equal(typeof support.validateNativeSshConfig, "function")
  assert.doesNotThrow(() => support.validateNativeSshConfig(effectiveConfig))
  assert.doesNotThrow(() => support.validateNativeSshConfig(effectiveConfig + "proxyjump none\nhostkeyalias none\n"))
  assert.doesNotThrow(() => support.validateNativeSshConfig(effectiveConfig + "proxycommand /owned/proxy --fixed\n"))
})

test("native SSH preflight refuses to silently bypass a configured ProxyJump", () => {
  assert.equal(typeof support.validateNativeSshConfig, "function")
  for (const jump of ["proxy-public", "user@jump:2222", "jump1,jump2"]) {
    assert.throws(() => support.validateNativeSshConfig(effectiveConfig + `proxyjump ${jump}\n`), /ProxyJump.*direct/i)
  }
})

test("native SSH preflight refuses unsupported HostKeyAlias rather than changing host trust", () => {
  assert.equal(typeof support.validateNativeSshConfig, "function")
  assert.throws(() => support.validateNativeSshConfig(effectiveConfig + "hostkeyalias target-ssh\n"), /HostKeyAlias.*trust/i)
})

test("native SSH preflight rejects incomplete and ambiguous effective configuration", () => {
  assert.equal(typeof support.validateNativeSshConfig, "function")
  for (const source of ["", "user account\n", effectiveConfig + "hostname other\n", effectiveConfig + "proxyjump none\nproxyjump jump\n"]) {
    assert.throws(() => support.validateNativeSshConfig(source), /missing|duplicate/)
  }
})

test("failed native preflight never provisions a remote target or starts an attachment", async () => {
  assert.equal(typeof support.prepareMuxTarget, "function")
  const actions = []
  await assert.rejects(support.prepareMuxTarget(effectiveConfig + "proxyjump proxy-public\n", "native-ssh", async () => {
    actions.push("provision or attach")
  }), /ProxyJump/)
  assert.deepEqual(actions, [])
})

test("OpenSSH preflight preserves configured jump/trust settings and rejects unknown modes", async () => {
  assert.equal(typeof support.prepareMuxTarget, "function")
  const config = effectiveConfig + "proxyjump proxy-public\nhostkeyalias target-ssh\n"
  let calls = 0
  assert.equal(await support.prepareMuxTarget(config, "openssh", async () => { calls++; return "prepared" }), "prepared")
  assert.equal(calls, 1)
  await assert.rejects(support.prepareMuxTarget(config, "auto", async () => { calls++ }), /transport/)
  await assert.rejects(support.prepareMuxTarget("", "openssh", async () => { calls++ }), /missing/)
  assert.equal(calls, 1)
})

test("association-only acceptance cannot invoke the full spawn/disconnect exercise", async () => {
  assert.equal(typeof support.runMuxAcceptance, "function")
  const actions = []
  assert.equal(await support.runMuxAcceptance(undefined, {
    associate: async () => { actions.push("associate"); return "owned" },
    full: async () => { actions.push("spawn"); throw new Error("must not spawn") },
  }), "owned")
  assert.deepEqual(actions, ["associate"])
})

test("full acceptance requires explicit selection and successful owned association", async () => {
  assert.equal(typeof support.runMuxAcceptance, "function")
  const actions = []
  const steps = {
    associate: async () => { actions.push("associate"); return "owned" },
    full: async (owned) => { assert.equal(owned, "owned"); actions.push("spawn"); return "verified" },
  }
  await assert.rejects(support.runMuxAcceptance("auto", steps), /acceptance mode/)
  assert.deepEqual(actions, [])
  assert.equal(await support.runMuxAcceptance("full", steps), "verified")
  assert.deepEqual(actions, ["associate", "spawn"])
  await assert.rejects(support.runMuxAcceptance("full", { ...steps, associate: async () => { throw new Error("foreign endpoint") } }), /foreign endpoint/)
  assert.deepEqual(actions, ["associate", "spawn"])
})

test("observation snapshots require an exact version and the current publication/phase", () => {
  assert.equal(typeof support.validatePublicationObservation, "function")
  const publication = { probe_version: 1, action: "publish", marker: "a".repeat(24), generation: "b".repeat(24), cookie: "owned", sequence: 1 }
  const snapshot = { ...publication, action: "observe", phase: "after", stage: "observed", panes: [] }
  assert.equal(support.validatePublicationObservation(snapshot, publication, "after"), snapshot)
  const invalid = { probe_version: 2 }
  Object.defineProperty(invalid, "marker", { get() { throw new Error("interpreted incompatible observation") } })
  assert.throws(() => support.validatePublicationObservation(invalid, publication, "after"), /version/)
  assert.throws(() => support.validatePublicationObservation(snapshot, invalid, "after"), /version/)
  for (const update of [{ marker: "foreign" }, { generation: "stale" }, { cookie: "foreign" }, { sequence: 2 }, { phase: "before" }, { action: "publish" }]) {
    assert.throws(() => support.validatePublicationObservation({ ...snapshot, ...update }, publication, "after"), /observation/)
  }
})

test("the isolated diagnostic environment enables the libssh callback's own target", () => {
  assert.equal(typeof support.muxDiagnosticEnv, "function")
  const original = { PATH: "/owned/bin", WEZTERM_LOG: "error" }
  const env = support.muxDiagnosticEnv(original)
  assert.equal(env.PATH, original.PATH)
  assert.ok(env.WEZTERM_LOG.split(",").includes("libssh=info"))
  assert.ok(env.WEZTERM_LOG.split(",").includes("wezterm_client=trace"))
  assert.equal(original.WEZTERM_LOG, "error", "diagnostics cannot mutate the parent environment")
})

test("stream diagnostics see milestones before truncating an oversized output chunk", () => {
  assert.equal(typeof support.createMuxLogCapture, "function")
  const stages = []
  const capture = support.createMuxLogCapture((stage) => stages.push(stage))
  capture.write(Buffer.from("DEBUG wezterm_client::client > going to run wezterm.exe cli proxy\n" + "x".repeat(10_000)))
  assert.deepEqual(stages, ["ssh-authenticated", "proxy-exec-requested"])
  assert.equal(capture.tail, "x".repeat(4096))
})

test("native callback and fixture-banner observations survive oversized diagnostic output", () => {
  const stages = []
  const capture = support.createMuxLogCapture((stage) => stages.push(stage))
  capture.write(Buffer.from("INFO libssh::ssh_analyze_banner > SSH-2.0-WispDiagnosticFixture\n" + "x".repeat(10_000)))
  assert.ok(stages.includes("libssh-callback-observed"))
  assert.ok(stages.includes("fixture-banner-observed"))
  assert.equal(capture.tail, "x".repeat(4096))
  assert.equal(stages.includes("ssh-authenticated"), false)
})

test("an unrelated diagnostic mentioning the fixture does not establish a native banner", () => {
  assert.deepEqual(support.muxConnectionStages("INFO fixture > SSH-2.0-WispDiagnosticFixture\n"), [])
  assert.deepEqual(support.muxConnectionStages("INFO libssh::ssh_config_parse_line > unrelated setting\n"), ["libssh-callback-observed"])
})

test("stream diagnostics preserve split lines and UTF-8 and flush the last unterminated line", () => {
  assert.equal(typeof support.createMuxLogCapture, "function")
  const stages = []
  const capture = support.createMuxLogCapture((stage) => stages.push(stage))
  const line = Buffer.from("snow-\u96ea TRACE wezterm_client::client > Server version is wezterm 1 (codec version 45)")
  for (const byte of line) capture.write(Buffer.from([byte]))
  assert.deepEqual(stages, [], "an incomplete line is not a completed milestone")
  capture.end()
  assert.deepEqual(stages, ["codec-verified"])
  assert.equal(capture.tail, line.toString("utf8"))
  capture.end()
  assert.deepEqual(stages, ["codec-verified"], "completed milestones are not reported twice")
})

test("a delayed milestone after a long partial log line still reaches the capture", () => {
  assert.equal(typeof support.createMuxLogCapture, "function")
  const stages = []
  const capture = support.createMuxLogCapture((stage) => stages.push(stage))
  for (let index = 0; index < 20; index++) capture.write(Buffer.from("x".repeat(4096)))
  capture.write(Buffer.from("\nDEBUG wezterm_client::cli"))
  capture.write(Buffer.from("ent > going to run wezterm.exe cli proxy\n"))
  assert.deepEqual(stages, ["ssh-authenticated", "proxy-exec-requested"])
  assert.ok(Buffer.byteLength(capture.tail) <= 4096)
})

test("macOS sockets are short and measured in UTF-8 bytes", () => {
  assert.equal(typeof support.muxSocketPath, "function")
  assert.equal(support.muxSocketPath("/approved/w-123456", "darwin"), "/approved/w-123456/s")
  assert.throws(() => support.muxSocketPath("/" + "x".repeat(102), "darwin"), /socket.*103|103.*socket/)
  assert.throws(() => support.muxSocketPath("/" + "\u96ea".repeat(34), "darwin"), /socket.*103|103.*socket/)
})

test("socket preflight rejects the archived 108-byte Mac path", () => {
  assert.equal(typeof support.muxSocketPath, "function")
  const root = "/private/var/folders/f5/1bwpwzh12b32w_p2v_czb_bm0000gn/T/opencode/wisp-tool-update-gpwfkd01/cross-mac-3"
  assert.throws(() => support.muxSocketPath(root, "darwin"), /socket/)
})

test("attachment readiness requires this run and connection generation", () => {
  assert.equal(typeof support.attachmentReady, "function")
  const snapshot = { probe_version: 1, marker: "run-a", generation: "connection-1", stage: "ready", panes: [{ domain: "remote-test" }] }
  assert.equal(support.attachmentReady(snapshot, "run-a", "connection-1"), true)
  assert.equal(support.attachmentReady(snapshot, "run-b", "connection-1"), false)
  assert.equal(support.attachmentReady(snapshot, "run-a", "connection-2"), false)
  assert.equal(support.attachmentReady({ ...snapshot, panes: [] }, "run-a", "connection-1"), false)
  assert.equal(support.attachmentReady({ ...snapshot, panes: [{ domain: "local" }] }, "run-a", "connection-1"), false)
  assert.equal(support.attachmentReady({ ...snapshot, stage: "spawning" }, "run-a", "connection-1"), false)
})

test("a current connection's explicit attachment failure does not become a timeout", () => {
  const snapshot = { probe_version: 1, marker: "run-a", generation: "connection-1", stage: "failed", error: "spawn rejected", panes: [] }
  assert.throws(() => support.attachmentReady(snapshot, "run-a", "connection-1"), /spawn rejected/)
  assert.equal(support.attachmentReady(snapshot, "run-a", "connection-2"), false)
})

test("attachment snapshot version is checked before payload fields", () => {
  assert.equal(typeof support.attachmentReady, "function")
  assert.throws(() => support.attachmentReady({ probe_version: 2, panes: false }, "run-a", "connection-1"), /version/)
})

test("readiness does not wait for a long-lived bootstrap to exit", async () => {
  assert.equal(typeof support.waitForReadiness, "function")
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" })
  const closed = once(child, "close")
  try {
    assert.equal(await support.waitForReadiness(child, async () => "listener-ready", { timeout: 2000 }), "listener-ready")
    assert.equal(child.exitCode, null)
  } finally {
    child.kill()
    await closed
  }
})

test("bootstrap failure is distinct from a readiness timeout", async () => {
  assert.equal(typeof support.waitForReadiness, "function")
  const child = spawn(process.execPath, ["-e", "process.exit(7)"], { stdio: "ignore" })
  const closed = once(child, "close")
  try {
    await assert.rejects(support.waitForReadiness(child, async () => false, { timeout: 2000 }), /bootstrap.*7/)
  } finally {
    if (child.exitCode === null) child.kill()
    await closed
  }
})

test("a successful daemon launcher may exit before its listener becomes ready", async () => {
  assert.equal(typeof support.waitForReadiness, "function")
  const child = spawn(process.execPath, ["-e", "process.exit(0)"], { stdio: "ignore" })
  await once(child, "close")
  assert.equal(await support.waitForReadiness(child, async () => "ready", { timeout: 2000 }), "ready")
})

test("a valid not-ready snapshot replaces an earlier transient read error", async () => {
  let reads = 0
  await assert.rejects(support.waitForReadiness(null, async () => {
    if (reads++ === 0) throw Object.assign(new Error("stale missing snapshot"), { code: "ENOENT" })
    return false
  }, { timeout: 50, interval: 1, retryError: (error) => error.code === "ENOENT" }), (error) => {
    assert.match(error.message, /readiness timed out/)
    assert.ok(!error.message.includes("stale missing snapshot"))
    return true
  })
})

test("an inspection that outlives the deadline is cancelled rather than accepted", async () => {
  let inspectionSignal
  await assert.rejects(support.waitForReadiness(null, async (signal) => {
    inspectionSignal = signal
    await delay(200, undefined, { signal })
    return "too late"
  }, { timeout: 20 }), /readiness timed out/)
  assert.equal(inspectionSignal.aborted, true)
})

test("a pending inspection cannot hide bootstrap failure", async () => {
  const child = spawn(process.execPath, ["-e", "process.exit(7)"], { stdio: "ignore" })
  const closed = once(child, "close")
  let inspectionSignal
  try {
    await assert.rejects(support.waitForReadiness(child, async (signal) => {
      inspectionSignal = signal
      await closed
      return "not ready after a failed bootstrap"
    }, { timeout: 5000 }), /bootstrap.*7/)
    assert.equal(inspectionSignal.aborted, true)
  } finally {
    if (child.exitCode === null) child.kill()
    await closed
  }
})

test("fatal snapshot versions fail immediately instead of retrying", async () => {
  let reads = 0
  await assert.rejects(support.waitForReadiness(null, async () => {
    reads++
    return support.attachmentReady({ probe_version: 2 }, "run-a", "connection-1")
  }, { timeout: 50, interval: 1 }), /unsupported attachment snapshot version/)
  assert.equal(reads, 1)
})

test("external cancellation aborts an owned inspection and removes listeners", async () => {
  const cancellation = new AbortController()
  const reason = new Error("test run cancelled")
  const bootstrap = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" })
  const closed = once(bootstrap, "close")
  const listeners = [bootstrap.listenerCount("error"), bootstrap.listenerCount("exit")]
  let inspectionSignal
  try {
    await assert.rejects(support.waitForReadiness(bootstrap, async (signal) => {
      inspectionSignal = signal
      cancellation.abort(reason)
      await delay(100, undefined, { signal })
      return false
    }, { timeout: 2000, signal: cancellation.signal }), (error) => error === reason)
    assert.equal(inspectionSignal.aborted, true)
    assert.deepEqual([bootstrap.listenerCount("error"), bootstrap.listenerCount("exit")], listeners)
  } finally {
    bootstrap.kill()
    await closed
  }
})

test("even an inspection that ignores cancellation cannot block the deadline", async () => {
  await assert.rejects(support.waitForReadiness(null, async () => new Promise(() => {}), { timeout: 20 }), /readiness timed out/)
})

test("a cancelled wait never starts an inspection", async () => {
  const cancellation = new AbortController()
  const reason = new Error("already cancelled")
  cancellation.abort(reason)
  let reads = 0
  await assert.rejects(support.waitForReadiness(null, async () => { reads++; return true }, {
    signal: cancellation.signal,
  }), (error) => error === reason)
  assert.equal(reads, 0)
})

test("control commands exchange a bounded input frame without a shell", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  const frame = JSON.stringify({ probe_version: 1, arg: "space 'quote' \u96ea" }) + "\n"
  const result = await support.runProbeCommand(process.execPath,
    ["-e", "process.stdin.pipe(process.stdout)"], { input: frame })
  assert.equal(result.stdout, frame)
  assert.equal(result.stderr, "")
})

test("empty control input closes stdin without an empty write to a fast-exiting command", async () => {
  // Isolate a deterministic stdin/exit ordering in a real child process. Keep
  // non-empty EPIPE fatal: it can mean a real control frame was never delivered.
  const script = `import assert from 'node:assert/strict'
import childProcess from 'node:child_process'
import { EventEmitter } from 'node:events'
import { syncBuiltinESMExports } from 'node:module'
import { promisify } from 'node:util'
const fake = function() {}
fake[promisify.custom] = () => {
  const child = new EventEmitter()
  child.exitCode = null; child.signalCode = null; child.stdin = new EventEmitter()
  child.stdin.end = (...chunks) => { if (chunks.length) queueMicrotask(() => child.stdin.emit('error', Object.assign(new Error('write EPIPE'), {code:'EPIPE'}))) }
  const task = new Promise(resolve => queueMicrotask(() => {child.exitCode=0; child.emit('close',0); resolve({stdout:'ready',stderr:''})}))
  task.child=child; return task
}
childProcess.execFile=fake; syncBuiltinESMExports()
const {runProbeCommand}=await import(${JSON.stringify(new URL("./remote_mux_support.mjs", import.meta.url).href)})
assert.equal((await runProbeCommand('unused',[],{input:''})).stdout,'ready')
await assert.rejects(runProbeCommand('unused',[],{input:'frame'}),{code:'EPIPE'})
`
  const child = spawn(process.execPath, ["--input-type=module", "-e", script], { stdio: ["ignore", "ignore", "pipe"] })
  let stderr = ""
  child.stderr.on("data", (bytes) => { stderr += bytes })
  assert.equal((await once(child, "close"))[0], 0, stderr)
})

test("oversized control input fails before trying to spawn", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  await assert.rejects(support.runProbeCommand("not-a-probe-executable", [], { input: "x".repeat(8192) }), /control frame.*8 KiB/)
})

test("cancelled control commands have exited before rejecting", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  const cancellation = new AbortController()
  const task = support.runProbeCommand(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    input: "{}\n", signal: cancellation.signal,
  })
  const timer = setTimeout(() => cancellation.abort(new Error("owned check cancelled")), 100)
  try {
    await assert.rejects(task, (error) => {
      assert.equal(error.code, "ABORT_ERR")
      assert.ok(error.pid > 0)
      assert.throws(() => process.kill(error.pid, 0), { code: "ESRCH" })
      return true
    })
  } finally { clearTimeout(timer) }
})

test("a control command timeout terminates the child before rejecting", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  await assert.rejects(support.runProbeCommand(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    input: "{}\n", timeout: 100,
  }), (error) => {
    assert.equal(error.killed, true)
    assert.ok(error.pid > 0)
    assert.throws(() => process.kill(error.pid, 0), { code: "ESRCH" })
    return true
  })
})

test("control output cannot grow beyond its byte budget", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  await assert.rejects(support.runProbeCommand(process.execPath,
    ["-e", "process.stdout.write('x'.repeat(100000))"], { input: "{}\n", maxBuffer: 1024 }),
  { code: "ERR_CHILD_PROCESS_STDIO_MAXBUFFER" })
})

test("a control spawn failure settles instead of waiting for a nonexistent child", async () => {
  assert.equal(typeof support.runProbeCommand, "function")
  await assert.rejects(support.runProbeCommand("not-a-probe-executable", [], { input: "{}\n" }), { code: "ENOENT" })
})

test("an early control exit preserves its status even if the input pipe closes", async () => {
  await assert.rejects(support.runProbeCommand(process.execPath, ["-e", "process.exit(7)"], { input: "x".repeat(8191) }), { code: 7 })
})
