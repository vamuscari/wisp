import { execFile } from "node:child_process"
import { posix, win32 } from "node:path"
import { performance } from "node:perf_hooks"
import { StringDecoder } from "node:string_decoder"
import { setTimeout as delay } from "node:timers/promises"
import { promisify } from "node:util"

const exec = promisify(execFile)

export function muxConnectionStages(log) {
  const stages = []
  if (/INFO\s+libssh::/.test(log)) stages.push("libssh-callback-observed")
  if (/INFO\s+libssh::[^\n]*SSH-2\.0-WispDiagnosticFixture/.test(log)) stages.push("fixture-banner-observed")
  if (/libssh::ssh_config_parse_file[^\n]*Reading configuration data/.test(log)) stages.push("ssh-config-parsing")
  // Resolution follows config parsing, but precedes the blocking socket connect.
  if (/wezterm_ssh::sessioninner[^\n]*> resolved /.test(log)) stages.push("ssh-address-resolved")
  // In the inspected client, this message follows successful SSH authentication
  // but precedes sess.exec. It proves a request, not successful proxy startup.
  if (/wezterm_client::client[^\n]*going to run /.test(log)
    || /debug1: Sending command: WEZTERM_UNIX_SOCKET=([A-Za-z]:\/[A-Za-z0-9_./-]+)\/s wezterm\.exe --config-file \1\/server\.lua cli --prefer-mux --no-auto-start proxy/.test(log)) {
    stages.push("ssh-authenticated", "proxy-exec-requested")
  }
  if (/wezterm_client::client[^\n]*Server version is [^\n]*\(codec version \d+\)/.test(log)) stages.push("codec-verified")
  return stages
}

function effectiveSshFields(source) {
  const fields = new Map()
  const relevant = new Set(["hostname", "port", "user", "proxyjump", "hostkeyalias"])
  for (const line of source.split(/\r?\n/)) {
    const match = /^(\S+)\s+(.*)$/.exec(line)
    if (!match || !relevant.has(match[1])) continue
    if (fields.has(match[1])) throw new Error(`duplicate effective SSH configuration field: ${match[1]}`)
    fields.set(match[1], match[2].trim())
  }
  for (const name of ["hostname", "port", "user"]) {
    if (!fields.get(name)) throw new Error(`missing effective SSH configuration field: ${name}`)
  }
  return fields
}

export function validateNativeSshConfig(source) {
  const fields = effectiveSshFields(source)
  if (fields.has("proxyjump") && fields.get("proxyjump") !== "none") {
    throw new Error("native WezTerm SSH does not implement ProxyJump; refusing a direct connection that bypasses the configured route")
  }
  if (fields.has("hostkeyalias") && fields.get("hostkeyalias") !== "none") {
    throw new Error("native WezTerm/libssh does not honor HostKeyAlias; refusing to change the configured host trust identity")
  }
}

export async function prepareMuxTarget(config, transport, prepare) {
  if (!["native-ssh", "openssh"].includes(transport)) throw new Error("unsupported cross-machine mux transport")
  if (transport === "native-ssh") validateNativeSshConfig(config)
  else effectiveSshFields(config)
  return prepare()
}

export async function runMuxAcceptance(mode = "association", { associate, full }) {
  if (!["association", "full"].includes(mode)) throw new Error("unsupported mux acceptance mode")
  const owned = await associate()
  return mode === "full" ? full(owned) : owned
}

export function validatePublicationObservation(snapshot, publication, phase) {
  if (publication?.probe_version !== 1 || snapshot?.probe_version !== 1) throw new Error("unsupported observation version")
  if (publication.action !== "publish" || snapshot.action !== "observe" || snapshot.stage !== "observed"
    || !["before", "after"].includes(phase) || snapshot.phase !== phase || !Array.isArray(snapshot.panes)) {
    throw new Error("invalid publication observation")
  }
  for (const field of ["marker", "generation", "cookie", "sequence"]) {
    if (snapshot[field] !== publication[field]) throw new Error(`foreign publication observation: ${field}`)
  }
  return snapshot
}

export function muxDiagnosticEnv(env) {
  return { ...env, WEZTERM_LOG: "wezterm_client=trace,wezterm_ssh=debug,libssh=info" }
}

