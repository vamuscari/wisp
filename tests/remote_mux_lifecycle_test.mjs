import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { once } from "node:events"
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { runProbeCommand } from "./remote_mux_support.mjs"

import * as lifecycle from "./remote_mux_lifecycle.mjs"
const info = {
  probe_version: 1, root: "C:/Temp/w-ssh-123456789012345678901234",
  node: "C:/Program Files/nodejs/node.exe", version: "wezterm test",
  baseline: [{ pid: 42, created: "2026-10-19T00:00:00.0000000Z" }],
}

test("a lost claim response still leaves cleanup registered for the owned root", async () => {
  assert.equal(typeof lifecycle.createRemoteProbe, "function")
  const events = []
  const lost = new Error("claim reply lost after creating owner.json")
  let claimed = false
  const probe = lifecycle.createRemoteProbe(async (action, request) => {
    events.push(action)
    if (action === "prepare") return info
    assert.equal(request.root, info.root)
    if (action === "claim") { claimed = true; throw lost }
    if (action === "stop") return { probe_version: 1, baseline: info.baseline }
    if (action === "remove") { claimed = false; return { probe_version: 1, removed: true } }
    throw new Error(`unexpected action: ${action}`)
  })
  await assert.rejects(probe.prepare(), (error) => error === lost)
  assert.equal(probe.info.root, info.root)
  await probe.cleanup()
  assert.equal(claimed, false)
  assert.deepEqual(events, ["prepare", "claim", "stop", "remove"])
})

test("failed read-only preparation does not attempt to claim or remove a root", async () => {
  assert.equal(typeof lifecycle.createRemoteProbe, "function")
  const events = []
  const probe = lifecycle.createRemoteProbe(async (action) => {
    events.push(action)
    throw new Error("missing remote executable")
  })
  await assert.rejects(probe.prepare(), /missing remote executable/)
  await probe.cleanup()
  assert.deepEqual(events, ["prepare"])
})

test("remote metadata versions are checked before claiming or interpreting fields", async () => {
  assert.equal(typeof lifecycle.createRemoteProbe, "function")
  const actions = []
  const response = { probe_version: 2 }
  Object.defineProperty(response, "root", { get() { throw new Error("interpreted a mismatched payload") } })
  const probe = lifecycle.createRemoteProbe(async (action) => { actions.push(action); return response })
  await assert.rejects(probe.prepare(), /version/)
  await probe.cleanup()
  assert.deepEqual(actions, ["prepare"])
})

test("PID reuse in the original mux prevents evidence/staging removal", async () => {
  assert.equal(typeof lifecycle.createRemoteProbe, "function")
  const actions = []
  const probe = lifecycle.createRemoteProbe(async (action) => {
    actions.push(action)
    if (action === "prepare") return info
    if (action === "claim") return { probe_version: 1 }
    if (action === "stop") return { probe_version: 1, baseline: [{ pid: 42, created: "changed" }] }
    throw new Error("staging must not be removed")
  })
  await probe.prepare()
  await assert.rejects(probe.cleanup(), /existing.*mux/)
  assert.deepEqual(actions, ["prepare", "claim", "stop"])
})

test("failed remote process termination never proceeds to staging removal", async () => {
  assert.equal(typeof lifecycle.createRemoteProbe, "function")
  const actions = []
  const probe = lifecycle.createRemoteProbe(async (action) => {
    actions.push(action)
    if (action === "prepare") return info
    if (action === "claim") return { probe_version: 1 }
    throw new Error("owned mux still running; staging retained")
  })
  await probe.prepare()
  await assert.rejects(probe.cleanup(), (error) => {
    assert.match(error.message, /still running/)
    assert.ok(error.message.includes(info.root), "failed cleanup must identify the retained remote root")
    return true
  })
  assert.deepEqual(actions, ["prepare", "claim", "stop"])
})

