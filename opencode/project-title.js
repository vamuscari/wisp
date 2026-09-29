import path from "node:path"

const DEFAULT_ROOT_TITLE = /^New session - \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/
const TITLE_SYSTEM_PREFIX = "You are a title generator. You output ONLY a thread title."

function validName(value) {
  if (typeof value !== "string") return
  const name = value.trim()
  if (!name || /[\u0000-\u001f\u007f]/.test(name)) return
  return name
}

function pathKey(value) {
  if (typeof value !== "string" || !value) return
  const resolved = path.resolve(value)
  return process.platform === "win32" ? resolved.toLowerCase() : resolved
}

function resolveProjectName(project, projectPath) {
  const wispName = validName(process.env.WISP_PROJECT_NAME)
  const wispPath = pathKey(process.env.WISP_PROJECT_DIR)
  if (wispName && wispPath && wispPath === pathKey(projectPath)) return wispName
  return validName(project?.name)
}

export default async function ProjectTitlePlugin({ client, project, directory, worktree }) {
  const projectPath = worktree === "/" ? directory : worktree
  const projectName = resolveProjectName(project, projectPath)
  if (!projectName) return {}

  const prefix = `[${projectName}] `
  const generationInstruction = `Prefix the title with the exact text ${JSON.stringify(prefix)}. The complete title, including this prefix, must be a single line of 50 characters or fewer.`
  const roots = new Set()
  const updating = new Set()

  async function prefixTitle(info) {
    if (
      !info
      || typeof info.id !== "string"
      || !info.id
      || typeof info.title !== "string"
      || DEFAULT_ROOT_TITLE.test(info.title)
      || info.title.startsWith(prefix)
      || updating.has(info.id)
    ) {
      return
    }

    updating.add(info.id)
    try {
      await client.session.update({
        path: { id: info.id },
        query: { directory: info.directory ?? directory },
        body: { title: `${prefix}${info.title}` },
      })
    } catch {
      // Session naming must never interrupt OpenCode or Wisp state handling.
    } finally {
      updating.delete(info.id)
    }
  }

  return {
    event: async ({ event }) => {
      const info = event.properties?.info
      if (event.type === "session.created") {
        if (typeof info?.id !== "string" || !info.id || info.parentID) return
        roots.add(info.id)
        await prefixTitle(info)
        return
      }
      if (event.type === "session.deleted") {
        if (typeof info?.id === "string") roots.delete(info.id)
        return
      }
      if (event.type === "session.updated" && typeof info?.id === "string" && info.id && !info.parentID) {
        roots.add(info.id)
        await prefixTitle(info)
      }
    },
    "experimental.chat.system.transform": async (input, output) => {
      if (!Array.isArray(output.system)) return
      if (!output.system.some((item) => typeof item === "string" && item.startsWith(TITLE_SYSTEM_PREFIX))) return
      if (!roots.has(input.sessionID)) {
        try {
          const { data: info } = await client.session.get({
            path: { id: input.sessionID },
            query: { directory },
          })
          if (info?.id !== input.sessionID || info.parentID) return
          roots.add(input.sessionID)
        } catch {
          return
        }
      }
      if (!output.system.includes(generationInstruction)) output.system.push(generationInstruction)
    },
  }
}
