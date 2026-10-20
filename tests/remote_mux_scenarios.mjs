// Shared ownership/lifecycle controls for isolated mux acceptance fixtures.
import assert from "node:assert/strict"

export function createPublicationCoordinator(marker, { limit = 16 } = {}) {
  assert.match(marker, /^[a-f0-9]{24}$/, "invalid publication owner")
  assert.ok(Number.isSafeInteger(limit) && limit >= 1 && limit <= 64, "invalid publication queue bound")
  const sequences = new Map()
  let tail = Promise.resolve(), failure, pending = 0
  return {
    async submit(request, deliver) {
      assert.equal(request?.probe_version, 1, "unsupported publication version")
      assert.equal(request.marker, marker, "foreign publication marker")
      assert.match(request.generation, /^[a-f0-9]{24}$/, "invalid publication generation")
      assert.ok(typeof request.cookie === "string" && request.cookie.length > 0 && request.cookie.length <= 1024, "invalid publication cookie")
      assert.ok(["publish", "barrier"].includes(request.action), "invalid publication action")
      assert.equal(typeof deliver, "function", "missing publication delivery")
      if (failure) throw failure
      assert.ok(pending < limit, "publication queue is full")
      const captured = { probe_version: 1, action: request.action, marker, generation: request.generation, cookie: request.cookie }
      pending++
      const operation = tail.then(async () => {
        try {
          if (failure) throw failure
          const sequence = (sequences.get(captured.cookie) ?? 0) + 1
          assert.ok(Number.isSafeInteger(sequence), "publication sequence exhausted")
          sequences.set(captured.cookie, sequence)
          // Delivery owns the slot until the canonical current-client gate,
          // not merely until the request file or keeper-side ack is written.
          return await deliver({ ...captured, sequence })
        } catch (error) {
          failure ??= error
          throw error
        } finally { pending-- }
      })
      tail = operation.catch(() => {})
      return operation
    },
  }
}

export function requireRunningChannel(child, label) {
  assert.ok(child && Number.isSafeInteger(child.pid) && child.pid > 1 && child.exitCode === null && !child.signalCode,
    `${label} channel is not running; disconnect ordering was not exercised`)
  return { pid: child.pid, exit_code: child.exitCode, signal: child.signalCode }
}

export async function runMuxLifecycle({ initial, startup, attach, recover, verify, stopClient, stopStartup, interruptProxy, observe }) {
  const running = (phase, child) => observe(phase, requireRunningChannel(child, phase))
  running("startup-before-client-disconnect", startup)
  running("initial-client", initial.child)
  await stopClient(initial, "SIGTERM")
  running("startup-after-client-disconnect", startup)
  await verify("client-disconnected")

  const first = await attach("startup-first")
  await recover(first)
  running("startup-before-startup-disconnect", startup)
  running("client-before-startup-disconnect", first.child)
  await stopStartup()
  running("client-after-startup-disconnect", first.child)
  await verify("startup-disconnected")
  await recover(first)

  const second = await attach("simultaneous")
  await Promise.all([recover(first), recover(second)])
  running("simultaneous-first", first.child)
  running("simultaneous-second", second.child)
  await stopClient(first, "SIGKILL")
  running("client-after-peer-disconnect", second.child)
  await verify("one-client-disconnected")
  await recover(second)

  let transportInterruption = { status: "skipped", reason: "no owned transport interruption control" }
  if (interruptProxy) {
    await interruptProxy(second)
    running("client-after-proxy-interrupt", second.child)
    await verify("proxy-interrupted")
    transportInterruption = { status: "passed" }
  }
  await stopClient(second, "SIGTERM")
  await verify("all-clients-disconnected")
  const final = await attach("fresh-recovery")
  await recover(final)
  await verify("fresh-recovery")
  return {
    connection: final,
    coverage: { transport_interruption: transportInterruption },
    summary: "both disconnect orders, simultaneous clients, abrupt client termination, and original-process recovery passed; transport interruption "
      + (transportInterruption.status === "passed" ? "passed" : `skipped (${transportInterruption.reason})`),
  }
}