test("cleanup failures preserve the original error and do not skip other cleanup", async () => {
  assert.equal(typeof lifecycle.withProbeCleanup, "function")
  const original = new Error("attachment failed")
  const cleanup = new Error("remote cleanup failed")
  const events = []
  await assert.rejects(lifecycle.withProbeCleanup(async () => { throw original }, [
    ["diagnostics", async (failure) => { assert.equal(failure, original); events.push("diagnostics") }],
    ["remote", async () => { events.push("remote"); throw cleanup }],
    ["local", async () => { events.push("local") }],
  ]), (error) => {
    assert.ok(error instanceof AggregateError)
    assert.equal(error.cause, original)
    assert.equal(error.errors[0], original)
    assert.equal(error.errors[1].cause, cleanup)
    assert.match(error.message, /attachment failed/)
    assert.match(error.message, /remote cleanup failed/)
    return true
  })
  assert.deepEqual(events, ["diagnostics", "remote", "local"])
})

test("an exited child's inherited stdio cannot block the remaining cleanup", {
  skip: process.platform === "win32" ? "the cross-machine driver owns POSIX channels" : false,
  timeout: 5000,
}, async () => {
  const child = spawn(process.execPath, ["-e", `
    const { spawn } = require('node:child_process')
    const descendant = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 1200)'], { stdio: ['ignore', 1, 2, 3] })
    descendant.once('spawn', () => process.exit(0))
  `], { stdio: ["ignore", "pipe", "pipe", "pipe"] })
  let stdioClosed = false
  const closed = new Promise((resolve) => child.once("close", () => { stdioClosed = true; resolve() }))
  child.stdout.resume(); child.stderr.resume()
  // This extra observation pipe lets the test join the self-terminating
  // descendant even if bounded cleanup closes its standard pipe endpoints.
  child.stdio[3].resume()
  const original = new Error("attachment failed before cleanup")
  const events = []
  try {
    await once(child, "exit")
    assert.equal(child.exitCode, 0)
    assert.equal(child.stdout.readableEnded, false, "the descendant still holds the inherited pipe")
    await assert.rejects(lifecycle.withProbeCleanup(async () => { throw original }, [
      ["stop channel", () => lifecycle.stopProbeChild(child, closed, { timeout: 50, killAfter: 25 })],
      ["remote cleanup", () => { events.push("remote cleanup") }],
      ["retain evidence", () => { events.push("retain evidence") }],
    ]), (error) => {
      assert.ok(error instanceof AggregateError)
      assert.equal(error.cause, original)
      assert.equal(error.errors[0], original)
      assert.match(error.errors[1].message, /channel.*stdio.*timed out/)
      return true
    })
    assert.deepEqual(events, ["remote cleanup", "retain evidence"])
    assert.equal(stdioClosed, false, "later cleanup ran before the inherited pipes closed naturally")
    assert.equal(child.stdout.destroyed, true)
    assert.equal(child.stderr.destroyed, true)
  } finally {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
  }
})

test("bounded channel cleanup reaps a live child and accepts an already closed channel", async () => {
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" })
  const closed = new Promise((resolve) => child.once("close", resolve))
  try {
    await once(child, "spawn")
    await lifecycle.stopProbeChild(child, closed, { killAfter: 100, timeout: 1000 })
    assert.ok(child.exitCode !== null || child.signalCode, "the owned child exited before cleanup returned")
    await lifecycle.stopProbeChild(child, closed, { killAfter: 100, timeout: 1000 })
    await lifecycle.stopProbeChild(undefined, undefined)
  } finally {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
  }
})

test("bounded channel cleanup escalates an owned child that ignores SIGTERM", {
  skip: process.platform === "win32" ? "requires POSIX SIGTERM handling" : false,
}, async () => {
  const child = spawn(process.execPath, ["-e", "process.on('SIGTERM', () => {});process.stdout.write('ready');setInterval(() => {}, 1000)"],
    { stdio: ["ignore", "pipe", "ignore"] })
  const closed = new Promise((resolve) => child.once("close", resolve))
  try {
    await once(child.stdout, "data")
    await lifecycle.stopProbeChild(child, closed, { killAfter: 50, timeout: 1000 })
    assert.equal(child.signalCode, "SIGKILL")
  } finally {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
  }
})

