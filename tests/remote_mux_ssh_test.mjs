import assert from "node:assert/strict"
import { execFile, spawn } from "node:child_process"
import { randomBytes } from "node:crypto"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import { fileURLToPath } from "node:url"
import { promisify } from "node:util"
import test from "node:test"
import { attachmentReady, attachmentRecovered, createMuxLogCapture, muxDiagnosticEnv, muxSocketPath, prepareMuxTarget, runMuxAcceptance, runProbeCommand, validatePublicationObservation, waitForReadiness } from "./remote_mux_support.mjs"
import { assertWindowsCwd, muxSshOptions, openSshMuxArgs, renderMuxClient } from "./remote_mux_client.mjs"
import { renderMuxServer } from "./remote_mux_server.mjs"
import { prepareMuxProxy, readMuxProxy, stopMuxProxy } from "./remote_mux_proxy.mjs"
import { createPublicationCoordinator, runMuxLifecycle } from "./remote_mux_scenarios.mjs"
import {
  createRemoteProbe, powershellCommand as powershell, probeInput as input,
  readProbeTail, remoteProbeScripts, retainProbeEvidence, stopProbeChild, withProbeCleanup,
} from "./remote_mux_lifecycle.mjs"

const exec = promisify(execFile)
const host = process.env.WISP_TEST_SSH_HOST
const transportKind = process.env.WISP_TEST_MUX_TRANSPORT ?? "native-ssh"
const checkKind = process.env.WISP_TEST_MUX_CHECK ?? "association"
const marker = () => randomBytes(12).toString("hex")
const args = ["", "space value", 'double"quote', "single'quote", "snow-\u96ea", "emoji-\ud83d\ude80", "semi;&|<>", "$literal", "line\n\t\u0001", "--flag=value"]
const sshOptions = muxSshOptions

