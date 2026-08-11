import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const executable = fileURLToPath(new URL(process.platform === "win32" ? "../bin/wisp.exe" : "../bin/wisp", import.meta.url))
const SUPPORTED_OPENCODE_VERSION = "1.18.15"

async function supportedServer(client) {
  try {
    const response = await client._client.get({ url: "/global/health" })
    if (response.error) return false
    const health = response.data
    if (!health || typeof health !== "object" || Array.isArray(health)) return false
    if (health.version !== SUPPORTED_OPENCODE_VERSION) return false
    if (Object.keys(health).some((field) => field !== "healthy" && field !== "version")) return false
    if (health.healthy !== true) return false
    return true
  } catch {
    return false
  }
}

function normalizedStatus(status) {
  if (!status || typeof status !== "object" || Array.isArray(status)) return
  if (status.type === "idle" || status.type === "busy") return { type: status.type }
  if (
    status.type === "retry"
    && Number.isSafeInteger(status.attempt)
    && status.attempt >= 0
    && typeof status.message === "string"
    && Number.isSafeInteger(status.next)
    && status.next >= 0
  ) {
    return {
      type: "retry",
      attempt: status.attempt,
      message: status.message,
      next: status.next,
    }
  }
}

function selectedSession(argv) {
  for (let index = 0; index < argv.length; index += 1) {
    if ((argv[index] === "--session" || argv[index] === "-s") && argv[index + 1]) return argv[index + 1]
    if (argv[index]?.startsWith("--session=")) return argv[index].slice("--session=".length)
  }
}