test("a lone original error is rethrown without being replaced", async () => {
  assert.equal(typeof lifecycle.withProbeCleanup, "function")
  const original = new Error("version mismatch")
  await assert.rejects(lifecycle.withProbeCleanup(async () => { throw original }, []), (error) => error === original)
  assert.equal(await lifecycle.withProbeCleanup(async () => "ok", []), "ok")
})

test("diagnostics see cleanup failures even after a successful probe", async () => {
  const failure = new Error("owned process did not exit")
  let observed
  await assert.rejects(lifecycle.withProbeCleanup(async () => "ok", [
    ["stop", async () => { throw failure }],
    ["retain diagnostics", async (error) => { observed = error }],
  ]), /owned process did not exit/)
  assert.ok(observed, "cleanup failures must reach the diagnostic step")
  assert.equal(observed.cause, failure)
})

test("local diagnostic reads retain only the last 4 KiB", async (t) => {
  assert.equal(typeof lifecycle.readProbeTail, "function")
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const path = join(parent, "mux.log")
  await writeFile(path, "old data".repeat(1024) + "x".repeat(4096))
  const tail = await lifecycle.readProbeTail(path)
  assert.equal(Buffer.from(tail.data_base64, "base64").toString(), "x".repeat(4096))
  assert.equal(tail.name, "mux.log")
  assert.equal(tail.truncated, true)
})

test("bounded evidence survives deletion of the owned staging tree", async (t) => {
  assert.equal(typeof lifecycle.withProbeCleanup, "function")
  assert.equal(typeof lifecycle.retainProbeEvidence, "function")
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const staging = join(parent, "staging")
  await mkdir(staging)
  const log = join(staging, "mux.log")
  const evidence = join(parent, "evidence.json")
  await writeFile(log, "owned mux diagnostics")
  const original = new Error("probe failed")
  await assert.rejects(lifecycle.withProbeCleanup(async () => { throw original }, [
    ["diagnostics", async () => lifecycle.retainProbeEvidence(evidence, {
      probe_version: 1, diagnostic: await readFile(log, "utf8"),
    })],
    ["staging", async () => rm(staging, { recursive: true })],
  ]), (error) => error === original)
  assert.equal(JSON.parse(await readFile(evidence, "utf8")).diagnostic, "owned mux diagnostics")
  await assert.rejects(readFile(log), { code: "ENOENT" })
  await assert.rejects(lifecycle.retainProbeEvidence(evidence, { probe_version: 1 }), { code: "EEXIST" })
  await assert.rejects(lifecycle.retainProbeEvidence(join(parent, "oversized.json"), {
    probe_version: 1, diagnostic: "x".repeat(512 * 1024),
  }), /evidence.*512 KiB/)
})

test("every fixed Windows lifecycle command fits the SSH bootstrap budget", () => {
  assert.equal(typeof lifecycle.powershellCommand, "function")
  assert.equal(typeof lifecycle.remoteProbeScripts, "object")
  for (const source of Object.values(lifecycle.remoteProbeScripts)) {
    assert.ok(lifecycle.powershellCommand(lifecycle.probeInput + source).length < 8000)
  }
})