export function createMuxLogCapture(onStage) {
  const decoder = new StringDecoder("utf8")
  const seen = new Set(), partialStages = new Set()
  let pending = "", tail = Buffer.alloc(0), ended = false
  function report(stages) {
    for (const stage of stages) {
      if (seen.has(stage)) continue
      seen.add(stage)
      onStage(stage)
    }
  }
  function parse(text, final = false) {
    const combined = pending + text
    const boundary = combined.lastIndexOf("\n") + 1
    if (boundary) {
      report([...partialStages, ...muxConnectionStages(combined.slice(0, boundary))])
      partialStages.clear()
    }
    pending = combined.slice(boundary)
    if (final) {
      report([...partialStages, ...muxConnectionStages(pending)])
      pending = ""
      partialStages.clear()
    } else if (pending.length > 4096) {
      // Preserve discovered milestones while bounding an overlong partial line.
      for (const stage of muxConnectionStages(pending)) partialStages.add(stage)
      pending = pending.slice(-4096)
    }
  }
  return {
    get tail() { return tail.toString("utf8") },
    write(bytes) {
      if (ended) throw new Error("diagnostic stream already ended")
      parse(decoder.write(bytes))
      tail = Buffer.from(Buffer.concat([tail, bytes]).subarray(-4096))
    },
    end() {
      if (ended) return
      ended = true
      parse(decoder.end(), true)
    },
  }
}

