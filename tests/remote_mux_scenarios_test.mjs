import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { once } from "node:events"
import test from "node:test"
import * as scenarios from "./remote_mux_scenarios.mjs"

const marker = "a".repeat(24), generation = "b".repeat(24)
const request = { probe_version: 1, action: "publish", marker, generation, cookie: marker + ":42:1" }

test("publication submission holds the single slot until the current client acknowledges", async () => {
  assert.equal(typeof scenarios.createPublicationCoordinator, "function")
  const coordinator = scenarios.createPublicationCoordinator(marker)
  let release, started
  const gate = new Promise((resolve) => { release = resolve })
  const began = new Promise((resolve) => { started = resolve })
  const sent = []
  const first = coordinator.submit(request, async (publication) => {
    sent.push(publication); started(); await gate; return publication
  })
  const secondRequest = { ...request, generation: "c".repeat(24), PRIVATE_FIXTURE: "not-recorded" }
  const second = coordinator.submit(secondRequest, async (publication) => { sent.push(publication); return publication })
  // Queued identity is captured on submission, not read from a mutable caller.
  secondRequest.generation = "d".repeat(24)
  await began
  assert.equal(sent.length, 1)
  release()
  assert.equal((await first).sequence, 1)
  assert.equal((await second).sequence, 2)
  assert.equal(sent[1].generation, "c".repeat(24))
  assert.equal(sent[1].PRIVATE_FIXTURE, undefined)
  assert.equal((await coordinator.submit({ ...request, cookie: marker + ":43:1" }, async (value) => value)).sequence, 1)
  assert.equal((await coordinator.submit({ ...request, action: "barrier" }, async (value) => value)).sequence, 3)
})

test("publication queues are bounded and a failed association prevents later writes", async () => {
  assert.equal(typeof scenarios.createPublicationCoordinator, "function")
  const coordinator = scenarios.createPublicationCoordinator(marker, { limit: 2 })
  let release
  const gate = new Promise((resolve) => { release = resolve })
  const failure = new Error("canonical client acknowledgement missing")
  let writes = 0
  const first = coordinator.submit(request, async () => { writes++; await gate; throw failure })
  const second = coordinator.submit(request, async () => { writes++; return "must not publish" })
  await assert.rejects(coordinator.submit(request, async () => { writes++ }), /queue.*full/)
  const stopped = Promise.allSettled([first, second])
  release()
  for (const result of await stopped) { assert.equal(result.status, "rejected"); assert.equal(result.reason, failure) }
  await assert.rejects(coordinator.submit(request, async () => { writes++ }), (error) => error === failure)
  assert.equal(writes, 1)
})

test("publication identity and versions are validated before reserving a sequence", async () => {
  assert.equal(typeof scenarios.createPublicationCoordinator, "function")
  const coordinator = scenarios.createPublicationCoordinator(marker)
  const invalid = { probe_version: 2 }
  Object.defineProperty(invalid, "marker", { get() { throw new Error("interpreted incompatible request") } })
  await assert.rejects(coordinator.submit(invalid, async () => {}), /version/)
  for (const update of [{ marker: "e".repeat(24) }, { generation: "invalid" }, { cookie: "" }, { action: "unknown" }]) {
    await assert.rejects(coordinator.submit({ ...request, ...update }, async () => { throw new Error("must not deliver") }))
  }
  assert.equal((await coordinator.submit(request, async (value) => value)).sequence, 1)
})

test("disconnect ordering requires an actually running channel, not an exited launcher", async () => {
  assert.equal(typeof scenarios.requireRunningChannel, "function")
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" })
  const closed = once(child, "close")
  try {
    await once(child, "spawn")
    assert.equal(scenarios.requireRunningChannel(child, "startup").pid, child.pid)
    child.kill("SIGTERM")
    await closed
    assert.throws(() => scenarios.requireRunningChannel(child, "startup"), /startup.*not running/)
    assert.throws(() => scenarios.requireRunningChannel(undefined, "startup"), /startup.*not running/)
  } finally {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL")
    await closed
  }
})

