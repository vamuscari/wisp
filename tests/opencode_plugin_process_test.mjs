import assert from "node:assert/strict"
import { access, copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import test from "node:test"
import { pathToFileURL } from "node:url"

const executableName = process.platform === "win32" ? "wisp.exe" : "wisp"
const builtExecutable = path.resolve("target", "debug", executableName)
const canonicalPlugin = path.resolve("opencode", "wisp.js")
const statusSequence = /^\x1b\]1337;SetUserVar=WISP_OPENCODE_STATUS=([A-Za-z0-9+/=]*)\x1b\\$/

function statusPayload(write) {
  const match = statusSequence.exec(write)
  assert.ok(match, `unexpected OSC sequence ${JSON.stringify(write)}`)
  return match[1] === "" ? undefined : Buffer.from(match[1], "base64").toString("utf8")
}

test("the canonical plugin registers through the bundled platform executable", async () => {
  await access(builtExecutable)
  const root = await mkdtemp(path.join(tmpdir(), "wisp-opencode-process-"))
  const pluginDirectory = path.join(root, "opencode")
  const binDirectory = path.join(root, "bin")
  const registryDirectory = path.join(root, "registry")
  const pluginPath = path.join(pluginDirectory, "wisp.js")
  await mkdir(pluginDirectory)
  await mkdir(binDirectory)
  await writeFile(path.join(root, "package.json"), '{"type":"module"}\n')
  await copyFile(canonicalPlugin, pluginPath)
  await copyFile(builtExecutable, path.join(binDirectory, executableName))

  const originalArgv = process.argv
  const originalRegistry = process.env.WISP_OPENCODE_REGISTRY_DIR
  const originalPane = process.env.WEZTERM_PANE
  const originalSetInterval = globalThis.setInterval
  const originalClearInterval = globalThis.clearInterval
  const originalStdoutWrite = process.stdout.write
  const directory = process.platform === "win32" ? "C:\\Repos\\wisp" : "/repos/wisp"
  const writes = []
  let hooks
  process.argv = [process.execPath, pluginPath, "--session", "ses_windows"]
  process.env.WISP_OPENCODE_REGISTRY_DIR = registryDirectory
  process.env.WEZTERM_PANE = "42"
  process.stdout.write = (chunk) => {
    writes.push(Buffer.isBuffer(chunk) ? chunk.toString("utf8") : String(chunk))
    return true
  }
  globalThis.setInterval = () => ({ unref() {} })
  globalThis.clearInterval = () => {}

  try {
    const plugin = await import(`${pathToFileURL(pluginPath).href}?process-test=${Date.now()}`)
    hooks = await plugin.default({
      client: {
        _client: {
          get: async ({ url }) => {
            assert.equal(url, "/global/health")
            return { data: { healthy: true, version: "1.18.31" } }
          },
        },
        session: {},
      },
      serverUrl: new URL("http://127.0.0.1:4096"),
      directory,
      worktree: directory,
    })

    const files = await readdir(registryDirectory)
    assert.equal(files.length, 1)
    const registration = JSON.parse(await readFile(path.join(registryDirectory, files[0]), "utf8"))
    assert.equal(registration.registry_version, 8)
    assert.equal(registration.directory, directory)
    assert.equal(registration.project_path, directory)
    assert.equal(registration.pane_id, "42")
    assert.equal(registration.session_id, "ses_windows")
    assert.equal(registration.session_activity, "idle")
    assert.match(statusPayload(writes[0]), /^idle:\d+$/)

    await hooks.dispose()
    hooks = undefined
    assert.equal(statusPayload(writes.at(-1)), undefined)
    assert.deepEqual(await readdir(registryDirectory), [])
  } finally {
    await hooks?.dispose()
    process.argv = originalArgv
    if (originalRegistry === undefined) delete process.env.WISP_OPENCODE_REGISTRY_DIR
    else process.env.WISP_OPENCODE_REGISTRY_DIR = originalRegistry
    if (originalPane === undefined) delete process.env.WEZTERM_PANE
    else process.env.WEZTERM_PANE = originalPane
    globalThis.setInterval = originalSetInterval
    globalThis.clearInterval = originalClearInterval
    process.stdout.write = originalStdoutWrite
    await rm(root, { recursive: true, force: true })
  }
})