export async function runProbeCommand(executable, args, {
  input, env = process.env, signal, timeout = 15_000, maxBuffer = 65_536,
}) {
  if (Buffer.byteLength(input, "utf8") >= 8192) throw new Error("probe control frame must be smaller than 8 KiB")
  signal?.throwIfAborted()
  const task = exec(executable, args, { env, signal, timeout, maxBuffer, killSignal: "SIGKILL", windowsHide: true })
  const child = task.child
  const closed = new Promise((resolve) => child.once("close", resolve))
  let inputError
  child.stdin.on("error", (error) => { inputError = error })
  if (input.length) child.stdin.end(input)
  else child.stdin.end()
  try {
    const result = await task
    if (inputError) throw inputError
    return result
  } catch (error) {
    error.pid = child.pid
    throw error
  } finally {
    // Abort/spawn errors may reject before stdio closes. Do not leave an owned
    // control process running while the caller proceeds to inspect or clean up.
    if (child.pid && child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
  }
}

export function muxSocketPath(directory, platform = process.platform) {
  const paths = platform === "win32" ? win32 : posix
  if (!paths.isAbsolute(directory) || directory.includes("\0")) throw new Error("socket directory must be absolute")
  const socket = paths.join(directory, "s")
  const limit = platform === "darwin" ? 103 : 107
  const bytes = Buffer.byteLength(socket, "utf8")
  if (bytes > limit) throw new Error(`socket path is ${bytes} bytes; limit is ${limit}; select a shorter WISP_TEST_TMPDIR`)
  return socket
}

export function attachmentReady(snapshot, marker, generation) {
  if (snapshot?.probe_version !== 1) throw new Error("unsupported attachment snapshot version")
  if (snapshot.marker !== marker || snapshot.generation !== generation) return false
  if (snapshot.stage === "failed") throw new Error(`client attachment failed: ${snapshot.error}`)
  return snapshot.stage === "ready" && Array.isArray(snapshot.panes)
    && snapshot.panes.some((pane) => pane.domain === "remote-test")
}

export function attachmentRecovered(snapshot, request) {
  if (request?.probe_version !== 1) throw new Error("unsupported publication request version")
  if (request.action !== "publish") throw new Error("expected the publish publication action")
  if (!attachmentReady(snapshot, request.marker, request.generation)) return false
  const matches = snapshot.panes.filter((pane) => {
    if (pane.domain !== "remote-test" || pane.vars?.WISP_TEST_ATTACHMENT !== request.cookie
      || !pane.vars.WISP_TEST_ATTACHMENT_ACK) return false
    const ack = JSON.parse(pane.vars.WISP_TEST_ATTACHMENT_ACK)
    if (ack?.probe_version !== 1) throw new Error("unsupported publication acknowledgement version")
    return ack.action === "publish" && ack.status === "published" && ack.marker === request.marker && ack.cookie === request.cookie
      && ack.generation === request.generation && ack.sequence === request.sequence
  })
  if (matches.length > 1) throw new Error("ambiguous attachment token on multiple panes")
  return matches[0] ?? false
}

export function attachmentAbsentAfterBarrier(snapshot, request) {
  if (request?.probe_version !== 1) throw new Error("unsupported notification barrier request version")
  if (request.action !== "barrier") throw new Error("expected a notification barrier request")
  if (!attachmentReady(snapshot, request.marker, request.generation)) return false
  const matches = snapshot.panes.filter((pane) => {
    if (pane.domain !== "remote-test" || !pane.vars?.WISP_TEST_BARRIER) return false
    const barrier = JSON.parse(pane.vars.WISP_TEST_BARRIER)
    if (barrier?.probe_version !== 1) throw new Error("unsupported notification barrier version")
    return barrier.action === "barrier" && barrier.status === "barrier" && barrier.marker === request.marker
      && barrier.cookie === request.cookie && barrier.generation === request.generation && barrier.sequence === request.sequence
  })
  if (matches.length > 1) throw new Error("ambiguous notification barrier on multiple panes")
  if (!matches.length) return false
  if (snapshot.panes.some((pane) => pane.domain === "remote-test" && pane.vars?.WISP_TEST_ATTACHMENT === request.cookie)) {
    throw new Error("attachment token replayed before the ordered notification barrier")
  }
  return matches[0]
}

function inspectUntilAborted(inspect, signal) {
  signal.throwIfAborted()
  return new Promise((resolve, reject) => {
    const onAbort = () => {
      signal.removeEventListener("abort", onAbort)
      reject(signal.reason)
    }
    signal.addEventListener("abort", onAbort, { once: true })
    Promise.resolve().then(() => {
      signal.throwIfAborted()
      return inspect(signal)
    }).then((value) => {
      signal.removeEventListener("abort", onAbort)
      resolve(value)
    }, (error) => {
      signal.removeEventListener("abort", onAbort)
      reject(error)
    })
  })
}

export async function waitForReadiness(bootstrap, inspect, {
  timeout = 20_000, interval = 50, signal, retryError = () => false,
} = {}) {
  const cancellation = new AbortController()
  let lastError
  const deadline = performance.now() + timeout
  const timedOut = () => cancellation.abort(new Error(`readiness timed out${lastError ? `: ${lastError.message}` : ""}`))
  const onError = (error) => cancellation.abort(new Error(`bootstrap failed: ${error.message}`, { cause: error }))
  const onExit = (code, exitedSignal) => {
    if (exitedSignal || code !== 0) {
      cancellation.abort(new Error(`bootstrap exited with ${exitedSignal ?? code} before readiness`))
    }
  }
  const onCancel = () => cancellation.abort(signal.reason)
  const check = () => {
    if (bootstrap && (bootstrap.exitCode !== null || bootstrap.signalCode)) onExit(bootstrap.exitCode, bootstrap.signalCode)
    if (performance.now() >= deadline) timedOut()
    cancellation.signal.throwIfAborted()
  }
  bootstrap?.on("error", onError)
  bootstrap?.on("exit", onExit)
  signal?.addEventListener("abort", onCancel, { once: true })
  if (signal?.aborted) onCancel()
  const timer = setTimeout(timedOut, timeout)
  try {
    for (;;) {
      check()
      let ready
      try {
        ready = await inspectUntilAborted(inspect, cancellation.signal)
        lastError = undefined
      } catch (error) {
        cancellation.signal.throwIfAborted()
        if (!retryError(error)) throw error
        lastError = error
      }
      check()
      if (ready) return ready
      await delay(Math.min(interval, Math.max(0, deadline - performance.now())), undefined, { signal: cancellation.signal })
    }
  } catch (error) {
    cancellation.signal.throwIfAborted()
    throw error
  } finally {
    clearTimeout(timer)
    bootstrap?.off("error", onError)
    bootstrap?.off("exit", onExit)
    signal?.removeEventListener("abort", onCancel)
  }
}
