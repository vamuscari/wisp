import assert from "node:assert/strict"
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import test from "node:test"
import { fileURLToPath, pathToFileURL } from "node:url"

const canonicalPlugin = fileURLToPath(new URL("../opencode/wisp.js", import.meta.url))

function session(id, parentID) {
  return {
    id,
    projectID: "project",
    directory: "/repos/wisp",
    parentID,
    title: id,
    version: "1.18.15",
    time: { created: 1, updated: 1 },
  }
}

function permission(id, sessionID) {
  return {
    id,
    sessionID,
    permission: "external_directory",
    patterns: ["/opt/homebrew/lib/node_modules/*"],
    metadata: {},
    always: ["/opt/homebrew/lib/node_modules/*"],
    tool: { messageID: `msg_${id}`, callID: `call_${id}` },
  }
}

function question(id, sessionID) {
  return {
    id,
    sessionID,
    questions: [
      {
        question: "Continue?",
        header: "Action",
        options: [{ label: "Yes", description: "Continue the task." }],
        multiple: false,
      },
    ],
    tool: { messageID: `msg_${id}`, callID: `call_${id}` },
  }
}

function option(args, name) {
  const index = args.indexOf(name)
  return index === -1 ? undefined : args[index + 1]
}

function deferred() {
  let resolve
  const promise = new Promise((done) => {
    resolve = done
  })
  return { promise, resolve }
}

async function registrations(log) {
  const contents = await readFile(log, "utf8")
  return contents
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line))
    .filter((args) => args[0] === "opencode" && args[1] === "register")
}

async function fixture() {
  const root = await mkdtemp(path.join(tmpdir(), "wisp-opencode-plugin-"))
  const pluginDirectory = path.join(root, "opencode")
  const binDirectory = path.join(root, "bin")
  const pluginPath = path.join(pluginDirectory, "wisp.js")
  const executable = path.join(binDirectory, "wisp")
  const log = path.join(root, "registrations.jsonl")
  await mkdir(pluginDirectory)
  await mkdir(binDirectory)
  await writeFile(path.join(root, "package.json"), '{"type":"module"}\n')
  await copyFile(canonicalPlugin, pluginPath)
  await writeFile(
    executable,
    `#!/usr/bin/env node
import { appendFileSync } from "node:fs"
appendFileSync(process.env.WISP_PLUGIN_TEST_LOG, JSON.stringify(process.argv.slice(2)) + "\\n")
`,
  )
  await chmod(executable, 0o755)
  return { root, pluginPath, log }
}

async function withPlugin(run, { sessionID = "ses_root" } = {}) {
  const item = await fixture()
  const originalArgv = process.argv
  const originalLog = process.env.WISP_PLUGIN_TEST_LOG
  const originalSetInterval = globalThis.setInterval
  const originalClearInterval = globalThis.clearInterval
  const state = {
    children: new Map(),
    permissionResponse: undefined,
    permissions: [],
    questions: [],
    statuses: { ses_root: { type: "busy" } },
  }
  let heartbeat
  let hooks
  process.argv = [process.execPath, item.pluginPath]
  if (sessionID) process.argv.push("--session", sessionID)
  process.env.WISP_PLUGIN_TEST_LOG = item.log
  globalThis.setInterval = (callback, delay) => {
    assert.equal(delay, 30_000)
    heartbeat = callback
    return { unref() {} }
  }
  globalThis.clearInterval = () => {}

  try {
    const plugin = await import(pathToFileURL(item.pluginPath).href)
    hooks = await plugin.default({
      client: {
        _client: {
          get: async ({ url }) => {
            if (url === "/global/health") return { data: { healthy: true, version: "1.18.15" } }
            if (url === "/permission") {
              if (state.permissionResponse) return state.permissionResponse()
              return { data: state.permissions }
            }
            if (url === "/question") return { data: state.questions }
            throw new Error(`unexpected request ${url}`)
          },
        },
        session: {
          children: async ({ path: requestPath }) => ({ data: state.children.get(requestPath.id) ?? [] }),
          status: async () => ({ data: state.statuses }),
        },
      },
      serverUrl: new URL("http://localhost:4096"),
      directory: "/repos/wisp",
      worktree: "/repos/wisp",
    })
    await run({
      hooks,
      state,
      heartbeat: async () => heartbeat(),
      latestRegistration: async () => (await registrations(item.log)).at(-1),
    })
  } finally {
    await hooks?.dispose()
    process.argv = originalArgv
    if (originalLog === undefined) delete process.env.WISP_PLUGIN_TEST_LOG
    else process.env.WISP_PLUGIN_TEST_LOG = originalLog
    globalThis.setInterval = originalSetInterval
    globalThis.clearInterval = originalClearInterval
    await rm(item.root, { recursive: true, force: true })
  }
}