test("PowerShell controls preserve UTF-8 input despite an inherited OEM code page", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for the UTF-8 control round trip",
}, async () => {
  const value = { cwd: "C:/Temp/work area \u96ea/emoji-\ud83d\ude80", args: ["", "snow-\u96ea", "emoji-\ud83d\ude80", "quote\"'", "line\n\t\u0001"] }
  const source = lifecycle.probeInput + String.raw`
[pscustomobject]@{probe_version=1;input_code_page=[Console]::InputEncoding.CodePage;output_code_page=[Console]::OutputEncoding.CodePage;value=$r.value}|ConvertTo-Json -Depth 6 -Compress
`
  const command = lifecycle.powershellCommand(source).split(" ").at(-1)
  // Reproduce Windows PowerShell's OEM stdin default before the real prefix.
  const encoded = Buffer.from("[Console]::InputEncoding=[Text.Encoding]::GetEncoding(437)\n"
    + Buffer.from(command, "base64").toString("utf16le"), "utf16le").toString("base64")
  const { stdout } = await runProbeCommand(process.env.WISP_TEST_POWERSHELL,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], {
      input: JSON.stringify({ probe_version: 1, value }) + "\n",
    })
  const response = JSON.parse(stdout)
  assert.equal(response.probe_version, 1)
  assert.deepEqual(response.value, value)
  assert.equal(response.input_code_page, 65001)
  assert.equal(response.output_code_page, 65001)
})

test("Windows lifecycle snippets parse with PowerShell without executing them", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL to parse fixed Windows commands locally",
}, async () => {
  const parser = `$ErrorActionPreference='Stop'
$r=ConvertFrom-Json -InputObject ([Console]::In.ReadLine())
if($r.probe_version -ne 1){throw 'Unsupported parser request'}
$tokens=$null;$errors=$null
$null=[System.Management.Automation.Language.Parser]::ParseInput($r.source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors.Message -join '\n')}
`
  for (const [name, source] of Object.entries(lifecycle.remoteProbeScripts)) {
    await runProbeCommand(process.env.WISP_TEST_POWERSHELL, ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand",
      Buffer.from(parser, "utf16le").toString("base64")], {
      input: JSON.stringify({ probe_version: 1, source: lifecycle.probeInput + source }) + "\n",
    }).catch((error) => { throw new Error(`${name}: ${error.message}`, { cause: error }) })
  }
})

async function localControl(parent, action, root, marker, request = {}, fakes = "", tagQuery) {
  // Execute filesystem guards locally, but never inspect or terminate host
  // processes: inventory is empty unless a test explicitly injects fake entries.
  let script = lifecycle.remoteProbeScripts[action]
  if (tagQuery !== undefined) {
    assert.ok(script.includes(lifecycle.nativeReparseTagQuery), "the native query must be the only replaced dependency")
    script = script.replace(lifecycle.nativeReparseTagQuery, tagQuery)
  }
  // Enforce the SSH budget on the real command, not the local dependency fakes.
  const command = lifecycle.powershellCommand(lifecycle.probeInput + script).split(" ").at(-1)
  const source = `function Get-CimInstance {param($ClassName,$Filter)}\n` + fakes
    + Buffer.from(command, "base64").toString("utf16le")
  const encoded = Buffer.from(source, "utf16le").toString("base64")
  const result = await runProbeCommand(process.env.WISP_TEST_POWERSHELL,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], {
      env: { ...process.env, TMPDIR: parent, TEMP: parent, TMP: parent },
      input: JSON.stringify({ probe_version: 1, marker, root, ...request }) + "\n",
    })
  return JSON.parse(result.stdout)
}

test("real PowerShell filesystem claims, bounded evidence, and removal stay inside the test root", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "a".repeat(24)
  const root = join(parent, "w-ssh-" + marker)
  assert.equal((await localControl(parent, "claim", root, marker)).claimed, true)
  const staged = await localControl(parent, "stage", root, marker, {
    files: { "probe.mjs": Buffer.from("owned fixture").toString("base64") },
  })
  await localControl(parent, "stage", root, marker, { files: { "server.lua": Buffer.from("return {}").toString("base64") } })
  assert.equal(staged.cwd, join(root, "work area \u96ea"))
  assert.equal((await stat(staged.cwd)).isDirectory(), true)
  await writeFile(join(root, "mux.stderr"), "z".repeat(16_384))
  await writeFile(join(root, "unrelated.txt"), "must not be collected")
  const evidence = await localControl(parent, "evidence", root, marker)
  assert.equal(evidence.probe_version, 1)
  assert.deepEqual(evidence.files.map((file) => file.name), ["mux.stderr"])
  assert.equal(Buffer.from(evidence.files[0].data_base64, "base64").length, 4096)
  assert.equal(evidence.files[0].truncated, true)
  assert.deepEqual((await localControl(parent, "stop", root, marker)).baseline, [])
  assert.equal((await localControl(parent, "remove", root, marker)).removed, true)
  await assert.rejects(readFile(join(root, "owner.json")), { code: "ENOENT" })
})