export default async function WispPlugin({ client, serverUrl, directory, worktree }) {
  if (!(await supportedServer(client))) return {}
  let sessionID = selectedSession(process.argv)
  let sessionStatus = sessionID ? { type: "idle" } : undefined
  let sessionError
  const permissions = new Set()
  const questions = new Set()
  const sessionParents = new Map()
  let revision = 0
  let refreshGeneration = 0
  const projectPath = worktree === "/" ? directory : worktree

  function run(args) {
    spawnSync(executable, args, {
      shell: false,
      stdio: "ignore",
      windowsHide: true,
    })
  }

  function register() {
    const args = [
      "opencode", "register",
      "--server-url", serverUrl.toString().replace(/\/$/, ""),
      "--directory", directory,
      "--project-path", projectPath,
      "--pid", String(process.pid),
    ]
    if (process.env.WEZTERM_PANE) args.push("--pane-id", process.env.WEZTERM_PANE)
    if (sessionID) {
      args.push("--session-id", sessionID)
      args.push("--session-status", JSON.stringify(sessionStatus ?? { type: "idle" }))
      args.push("--waiting-permissions", String(permissions.size))
      args.push("--waiting-questions", String(questions.size))
      if (sessionError) args.push("--session-error", sessionError)
    }
    run(args)
  }

  function unregister() {
    run(["opencode", "unregister", "--directory", directory, "--pid", String(process.pid)])
  }

  function selectSession(id) {
    sessionID = id
    sessionStatus = { type: "idle" }
    sessionError = undefined
    permissions.clear()
    questions.clear()
    sessionParents.clear()
    revision += 1
  }

  function clearSession() {
    sessionID = undefined
    sessionStatus = undefined
    sessionError = undefined
    permissions.clear()
    questions.clear()
    sessionParents.clear()
    revision += 1
  }

  function ownsSession(id, selected = sessionID, parents = sessionParents) {
    const visited = new Set()
    while (typeof id === "string" && id && !visited.has(id)) {
      if (id === selected) return true
      visited.add(id)
      id = parents.get(id)
    }
    return false
  }

  function applyStatus(status) {
    const next = normalizedStatus(status)
    if (!next) return false
    sessionStatus = next
    if (next.type === "busy" || next.type === "retry") sessionError = undefined
    revision += 1
    return true
  }

  async function refreshAndRegister() {
    const selected = sessionID
    if (selected) {
      const generation = ++refreshGeneration
      const startingRevision = revision
      let nextStatus
      let nextParents
      let nextPermissions
      let nextQuestions
      try {
        const response = await client.session.status()
        if (!response.error && response.data && typeof response.data === "object") {
          nextStatus = normalizedStatus(response.data[selected] ?? { type: "idle" })
        }
      } catch {
        // Event state remains authoritative until the next heartbeat.
      }
      try {
        const discoveredParents = new Map()
        const visited = new Set([selected])
        const pending = [selected]
        while (pending.length > 0) {
          const parentID = pending.shift()
          const response = await client.session.children({ path: { id: parentID } })
          if (response.error || !Array.isArray(response.data)) throw new Error("invalid session children")
          for (const info of response.data) {
            if (
              !info
              || typeof info !== "object"
              || typeof info.id !== "string"
              || !info.id
              || info.parentID !== parentID
            ) {
              throw new Error("invalid child session")
            }
            if (visited.has(info.id)) continue
            visited.add(info.id)
            discoveredParents.set(info.id, parentID)
            pending.push(info.id)
          }
        }

        const permissionResponse = await client._client.get({ url: "/permission", query: { directory } })
        if (permissionResponse.error || !Array.isArray(permissionResponse.data)) {
          throw new Error("invalid pending permissions")
        }
        const questionResponse = await client._client.get({ url: "/question", query: { directory } })
        if (questionResponse.error || !Array.isArray(questionResponse.data)) {
          throw new Error("invalid pending questions")
        }
        const discoveredPermissions = new Set()
        for (const request of permissionResponse.data) {
          if (
            request
            && typeof request === "object"
            && typeof request.id === "string"
            && request.id
            && ownsSession(request.sessionID, selected, discoveredParents)
          ) {
            discoveredPermissions.add(request.id)
          }
        }
        const discoveredQuestions = new Set()
        for (const request of questionResponse.data) {
          if (
            request
            && typeof request === "object"
            && typeof request.id === "string"
            && request.id
            && ownsSession(request.sessionID, selected, discoveredParents)
          ) {
            discoveredQuestions.add(request.id)
          }
        }
        nextParents = discoveredParents
        nextPermissions = discoveredPermissions
        nextQuestions = discoveredQuestions
      } catch {
        // Event state remains authoritative until the next heartbeat.
      }

      if (generation === refreshGeneration && revision === startingRevision && sessionID === selected) {
        if (nextStatus) applyStatus(nextStatus)
        if (nextParents && nextPermissions && nextQuestions) {
          sessionParents.clear()
          for (const [id, parentID] of nextParents) sessionParents.set(id, parentID)
          permissions.clear()
          for (const id of nextPermissions) permissions.add(id)
          questions.clear()
          for (const id of nextQuestions) questions.add(id)
          revision += 1
        }
      }
    }
    register()
  }

  register()
  const heartbeat = setInterval(refreshAndRegister, 30_000)
  heartbeat.unref?.()
  return {
    event: async ({ event }) => {
      if (event.type === "session.created") {
        const info = event.properties?.info
        if (typeof info?.id === "string" && info.id && typeof info.parentID === "string" && info.parentID) {
          if (sessionParents.get(info.id) !== info.parentID) {
            sessionParents.set(info.id, info.parentID)
            revision += 1
          }
        }
        if (!sessionID && info?.id && !info.parentID) {
          selectSession(info.id)
          register()
        }
        return
      }
      if (event.type === "session.deleted" && event.properties?.info?.id === sessionID) {
        clearSession()
        register()
        return
      }
      if (!sessionID && event.type === "session.status") {
        const activity = event.properties?.status?.type
        if (event.properties?.sessionID && (activity === "busy" || activity === "retry")) {
          selectSession(event.properties.sessionID)
          applyStatus(event.properties.status)
          register()
        }
        return
      }
      if (event.type === "session.error" && event.properties?.sessionID === sessionID) {
        const error = event.properties?.error
        const message = typeof error?.data?.message === "string" ? error.data.message : error?.name
        if (typeof message === "string" && message) {
          sessionError = message
          revision += 1
          register()
        }
        return
      }
      if (event.type === "session.status" && event.properties?.sessionID === sessionID) {
        if (applyStatus(event.properties.status)) {
          register()
        }
        return
      }
      if (event.type === "permission.asked" && ownsSession(event.properties?.sessionID)) {
        const id = event.properties?.id
        if (typeof id === "string" && id && !permissions.has(id)) {
          permissions.add(id)
          revision += 1
          register()
        }
        return
      }
      if (event.type === "permission.replied" && ownsSession(event.properties?.sessionID)) {
        if (permissions.delete(event.properties?.requestID)) {
          revision += 1
          register()
        }
        return
      }
      if (event.type === "question.asked" && ownsSession(event.properties?.sessionID)) {
        const id = event.properties?.id
        if (typeof id === "string" && id && !questions.has(id)) {
          questions.add(id)
          revision += 1
          register()
        }
        return
      }
      if (
        (event.type === "question.replied" || event.type === "question.rejected")
        && ownsSession(event.properties?.sessionID)
      ) {
        if (questions.delete(event.properties?.requestID)) {
          revision += 1
          register()
        }
      }
    },
    dispose: async () => {
      clearInterval(heartbeat)
      unregister()
    },
  }
}