function lifecycleInputs(update = {}) {
  const events = [], channel = (pid) => ({ pid, exitCode: null, signalCode: null })
  const startup = channel(100), initial = { child: channel(101), generation: "initial" }
  let next = 101
  const callbacks = {
    initial, startup,
    attach: async (phase) => { events.push(`attach:${phase}`); return { child: channel(++next), generation: phase } },
    recover: async (connection) => { events.push(`recover:${connection.generation}`) },
    verify: async (phase) => { events.push(`verify:${phase}`) },
    stopClient: async (connection, signal) => { events.push(`stop:${connection.generation}:${signal}`); connection.child.signalCode = signal },
    stopStartup: async () => { events.push("stop:startup"); startup.signalCode = "SIGTERM" },
    interruptProxy: async (connection) => { events.push(`interrupt:${connection.generation}`) },
    observe: (phase, value) => { assert.ok(value.pid > 1); events.push(`running:${phase}`) },
    ...update,
  }
  return { callbacks, events }
}

test("lifecycle acceptance exercises both real disconnect orders and independently attached clients", async () => {
  assert.equal(typeof scenarios.runMuxLifecycle, "function")
  const { callbacks, events } = lifecycleInputs()
  const result = await scenarios.runMuxLifecycle(callbacks)
  assert.deepEqual(result.coverage, { transport_interruption: { status: "passed" } })
  assert.equal(result.connection.generation, "fresh-recovery")
  assert.match(result.summary, /transport interruption passed/)
  assert.doesNotMatch(result.summary, /skipped/)
  assert.deepEqual(events.filter((event) => !event.startsWith("running:")), [
    "stop:initial:SIGTERM", "verify:client-disconnected", "attach:startup-first", "recover:startup-first",
    "stop:startup", "verify:startup-disconnected", "recover:startup-first", "attach:simultaneous",
    "recover:startup-first", "recover:simultaneous", "stop:startup-first:SIGKILL", "verify:one-client-disconnected",
    "recover:simultaneous", "interrupt:simultaneous", "verify:proxy-interrupted", "stop:simultaneous:SIGTERM",
    "verify:all-clients-disconnected", "attach:fresh-recovery", "recover:fresh-recovery", "verify:fresh-recovery",
  ])
  assert.ok(events.indexOf("running:startup-after-client-disconnect") > events.indexOf("stop:initial:SIGTERM"))
  assert.ok(events.indexOf("running:client-after-startup-disconnect") > events.indexOf("stop:startup"))
})

test("lifecycle acceptance records unavailable transport interruption as skipped", async () => {
  const { callbacks, events } = lifecycleInputs({ interruptProxy: undefined })
  const result = await scenarios.runMuxLifecycle(callbacks)
  assert.deepEqual(result.coverage, { transport_interruption: {
    status: "skipped", reason: "no owned transport interruption control",
  } })
  assert.equal(result.connection.generation, "fresh-recovery")
  assert.equal(events.includes("verify:proxy-interrupted"), false)
  assert.equal(events.some((event) => event.startsWith("interrupt:")), false)
  assert.match(result.summary, /transport interruption skipped \(no owned transport interruption control\)/)
  assert.doesNotMatch(result.summary, /transport interruption passed/)
})

test("lifecycle failure stops at its first boundary without another attachment attempt", async () => {
  assert.equal(typeof scenarios.runMuxLifecycle, "function")
  const failure = new Error("original process changed")
  const { callbacks, events } = lifecycleInputs({ verify: async () => { throw failure } })
  await assert.rejects(scenarios.runMuxLifecycle(callbacks), (error) => error === failure)
  assert.deepEqual(events.filter((event) => !event.startsWith("running:")), ["stop:initial:SIGTERM"])
  const exited = lifecycleInputs()
  exited.callbacks.startup.exitCode = 0
  await assert.rejects(scenarios.runMuxLifecycle(exited.callbacks), /startup.*not running/)
  assert.deepEqual(exited.events, [])
})