test("real PowerShell refuses unclaimed, foreign, and wrong-version staging", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "b".repeat(24)
  const root = join(parent, "w-ssh-" + marker)
  await mkdir(root)
  const unrelated = join(root, "unrelated.txt")
  await writeFile(unrelated, "preserved")
  await assert.rejects(localControl(parent, "claim", root, marker), /Staging already exists/)
  await assert.rejects(localControl(parent, "remove", root, marker))
  await writeFile(join(root, "owner.json"), JSON.stringify({ probe_version: 2, marker }))
  await assert.rejects(localControl(parent, "stop", root, marker), /Unsupported owner version/)
  await writeFile(join(root, "owner.json"), JSON.stringify({ probe_version: 1, marker: "c".repeat(24) }))
  await assert.rejects(localControl(parent, "evidence", root, marker), /Owner marker mismatch/)
  await assert.rejects(localControl(parent, "remove", root, marker), /Owner marker mismatch/)
  assert.equal(await readFile(unrelated, "utf8"), "preserved")
})

test("Windows readiness pins CLI inspection to the owned socket before connecting", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "ab".repeat(12), root = join(parent, "w-ssh-" + marker)
  await localControl(parent, "claim", root, marker)
  await writeFile(join(root, "keeper.json"), JSON.stringify({ probe_version: 1, marker, pid: 42 }))
  const fakes = String.raw`
$env:WEZTERM_UNIX_SOCKET='foreign-socket'
function Get-CimInstance {param($ClassName,$Filter)
 if($Filter -eq "Name='wezterm-mux-server.exe'"){
  [pscustomobject]@{ProcessId=10;CommandLine=(Join-Path $r.root 'server.lua')}
 }else{[pscustomobject]@{CommandLine=(Join-Path $r.root 'probe.mjs');CreationDate=[DateTime]::UtcNow}}
}
function wezterm.exe {
 if($env:WEZTERM_UNIX_SOCKET -cne (Join-Path $r.root 's')){throw 'Readiness selected a foreign socket'}
 $global:LASTEXITCODE=0
}
`
  assert.equal((await localControl(parent, "status", root, marker, { report: "keeper.json" }, fakes)).ready, true)
})

test("real PowerShell publication control writes only an owned versioned probe request", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  assert.equal(typeof lifecycle.remoteProbeScripts.publish, "string")
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "d".repeat(24)
  const root = join(parent, "w-ssh-" + marker)
  await localControl(parent, "claim", root, marker)
  const report = { probe_version: 1, marker, pid: 42, cookie: `${marker}:42:123` }
  await writeFile(join(root, "keeper.json"), JSON.stringify(report))
  const publication = { ...report, action: "publish", generation: "e".repeat(24), sequence: 1 }
  const result = await localControl(parent, "publish", root, marker, { report: "keeper.json", publication })
  assert.equal(result.requested, true)
  assert.deepEqual(JSON.parse(await readFile(join(root, "keeper.json.request.json"), "utf8")), publication)
  for (const update of [{ report: "../foreign.json" }, { publication: { ...publication, probe_version: 2 } },
    { publication: { ...publication, marker: "foreign" } }, { publication: { ...publication, cookie: "foreign" } },
    { publication: { ...publication, action: "unknown" } }, { publication: { ...publication, action: "Publish" } }]) {
    await assert.rejects(localControl(parent, "publish", root, marker, { report: "keeper.json", publication, ...update }))
  }
  const barrier = { ...publication, action: "barrier", sequence: 2 }
  assert.equal((await localControl(parent, "publish", root, marker, { report: "keeper.json", publication: barrier })).requested, true)
  assert.deepEqual(JSON.parse(await readFile(join(root, "keeper.json.request.json"), "utf8")), barrier)
})