test(`${transportKind} mux ${checkKind} acceptance`, {
  skip: host ? false : "set WISP_TEST_SSH_HOST explicitly; this test creates an isolated mux on that Windows host",
  timeout: 300_000,
}, async (t) => {
  assert.ok(process.platform !== "win32", "this first cross-machine fixture requires a POSIX client")
  assert.match(host, /^[A-Za-z0-9][A-Za-z0-9._-]*$/, "use a configured SSH alias, not shell syntax")
  assert.ok(["native-ssh", "openssh"].includes(transportKind), "select an explicit native-ssh or openssh fixture transport")
  assert.ok(["association", "full"].includes(checkKind), "select association or full acceptance explicitly")
  const ssh = process.env.WISP_TEST_SSH ?? "ssh"
  if (transportKind === "openssh") assert.ok(isAbsolute(ssh), "set an absolute WISP_TEST_SSH for the OpenSSH-backed fixture")
  const wezterm = process.env.WISP_TEST_WEZTERM
  const mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm), "set an absolute WISP_TEST_WEZTERM")
  assert.ok(mux && isAbsolute(mux), "set an absolute WISP_TEST_MUX_SERVER")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX", "c1"))
  const local = await mkdtemp(join(parent, "w-"))
  const socket = muxSocketPath(local)
  const run = marker()
  let remote, bootstrap
  const logs = []
  const controls = new Set()
  const artifacts = []
  const proxies = []
  const clients = [], closedChildren = new WeakMap(), publications = createPublicationCoordinator(run)
  const evidence = { probe_version: 1, marker: run, transport: transportKind, check: checkKind, files: [], collection_errors: [], progress: [], children: [], proxies: [], observations: {}, observation_errors: [], publications: [], persistence: [] }
  const started = performance.now()
  function progress(stage, detail) {
    evidence.progress.push({ stage, detail, elapsed_ms: Math.round(performance.now() - started) })
    t.diagnostic(`${stage}: ${detail}`)
  }
  const missingSnapshot = (error) => error.code === "ENOENT"
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("WEZTERM_")))
  env.WEZTERM_UNIX_SOCKET = socket
  for (const [name, directory] of [["XDG_CACHE_HOME", "cache"], ["XDG_DATA_HOME", "data"], ["XDG_RUNTIME_DIR", "run"]]) {
    env[name] = join(local, directory)
  }
  const sshArgs = (command) => [...sshOptions, host, command]
  function record(data) {
    if (!data) return
    logs.push(data.toString().slice(-4096))
    if (logs.length > 16) logs.shift()
  }
  function tracked(executable, argv, { label = executable, ...options } = {}) {
    const child = spawn(executable, argv, { env, stdio: ["pipe", "pipe", "pipe"], ...options })
    const output = { label, pid: child.pid, started_ms: Math.round(performance.now() - started), stdout: "", stderr: "", stages: [] }
    evidence.children.push(output)
    closedChildren.set(child, new Promise((resolve) => child.once("close", () => {
      output.closed_ms = Math.round(performance.now() - started)
      resolve()
    })))
    child.once("exit", (code, signal) => {
      output.exit_code = code; output.signal = signal; output.exited_ms = Math.round(performance.now() - started)
    })
    child.on("error", (error) => record(String(error)))
    child.stdin.on("error", (error) => record(String(error)))
    for (const name of ["stdout", "stderr"]) {
      const capture = createMuxLogCapture((stage) => {
        if (output.stages.includes(stage)) return
        output.stages.push(stage)
        progress(stage, label)
      })
      child[name].on("data", (bytes) => {
        record(bytes)
        capture.write(bytes)
        output[name] = capture.tail
      })
      child[name].on("end", () => capture.end())
    }
    return child
  }
  async function stop(child, signal = "SIGTERM") {
    await stopProbeChild(child, closedChildren.get(child), { signal })
  }
  async function control(action, request = {}, signal = t.signal) {
    const payload = JSON.stringify({ probe_version: 1, marker: run, root: remote?.root, ...request })
    const task = runProbeCommand(ssh, sshArgs(powershell(input + remoteProbeScripts[action])), {
      input: payload + "\n", env, signal,
    })
    controls.add(task)
    let output
    try {
      const { stdout, stderr } = await task
      output = stdout
      record(stderr)
      const response = JSON.parse(stdout)
      assert.equal(response.probe_version, 1, "control response version")
      return response
    } catch (error) {
      record(error.stderr)
      record(error.stdout ?? output)
      throw error
    } finally { controls.delete(task) }
  }
  const remoteProbe = createRemoteProbe(control)
  async function captureEvidence() {
    const errors = []
    evidence.remote = remoteProbe.info
    try { evidence.server = await remoteProbe.evidence(AbortSignal.timeout(15_000)) } catch (error) { errors.push(error) }
    for (const path of artifacts) {
      try { evidence.files.push(await readProbeTail(path)) } catch (error) { if (error.code !== "ENOENT") errors.push(error) }
    }
    evidence.collection_errors = errors.map((error) => error.message.slice(-4096))
    if (errors.length) throw new AggregateError(errors, evidence.collection_errors.join("\n"))
  }
  const status = (report = "keeper.json", signal = t.signal) => control("status", { report }, signal)
  async function stopProxy(proxy) {
    if (!proxy) return
    proxy.stopped = false
    await stopMuxProxy(proxy.path, proxy.request)
    proxy.stopped = true
    const final = await readMuxProxy(proxy.path, proxy.request)
    if (final && !evidence.proxies.some((entry) => entry.generation === final.generation)) evidence.proxies.push(final)
  }
  async function stopProxies() {
    const results = await Promise.allSettled(proxies.map((proxy) => stopProxy(proxy)))
    const failures = results.filter((value) => value.status === "rejected").map((value) => value.reason)
    if (failures.length) throw new AggregateError(failures)
  }
  async function stopClient(connection, signal = "SIGTERM") {
    await stop(connection.child, signal)
    await stopProxy(connection.ownedProxy)
  }
  async function attach(phase) {
    const directory = join(local, `c${clients.length + 1}`)
    const clientSocket = muxSocketPath(directory)
    await mkdir(directory)
    const clientEnv = { ...env, WEZTERM_UNIX_SOCKET: clientSocket }
    const generation = marker()
    const metadata = join(directory, `state-${generation}.json`)
    const config = join(directory, `client-${generation}.lua`)
    artifacts.push(config, metadata, metadata + ".tmp")
    let transport = { kind: "native-ssh" }, ownedProxy
    if (transportKind === "openssh") {
      const path = join(directory, `proxy-${generation}.json`)
      const request = { probe_version: 1, marker: run, generation, args: openSshMuxArgs({ ssh, host, remoteRoot: remote.root }) }
      const prepared = await prepareMuxProxy(path, request)
      ownedProxy = { path, request }
      proxies.push(ownedProxy)
      artifacts.push(path, path + ".state.json", path + ".claim.json")
      transport = { kind: "openssh", socket: join(directory, "p"), args: prepared.args }
    }
    await writeFile(config, renderMuxClient({ host, remoteRoot: remote.root, socket: clientSocket, metadata, marker: run, generation, transport }))
    const cli = async (...argv) => (await exec(wezterm, ["--config-file", config, "cli", "--prefer-mux", "--no-auto-start", ...argv],
      { env: clientEnv, timeout: 10_000, maxBuffer: 1024 * 1024 })).stdout.trim()
    const child = tracked(mux, ["--config-file", config], {
      cwd: directory, label: `${phase}:${generation}`,
      env: muxDiagnosticEnv(clientEnv),
    })
    const connection = { cli, metadata, generation, child, ownedProxy }
    clients.push(connection)
    try {
      await waitForReadiness(child, async (signal) => {
        if (ownedProxy) {
          const state = await readMuxProxy(ownedProxy.path, ownedProxy.request)
          if (state?.stage === "exited") throw new Error(`OpenSSH mux proxy exited before attachment: ${state.error ?? state.child_code ?? state.child_signal ?? state.reason}`)
        }
        const snapshot = JSON.parse(await readFile(metadata, { encoding: "utf8", signal }))
        if (!evidence.progress.some((entry) => entry.stage === "client-config-loaded" && entry.detail === generation)) {
          assert.equal(snapshot.probe_version, 1)
          progress("client-config-loaded", generation)
        }
        return attachmentReady(snapshot, run, generation) && snapshot
      }, { timeout: 30_000, signal: t.signal, retryError: missingSnapshot })
    } catch (error) {
      // A newly attached endpoint is not owned until the keeper handshake.
      // Do not read terminal contents from an unverified endpoint on failure.
      const inspect = async (...argv) => (await exec(wezterm,
        ["--config-file", config, "cli", "--prefer-mux", "--no-auto-start", ...argv],
        { env: clientEnv, timeout: 2000, maxBuffer: 32_768 })).stdout
      try {
        const panes = JSON.parse(await inspect("list", "--format", "json"))
        logs.push(`isolated client pane IDs: ${panes.map((pane) => pane.pane_id).join(",")}`)
      } catch (failure) { logs.push(`owned client inspection: ${failure.message}`) }
      throw error
    }
    t.diagnostic(`${phase}: fresh generation ${generation} attached`)
    const panes = JSON.parse(await cli("list", "--format", "json"))
    progress("client-pane-list", `${phase}:${generation}: ${panes.length} panes`)
    return connection
  }
  async function observeTarget(publication, phase) {
    const observation = { ...publication, action: "observe", phase }
    await waitForReadiness(null, async (signal) => (await control("observe", { observation }, signal)).requested,
      { timeout: 8000, signal: t.signal })
    const snapshot = await waitForReadiness(null, async (signal) => {
      const { result } = await control("observation_status", { phase }, signal)
      return result && validatePublicationObservation(result, publication, phase)
    }, { timeout: 8000, signal: t.signal })
    evidence.observations[phase] = snapshot
    progress("target-vars-observed", `${phase}: ${snapshot.panes.map((pane) => `pane ${pane.pane_id} [${Object.keys(pane.vars).join(",")}]`).join("; ")}`)
  }
  function recover(connection, report, name) {
    return publications.submit({ probe_version: 1, action: "publish", marker: run, generation: connection.generation, cookie: report.cookie },
      (publication) => recoverPublication(connection, name, publication))
  }
  async function recoverPublication(connection, name, publication) {
    const diagnostic = name === "keeper.json" && publication.sequence === 1
    if (diagnostic) await observeTarget(publication, "before")
    let pane, failure
    try {
      assert.equal((await control("publish", { report: name, publication })).requested, true)
      pane = await waitForReadiness(connection.child, async (signal) => {
        const snapshot = JSON.parse(await readFile(connection.metadata, { encoding: "utf8", signal }))
        const matched = attachmentRecovered(snapshot, publication)
        if (matched) evidence.publications.push({ ...publication, report: name, pane_id: matched.pane_id, snapshot })
        return matched
      }, { timeout: 10_000, signal: t.signal, retryError: missingSnapshot })
    } catch (error) { failure = error }
    if (diagnostic) {
      try { await observeTarget(publication, "after") } catch (error) {
        evidence.observation_errors.push(error.message.slice(-2048))
        failure ??= error
      }
    }
    if (failure) throw failure
    t.diagnostic(`${name}: original token recovered in client pane ${pane.pane_id}, sequence ${publication.sequence}`)
    return pane
  }
  async function runProbe() {
    const effective = await runProbeCommand(ssh, [...sshOptions, "-G", host], { input: "", env, signal: t.signal })
    for (const name of ["XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"]) await mkdir(env[name])
    const cliVersion = (await exec(wezterm, ["--version"], { env, timeout: 5000 })).stdout.trim()
    const muxVersion = (await exec(mux, ["--version"], { env, timeout: 5000 })).stdout.trim()
    assert.equal(cliVersion.split(/\s+/).at(-1), muxVersion.split(/\s+/).at(-1), "local CLI/mux versions match")
    t.diagnostic(`local ${cliVersion}`)
    remote = await prepareMuxTarget(effective.stdout, transportKind, () => remoteProbe.prepare(t.signal))
    progress("ssh-effective-config-read", `${transportKind}: effective configuration checked; routing/trust remain owned by the selected SSH implementation`)
    progress("remote-preflight", `${remote.version}; baseline muxes ${remote.baseline.length}`)
    muxSocketPath(remote.root, "win32")
    t.diagnostic(`remote ${remote.version}; existing mux processes ${remote.baseline.length}`)
    const probe = await readFile(fileURLToPath(new URL("./fixtures/remote-mux/ssh-probe.mjs", import.meta.url)), "utf8")
    const targetHelper = await readFile(fileURLToPath(new URL("./fixtures/remote-mux/target-spawn.lua", import.meta.url)), "utf8")
    const observer = await readFile(fileURLToPath(new URL("./fixtures/remote-mux/target-observe.lua", import.meta.url)), "utf8")
    const serverConfig = renderMuxServer({ root: remote.root, marker: run, node: remote.node })
    let staged
    for (const [name, source] of [["probe.mjs", probe], ["target-spawn.lua", targetHelper], ["target-observe.lua", observer], ["server.lua", serverConfig]]) {
      staged = await control("stage", { files: { [name]: Buffer.from(source).toString("base64") } })
    }
    progress("remote-fixtures-staged", remote.root)
    assertWindowsCwd(staged.cwd, remote.root + "/work area \u96ea")
    bootstrap = tracked(ssh, sshArgs(powershell(input + remoteProbeScripts.bootstrap)))
    bootstrap.stdin.end(JSON.stringify({ probe_version: 1, marker: run, root: remote.root }) + "\n")
    const before = await waitForReadiness(bootstrap, async (signal) => {
      const value = await status("keeper.json", signal)
      return value.ready && value
    }, { timeout: 30_000, signal: t.signal })
    t.diagnostic(`Windows listener and keeper ready; startup SSH exit=${bootstrap.exitCode}`)
    progress("remote-listener-ready", `mux ${before.mux_pid}; keeper ${before.report.pid}`)
    // Prove target Lua startup and scheduling before spending an attachment
    // attempt or publishing a token. A codec handshake cannot prove this path.
    for (const [phase, stage] of [["startup", "started"], ["timer", "ticking"]]) {
      await waitForReadiness(bootstrap, async (signal) => {
        const { result } = await control("observation_status", { phase }, signal)
        if (!result) return false
        assert.equal(result.probe_version, 1)
        assert.equal(result.marker, run)
        assert.equal(result.stage, stage)
        return true
      }, { timeout: 8000, signal: t.signal })
      progress("target-observer-ready", phase)
    }
    await runMuxAcceptance(checkKind, {
      associate: async () => {
        const connection = await attach("initial")
        const keeperPane = await recover(connection, before.report, "keeper.json")
        const current = await status()
        assert.equal(current.ready, true)
        assert.equal(current.mux_pid, before.mux_pid)
        assert.equal(current.process_created, before.process_created)
        assert.equal(current.report.cookie, before.report.cookie)
        assert.equal((await status("argv.json")).report, null, "association-only cannot create an argv probe")
        progress("owned-server-associated", connection.generation)
        return { connection, keeperPane }
      },
      full: async ({ connection, keeperPane }) => {
        const command = { probe_version: 1, action: "spawn", marker: run, generation: connection.generation,
          cookie: before.report.cookie, sequence: 1, cwd: staged.cwd,
          args: [remote.node, remote.root + "/probe.mjs", remote.root + "/argv.json", remote.root + "/stop", ...args] }
        assert.equal((await control("spawn", { command })).requested, true)
        const spawned = await waitForReadiness(connection.child, async (signal) => {
          const { result } = await control("spawn_status", {}, signal)
          if (!result) return false
          assert.equal(result.probe_version, 1, "target spawn result version")
          if (result.stage === "failed") throw new Error(result.error)
          for (const field of ["marker", "generation", "cookie", "sequence"]) assert.equal(result[field], command[field], "target result identity")
          assert.ok(Number.isSafeInteger(result.pane_id) && result.pane_id >= 0, "target-local pane ID")
          return result.stage === "ready" && result
        }, { timeout: 10_000, signal: t.signal })
        const native = await waitForReadiness(null, async (signal) => {
          const value = await status("argv.json", signal)
          return value.ready && value
        }, { signal: t.signal })
        assertWindowsCwd(native.report.cwd, staged.cwd)
        assert.deepEqual(native.report.args, args, "structured argv is constructed natively on Windows")
        assert.equal(native.report.target_env, run, "the command inherits the target mux environment")
        assert.equal(String(native.report.pane_id), String(spawned.pane_id), "report matches the target-local spawn result")
        const nativePane = await recover(connection, native.report, "argv.json")
        assert.notEqual(nativePane.pane_id, keeperPane.pane_id, "the spawned process has a distinct client-local association")
        const text = async (owner, pane) => (await owner.cli("get-text", "--pane-id", String(pane.pane_id))).replace(/[\r\n]/g, "")
        assert.ok((await text(connection, keeperPane)).includes(before.report.cookie), "only the verified keeper terminal is read")
        const result = await runMuxLifecycle({
          initial: connection, startup: bootstrap, attach, stopClient,
          recover: async (owner) => {
            owner.keeperPane = await recover(owner, before.report, "keeper.json")
            owner.nativePane = await recover(owner, native.report, "argv.json")
            assert.notEqual(owner.keeperPane.pane_id, owner.nativePane.pane_id)
          },
          verify: async (phase) => {
            const keeper = await status(), probe = await status("argv.json")
            for (const [current, original] of [[keeper, before], [probe, native]]) {
              assert.equal(current.ready, true, phase)
              assert.equal(current.mux_pid, before.mux_pid, phase)
              assert.equal(current.process_created, original.process_created, phase)
              assert.deepEqual(current.report, original.report, phase)
            }
            evidence.persistence.push({ phase, mux_pid: keeper.mux_pid, keeper_pid: keeper.report.pid,
              keeper_created: keeper.process_created, probe_pid: probe.report.pid, probe_created: probe.process_created })
            progress("original-processes-preserved", phase)
          },
          stopStartup: async () => { await stop(bootstrap); bootstrap = undefined },
          interruptProxy: transportKind === "openssh" ? async (owner) => {
            await stopMuxProxy(owner.ownedProxy.path, owner.ownedProxy.request, { abrupt: true, allowDefunct: true })
            const interrupted = await readMuxProxy(owner.ownedProxy.path, owner.ownedProxy.request)
            assert.equal(interrupted.reason, "transport-interrupted")
            assert.equal(interrupted.child_signal, "SIGKILL")
            assert.equal(interrupted.group_gone, true)
            progress("owned-proxy-interrupted", owner.generation)
          } : undefined,
          observe: (phase, value) => progress("channel-running", `${phase}: pid ${value.pid}`),
        })
        evidence.coverage = result.coverage
        const final = result.connection
        assert.ok((await text(final, final.keeperPane)).includes(before.report.cookie), "reconnect reads the same process output")
        t.diagnostic(result.summary)
      },
    })
  }
  try {
    await withProbeCleanup(runProbe, [
      ["drain controls", () => Promise.allSettled([...controls])],
      ["capture evidence", captureEvidence],
      ["stop clients", async () => {
        const results = await Promise.allSettled(clients.map((connection) => stopClient(connection)))
        const failures = results.filter((value) => value.status === "rejected").map((value) => value.reason)
        if (failures.length) throw new AggregateError(failures)
      }],
      ["stop owned mux proxies", stopProxies],
      ["stop startup connection", () => stop(bootstrap)],
      ["remote cleanup", () => remoteProbe.cleanup(AbortSignal.timeout(15_000))],
      ["local cleanup", async () => {
        assert.ok(proxies.every((proxy) => proxy.stopped), `owned proxy cleanup unconfirmed; local staging retained: ${local}`)
        await rm(local, { recursive: true, force: true })
      }],
      ["retain evidence", async (failure) => {
        const path = join(parent, `w-ssh-${run}.evidence.json`)
        await retainProbeEvidence(path, { ...evidence, outcome: failure ? "failed" : "passed", error: failure?.message.slice(-8192), logs })
        t.diagnostic(`bounded evidence retained: ${path}`)
      }],
    ])
  } catch (error) {
    throw new Error(`${error.message}\n${logs.join("")}`, { cause: error })
  }
})
