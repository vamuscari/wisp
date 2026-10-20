import assert from "node:assert/strict"
import { execFile, spawn } from "node:child_process"
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { isAbsolute, join, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
import { fileURLToPath } from "node:url"
import { promisify } from "node:util"
import test from "node:test"
import { selectMuxProfiles } from "./remote_mux_profiles.mjs"
import { muxSocketPath } from "./remote_mux_support.mjs"

const exec = promisify(execFile)
const repository = fileURLToPath(new URL("../", import.meta.url))
const fixtures = join(repository, "tests", "fixtures", "remote-mux")
const optedIn = Boolean(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
const probeArgs = ["space value", 'double"quote', "single'quote", "snow-\u96ea", "semi;&|<>", "$literal", "--flag=value"]

async function waitFor(description, inspect, serverLog) {
  const deadline = Date.now() + 20_000
  let lastError
  while (Date.now() < deadline) {
    try {
      const result = await inspect()
      if (result) return result
    } catch (error) {
      lastError = error
    }
    await delay(100)
  }
  throw new Error(`Timed out waiting for ${description}: ${lastError ?? "not ready"}\n${serverLog()}`)
}

function nativeKey(path) {
  const normalized = resolve(path)
  return process.platform === "win32" ? normalized.toLowerCase() : normalized
}

test("isolated headless mux shell feasibility", {
  skip: optedIn ? false : "set WISP_TEST_WEZTERM and WISP_TEST_MUX_SERVER to isolated-test binaries",
  timeout: 180_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM
  const muxServer = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm), "WISP_TEST_WEZTERM must be an absolute executable path")
  assert.ok(muxServer && isAbsolute(muxServer), "WISP_TEST_MUX_SERVER must be an absolute executable path")

  const profiles = selectMuxProfiles(process.platform, process.env)

  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX")) // Reject an overlong parent before creating files.
  const root = await mkdtemp(join(parent, "w-"))
  const cwd = join(root, "work area \u96ea")
  const splitCwd = join(cwd, "split directory")
  const socket = muxSocketPath(root)
  const configPath = join(root, "wezterm.lua")
  const stopPath = join(root, "stop")
  const marker = `isolated-${root.split(/[\\/]/).at(-1)}`
  const env = { ...process.env }
  for (const name of Object.keys(env)) {
    if (name.startsWith("WEZTERM_")) delete env[name]
  }
  Object.assign(env, {
    WEZTERM_UNIX_SOCKET: socket,
    HOME: join(root, "home"),
    USERPROFILE: join(root, "home"),
    XDG_CONFIG_HOME: join(root, "config"),
    XDG_CACHE_HOME: join(root, "cache"),
    XDG_DATA_HOME: join(root, "data"),
    XDG_RUNTIME_DIR: root,
    WISP_MUX_PROBE_MARKER: marker,
    CHERE_INVOKING: "1",
  })
  await Promise.all([cwd, splitCwd, env.HOME, env.XDG_CONFIG_HOME, env.XDG_CACHE_HOME, env.XDG_DATA_HOME]
    .map((directory) => mkdir(directory, { recursive: true })))
  // An explicit socket plus --no-auto-start prevents falling back to a live GUI
  // or starting a second mux. This server is always a foreground child we own.
  await writeFile(configPath, `
local config = assert(loadfile(${JSON.stringify(join(repository, "examples", "remote", "wezterm-server.lua"))}))()
config.unix_domains = {{ name = "wisp-mux-test", socket_path = ${JSON.stringify(socket)}, no_serve_automatically = true }}
config.default_domain = "local"
config.exit_behavior = "Hold"
config.set_environment_variables = {
  WISP_MUX_PROBE_MARKER = ${JSON.stringify(marker)}, CHERE_INVOKING = "1",
  HOME = ${JSON.stringify(env.HOME)}, USERPROFILE = ${JSON.stringify(env.USERPROFILE)},
  XDG_CONFIG_HOME = ${JSON.stringify(env.XDG_CONFIG_HOME)}, XDG_CACHE_HOME = ${JSON.stringify(env.XDG_CACHE_HOME)},
  XDG_DATA_HOME = ${JSON.stringify(env.XDG_DATA_HOME)}, XDG_RUNTIME_DIR = ${JSON.stringify(root)},
}
return config
`)

  const options = { env, cwd: root, timeout: 5_000, maxBuffer: 1024 * 1024, windowsHide: true }
  let server
  let serverClosed
  let logs = ""
  const serverLog = () => logs
  const cli = async (...args) => (await exec(wezterm, [
    "--config-file", configPath, "cli", "--prefer-mux", "--no-auto-start", ...args,
  ], options)).stdout.trim()
  const list = async () => JSON.parse(await cli("list", "--format", "json"))
  const probePath = join(fixtures, "probe.mjs")
  const powerShellProbe = await readFile(join(fixtures, "probe.ps1"), "utf8")
  const keeperOutput = join(root, "keeper.json")
  const panes = new Set()

  try {
    const cliVersion = (await exec(wezterm, ["--version"], options)).stdout.trim()
    const muxVersion = (await exec(muxServer, ["--version"], options)).stdout.trim()
    const version = cliVersion.split(/\s+/).at(-1)
    assert.equal(muxVersion.split(/\s+/).at(-1), version, "CLI and server versions must match")
    if (process.env.WISP_TEST_WEZTERM_VERSION) {
      assert.equal(version, process.env.WISP_TEST_WEZTERM_VERSION, "expected WezTerm version")
    }
    t.diagnostic(`${cliVersion}; ${muxVersion}; ${process.platform}`)
    server = spawn(muxServer, [
      "--config-file", configPath, "--cwd", cwd, "--", process.execPath, probePath, "argv", keeperOutput, stopPath,
    ], { env, cwd: root, stdio: ["ignore", "pipe", "pipe"], windowsHide: true })
    serverClosed = new Promise((resolve) => server.once("close", resolve))
    server.on("error", (error) => { logs += String(error) })
    server.stdout.on("data", (data) => { logs = (logs + data).slice(-32_768) })
    server.stderr.on("data", (data) => { logs = (logs + data).slice(-32_768) })

    async function readReport(path) {
      return waitFor(path, async () => {
        const report = JSON.parse(await readFile(path, "utf8"))
        assert.equal(report.probe_version, 1, "probe report version")
        return report
      }, serverLog)
    }

    const keeper = await readReport(keeperOutput)
    assert.equal(keeper.marker, marker)
    panes.add(Number(keeper.pane_id))
    await waitFor("isolated mux listener", async () => (await list()).length > 0, serverLog)

    let nextOutput = 0
    function shellCommand(profile, output) {
      if (profile.native) return [process.execPath, probePath, "argv", output, stopPath, ...probeArgs]
      // Windows shell command-line decoders are not generic native argv parsers.
      // Keep arbitrary data in a versioned request, not in shell syntax.
      const request = Buffer.from(JSON.stringify({ probe_version: 1, output, stop: stopPath, args: probeArgs })).toString("base64")
      if (profile.bash) {
        // Git Bash accepts forward-slash Windows paths without shell-string quoting.
        const bashPath = (path) => path.replaceAll("\\", "/")
        return [profile.executable, "--login", bashPath(join(fixtures, "probe.sh")),
          bashPath(process.execPath), bashPath(probePath), request]
      }
      // A fixed inline command works without changing .ps1 execution policy.
      const command = `$encodedRequest = '${request}'\n${powerShellProbe}`
      return [profile.executable, "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand",
        Buffer.from(command, "utf16le").toString("base64")]
    }

    async function launch(profile, operation, targetCwd) {
      const output = join(root, `probe-${nextOutput++}.json`)
      const pane = Number(await cli(...operation, "--cwd", targetCwd, "--", ...shellCommand(profile, output)))
      assert.ok(Number.isSafeInteger(pane) && pane >= 0, "spawn must return a pane ID")
      panes.add(pane)
      let report
      try {
        report = await readReport(output)
      } catch (error) {
        const text = await cli("get-text", "--pane-id", String(pane)).catch((failure) => String(failure))
        throw new Error(`${error.message}\nProbe pane output: ${text}`, { cause: error })
      }
      assert.equal(report.marker, marker, "probe ran in the isolated server environment")
      assert.equal(nativeKey(report.home), nativeKey(env.HOME), "shell uses only the isolated home directory")
      assert.equal(report.argument_transport, profile.native ? "argv" : "structured", "probe data transport")
      assert.equal(Number(report.pane_id), pane, "server-side WEZTERM_PANE matches the returned ID")
      assert.ok(Number.isSafeInteger(report.shell_pid) && report.shell_pid > 0, "shell PID")
      assert.equal(nativeKey(report.cwd), nativeKey(targetCwd), "shell preserves the requested native cwd")
      assert.deepEqual(report.args, probeArgs, "probe receives literal argument data")
      const live = await waitFor("OSC 7 working directory", async () => {
        const entry = (await list()).find((candidate) => candidate.pane_id === pane)
        assert.equal(entry?.cwd, report.cwd_url, "reported OSC 7 URL")
        return entry
      }, serverLog)
      const text = await cli("get-text", "--pane-id", String(pane))
      assert.ok(text.replace(/[\r\n]/g, "").includes(`WISP_MUX_PROBE_READY ${marker}`),
        `pane output is readable across wrapped lines: ${JSON.stringify({ live, text })}`)
      return { live, report }
    }

    await t.test("native executable preserves the full argv before shell decoding", async () => {
      await launch({ native: true }, ["spawn", "--new-window", "--workspace", "probe-native-argv"], cwd)
    })

    for (const profile of profiles) {
      await t.test(profile.name, async (t) => {
        const first = await launch(profile, ["spawn", "--new-window", "--workspace", `probe-${nextOutput}`], cwd)
        let tab
        await t.test("new tabs stay in the same server window", async () => {
          tab = await launch(profile, ["spawn", "--pane-id", String(first.live.pane_id)], cwd)
          assert.equal(tab.live.window_id, first.live.window_id)
          assert.notEqual(tab.live.tab_id, first.live.tab_id)
        })
        await t.test("splits stay in the requested tab and use their own cwd", async () => {
          assert.ok(tab, "new tab must be available")
          const split = await launch(profile, ["split-pane", "--pane-id", String(tab.live.pane_id), "--right"], splitCwd)
          assert.equal(split.live.window_id, tab.live.window_id)
          assert.equal(split.live.tab_id, tab.live.tab_id)
          assert.notEqual(split.live.pane_id, tab.live.pane_id)
        })
        await t.test("a fresh CLI connection sees the existing shell", async () => {
          const entry = (await list()).find((candidate) => candidate.pane_id === first.live.pane_id)
          assert.ok(entry, "pane survives the previous CLI client's exit")
          assert.equal(entry.window_id, first.live.window_id)
          assert.equal(entry.tab_id, first.live.tab_id)
          process.kill(first.report.shell_pid, 0)
        })
      })
    }
  } finally {
    await writeFile(stopPath, "stop")
    // Only terminate processes on our unique socket, never panes from a live GUI.
    if (server && server.exitCode === null) {
      await Promise.allSettled([...panes].map((pane) => cli("kill-pane", "--pane-id", String(pane))))
      server.kill()
      await Promise.race([
        serverClosed,
        delay(5_000, undefined, { ref: false }),
      ])
    }
    await rm(root, { recursive: true, force: true })
  }
})