test("target spawn control preserves structured request bytes only after keeper acknowledgement", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  assert.equal(typeof lifecycle.remoteProbeScripts.spawn, "string")
  assert.equal(typeof lifecycle.remoteProbeScripts.spawn_status, "string")
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "c".repeat(24), root = join(parent, "w-ssh-" + marker), cookie = marker + ":42:1"
  await localControl(parent, "claim", root, marker)
  await writeFile(join(root, "keeper.json"), JSON.stringify({ probe_version: 1, marker, cookie }))
  const command = { probe_version: 1, action: "spawn", marker, cookie, generation: "d".repeat(24), sequence: 1,
    cwd: root + "/work area \u96ea", args: ["C:/node.exe", "", "quote\"", "snow-\u96ea", "control\n\t\u0001"] }
  const inventory = `function Get-CimInstance {param($ClassName,$Filter) [pscustomobject]@{CommandLine=(Join-Path $r.root 'server.lua')}}\n`
  await assert.rejects(localControl(parent, "spawn", root, marker, { command }, inventory))
  const ack = { ...command, action: "publish", status: "published" }
  await writeFile(join(root, "keeper.json.ack.json"), JSON.stringify({ ...ack, generation: "e".repeat(24) }))
  await assert.rejects(localControl(parent, "spawn", root, marker, { command }, inventory), /acknowledgement/)
  await assert.rejects(readFile(join(root, "spawn.request.json")), { code: "ENOENT" })
  await writeFile(join(root, "keeper.json.ack.json"), JSON.stringify(ack))
  for (const update of [{ probe_version: 2 }, { marker: "foreign" }, { cookie: "foreign" }]) {
    await assert.rejects(localControl(parent, "spawn", root, marker, { command: { ...command, ...update } }, inventory))
  }
  assert.equal((await localControl(parent, "spawn", root, marker, { command }, inventory)).requested, true)
  assert.deepEqual(JSON.parse(await readFile(join(root, "spawn.request.json"), "utf8")), command)
  await assert.rejects(localControl(parent, "spawn", root, marker, { command }, inventory), /already exists/)
  assert.equal((await localControl(parent, "spawn_status", root, marker)).result, null)
  const result = { probe_version: 1, marker, stage: "ready", pane_id: 19 }
  await writeFile(join(root, "spawn.result.json"), JSON.stringify(result))
  assert.deepEqual((await localControl(parent, "spawn_status", root, marker)).result, result)
  await writeFile(join(root, "spawn.result.json"), '{"probe_version":2}')
  await assert.rejects(localControl(parent, "spawn_status", root, marker), /version/)
})