test("a child permission marks its selected root launch as waiting", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ hooks, latestRegistration }) => {
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_root") },
      },
    })
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_child", "ses_root") },
      },
    })
    await hooks.event({
      event: {
        type: "permission.asked",
        properties: permission("per_child", "ses_child"),
      },
    })

    const latest = await latestRegistration()
    assert.equal(option(latest, "--session-id"), "ses_root")
    assert.equal(option(latest, "--waiting-permissions"), "1")
  }, { sessionID: undefined })
})

test("root idle does not clear a pending child permission", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ hooks, latestRegistration }) => {
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_child", "ses_root") },
      },
    })
    await hooks.event({
      event: {
        type: "permission.asked",
        properties: permission("per_child", "ses_child"),
      },
    })
    await hooks.event({
      event: {
        type: "session.status",
        properties: { sessionID: "ses_root", status: { type: "idle" } },
      },
    })

    let latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-permissions"), "1")
    await hooks.event({
      event: {
        type: "permission.replied",
        properties: { sessionID: "ses_child", requestID: "per_child", reply: "once" },
      },
    })

    latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-permissions"), "0")
  })
})

test("a nested child question is cleared when it is answered", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ hooks, latestRegistration }) => {
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_child", "ses_root") },
      },
    })
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_grandchild", "ses_child") },
      },
    })
    await hooks.event({
      event: {
        type: "question.asked",
        properties: question("que_grandchild", "ses_grandchild"),
      },
    })

    let latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-questions"), "1")
    await hooks.event({
      event: {
        type: "question.replied",
        properties: {
          sessionID: "ses_grandchild",
          requestID: "que_grandchild",
          answers: [["Yes"]],
        },
      },
    })

    latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-questions"), "0")
  })
})

test("a heartbeat recovers only pending permissions owned by the selected root", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ heartbeat, latestRegistration, state }) => {
    state.children.set("ses_root", [session("ses_child", "ses_root")])
    state.children.set("ses_child", [])
    state.permissions = [permission("per_child", "ses_child"), permission("per_other", "ses_other")]

    await heartbeat()

    let latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-permissions"), "1")
    state.permissions = []
    await heartbeat()

    latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-permissions"), "0")
  })
})

test("a heartbeat recovers only pending questions owned by the selected root", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ heartbeat, latestRegistration, state }) => {
    state.children.set("ses_root", [session("ses_child", "ses_root")])
    state.children.set("ses_child", [])
    state.questions = [question("que_child", "ses_child"), question("que_other", "ses_other")]

    await heartbeat()

    let latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-questions"), "1")
    state.questions = []
    await heartbeat()

    latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-questions"), "0")
  })
})

test("a stale heartbeat cannot resurrect an answered child permission", { skip: process.platform === "win32" }, async () => {
  await withPlugin(async ({ heartbeat, hooks, latestRegistration, state }) => {
    state.children.set("ses_root", [session("ses_child", "ses_root")])
    state.children.set("ses_child", [])
    await hooks.event({
      event: {
        type: "session.created",
        properties: { info: session("ses_child", "ses_root") },
      },
    })
    await hooks.event({
      event: {
        type: "permission.asked",
        properties: permission("per_child", "ses_child"),
      },
    })

    const requested = deferred()
    const response = deferred()
    state.permissionResponse = () => {
      requested.resolve()
      return response.promise
    }
    const refresh = heartbeat()
    await requested.promise
    await hooks.event({
      event: {
        type: "permission.replied",
        properties: { sessionID: "ses_child", requestID: "per_child", reply: "once" },
      },
    })
    response.resolve({ data: [permission("per_child", "ses_child")] })
    await refresh

    const latest = await latestRegistration()
    assert.equal(option(latest, "--waiting-permissions"), "0")
  })
})