test("observation controls bind phases to the keeper and wait for completed publication", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local observation controls",
}, async (t) => {
  assert.equal(typeof lifecycle.remoteProbeScripts.observe, "string")
  assert.equal(typeof lifecycle.remoteProbeScripts.observation_status, "string")
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "e".repeat(24), root = join(parent, "w-ssh-" + marker)
  await localControl(parent, "claim", root, marker)
  const cookie = marker + ":42:1"
  await writeFile(join(root, "keeper.json"), JSON.stringify({ probe_version: 1, marker, cookie }))
  const observation = { probe_version: 1, action: "observe", marker, generation: "f".repeat(24), cookie, sequence: 1, phase: "before" }
  assert.equal((await localControl(parent, "observe", root, marker, { observation })).requested, true)
  assert.deepEqual(JSON.parse(await readFile(join(root, "observation.request.json"), "utf8")), observation)
  assert.equal((await localControl(parent, "observation_status", root, marker, { phase: "before" })).result, null)
  for (const update of [{ probe_version: 2 }, { marker: "foreign" }, { phase: "../escape" }, { cookie: "foreign" }]) {
    await assert.rejects(localControl(parent, "observe", root, marker, { observation: { ...observation, ...update } }))
  }
  const after = { ...observation, phase: "after" }
  assert.equal((await localControl(parent, "observe", root, marker, { observation: after })).requested, false)
  await writeFile(join(root, "keeper.json.ack.json"), JSON.stringify({ ...after, action: "publish", status: "published" }))
  assert.equal((await localControl(parent, "observe", root, marker, { observation: after })).requested, true)
  const result = { ...after, stage: "observed", panes: [] }
  await writeFile(join(root, "observation.after.json"), JSON.stringify(result))
  assert.deepEqual((await localControl(parent, "observation_status", root, marker, { phase: "after" })).result, result)
  await writeFile(join(root, "observation.after.json"), '{"probe_version":2}')
  await assert.rejects(localControl(parent, "observation_status", root, marker, { phase: "after" }), /version/)
})

test("numeric Windows reparse tags allow only the exact owned AF_UNIX socket without localized CLI output", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for local filesystem checks",
}, async (t) => {
  assert.equal(typeof lifecycle.nativeReparseTagQuery, "string")
  assert.doesNotMatch(lifecycle.remoteProbeScripts.remove, /fsutil|Reparse Tag Value/)
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "f".repeat(24), root = join(parent, "w-ssh-" + marker)
  await localControl(parent, "claim", root, marker)
  await writeFile(join(root, "s"), "fake AF_UNIX leaf")
  const inventory = (leaf) => `
function Get-ChildItem {param($LiteralPath,[switch]$Force,[switch]$Recurse,$Attributes)
 [pscustomobject]@{FullName=(Join-Path $LiteralPath '${leaf}')}
}
`
  const query = (tag) => `function Get-ProbeReparseTag($path) { [uint64]${tag} }\n`
  await assert.rejects(localControl(parent, "remove", root, marker, {}, inventory("s"), query(0xa000000c)), /reparse-point/)
  await assert.rejects(localControl(parent, "remove", root, marker, {}, inventory("other"), query(0x80000023)), /reparse-point/)
  await assert.rejects(localControl(parent, "remove", root, marker, {}, inventory("s"),
    "function Get-ProbeReparseTag($path) { throw 'Native query failed' }\n"), /Native query failed/)
  assert.equal(await readFile(join(root, "s"), "utf8"), "fake AF_UNIX leaf")
  assert.equal((await localControl(parent, "remove", root, marker, {}, inventory("s"), query(0x80000023))).removed, true)
  await assert.rejects(readFile(join(root, "owner.json")), { code: "ENOENT" })
})

test("the native Windows tag query compiles with the selected PowerShell without invoking kernel32", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for native-query compilation",
}, async () => {
  assert.equal(typeof lifecycle.nativeReparseTagQuery, "string")
  const source = lifecycle.nativeReparseTagQuery + "\n$null=Get-Command Get-ProbeReparseTag -ErrorAction Stop\n"
  const encoded = lifecycle.powershellCommand(source).split(" ").at(-1)
  await runProbeCommand(process.env.WISP_TEST_POWERSHELL, ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { input: "" })
})

test("native Windows handles distinguish an owned junction from its ordinary target", {
  skip: process.platform === "win32" && process.env.WISP_TEST_POWERSHELL ? false : "requires native Windows PowerShell and filesystem handles",
}, async (t) => {
  assert.equal(typeof lifecycle.nativeReparseTagQuery, "string")
  const root = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-tag-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const source = lifecycle.probeInput + lifecycle.nativeReparseTagQuery + String.raw`
$target=Join-Path $r.root 'target';$link=Join-Path $r.root 'junction'
$null=New-Item -ItemType Directory -Path $target
$null=New-Item -ItemType Junction -Path $link -Target $target
@{probe_version=1;target=(Get-ProbeReparseTag $target);junction=(Get-ProbeReparseTag $link)}|ConvertTo-Json -Compress
`
  const encoded = lifecycle.powershellCommand(source).split(" ").at(-1)
  const result = await runProbeCommand(process.env.WISP_TEST_POWERSHELL,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], {
      input: JSON.stringify({ probe_version: 1, root }) + "\n",
    })
  const tags = JSON.parse(result.stdout)
  assert.equal(tags.probe_version, 1)
  assert.equal(tags.target, 0)
  assert.equal(tags.junction, 0xa0000003, "open-reparse-point handles must inspect the junction, not its target")
})

const unixSocketFixture = new URL("./fixtures/remote-mux/windows-unix-socket.ps1", import.meta.url)

test("the native AF_UNIX fixture compiles without opening a socket", {
  skip: process.env.WISP_TEST_POWERSHELL ? false : "set WISP_TEST_POWERSHELL for native-fixture compilation",
}, async () => {
  const source = await readFile(unixSocketFixture, "utf8")
  const encoded = lifecycle.powershellCommand(source).split(" ").at(-1)
  await runProbeCommand(process.env.WISP_TEST_POWERSHELL,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { input: "" })
})

test("native AF_UNIX socket cleanup verifies the tag and process-exit guard", {
  skip: process.platform === "win32" && process.env.WISP_TEST_POWERSHELL ? false : "requires native Windows AF_UNIX and PowerShell",
}, async (t) => {
  const parent = await mkdtemp(join(process.env.WISP_TEST_TMPDIR ?? tmpdir(), "w-life-"))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const marker = "a1".repeat(12), root = join(parent, "w-ssh-" + marker)
  await localControl(parent, "claim", root, marker)
  const source = lifecycle.probeInput + lifecycle.nativeReparseTagQuery
    + await readFile(unixSocketFixture, "utf8") + String.raw`
$path=Join-Path $r.root 's'
[WispProbeUnixSocket]::Bind($path)
@{probe_version=1;tag=(Get-ProbeReparseTag $path)}|ConvertTo-Json -Compress
`
  // Use a file for this test-only combination of native helpers. The real
  // remote cleanup command is separately held to its fixed SSH byte budget.
  const fixture = join(parent, "create-socket.ps1")
  await writeFile(fixture, "$ErrorActionPreference='Stop'\n" + source)
  const result = await runProbeCommand(process.env.WISP_TEST_POWERSHELL,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "& ([scriptblock]::Create([IO.File]::ReadAllText($env:WISP_TEST_SOCKET_FIXTURE)))"], {
      env: { ...process.env, WISP_TEST_SOCKET_FIXTURE: fixture },
      input: JSON.stringify({ probe_version: 1, root }) + "\n",
    })
  const socket = JSON.parse(result.stdout)
  assert.equal(socket.probe_version, 1)
  assert.equal(socket.tag, 0x80000023, "a real Windows AF_UNIX leaf must have the exact accepted tag")
  const running = `function Get-CimInstance {param($ClassName,$Filter)
if($Filter -eq "Name='node.exe'"){[pscustomobject]@{CommandLine=(Join-Path $r.root 'probe.mjs')}}
}\n`
  await assert.rejects(localControl(parent, "remove", root, marker, {}, running), /Owned processes remain/)
  assert.equal(JSON.parse(await readFile(join(root, "owner.json"), "utf8")).marker, marker)
  // runProbeCommand has observed the socket creator's stdio close and exit.
  assert.equal((await localControl(parent, "remove", root, marker)).removed, true)
  await assert.rejects(stat(root), { code: "ENOENT" })
})
