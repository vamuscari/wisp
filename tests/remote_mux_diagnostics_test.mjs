import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { once } from "node:events"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { createServer } from "node:net"
import { tmpdir } from "node:os"
import { isAbsolute, join } from "node:path"
import test from "node:test"
import { renderMuxClient } from "./remote_mux_client.mjs"
import { withProbeCleanup } from "./remote_mux_lifecycle.mjs"
import { createMuxLogCapture, muxDiagnosticEnv, muxSocketPath, runProbeCommand, waitForReadiness } from "./remote_mux_support.mjs"

test("native libssh callbacks reach the bounded capture before authentication", {
  skip: process.platform === "win32" || !(process.env.WISP_TEST_WEZTERM || process.env.WISP_TEST_MUX_SERVER)
    ? "set WISP_TEST_WEZTERM and WISP_TEST_MUX_SERVER for an owned POSIX loopback control" : false,
  timeout: 20_000,
}, async (t) => {
  const wezterm = process.env.WISP_TEST_WEZTERM, mux = process.env.WISP_TEST_MUX_SERVER
  assert.ok(wezterm && isAbsolute(wezterm), "set an absolute WISP_TEST_WEZTERM")
  assert.ok(mux && isAbsolute(mux), "set an absolute WISP_TEST_MUX_SERVER")
  const parent = process.env.WISP_TEST_TMPDIR ?? tmpdir()
  muxSocketPath(join(parent, "w-XXXXXX"))
  const root = await mkdtemp(join(parent, "w-"))
  const socket = muxSocketPath(root), config = join(root, "client.lua")
  const env = muxDiagnosticEnv(Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("WEZTERM_"))))
  Object.assign(env, {
    HOME: join(root, "home"), USERPROFILE: join(root, "home"), WEZTERM_UNIX_SOCKET: socket,
    XDG_CONFIG_HOME: join(root, "config"), XDG_CACHE_HOME: join(root, "cache"),
    XDG_DATA_HOME: join(root, "data"), XDG_RUNTIME_DIR: root,
  })
  const connections = new Set(), stages = []
  let hello = "", child, closed
  const output = {}
  // This is only a TCP banner endpoint: it has no host keys, user accounts,
  // authentication, shell, proxy, or mux protocol implementation.
  const endpoint = createServer((connection) => {
    connections.add(connection)
    connection.on("close", () => connections.delete(connection))
    connection.on("error", () => {})
    connection.write("SSH-2.0-WispDiagnosticFixture\r\n")
    connection.on("data", (bytes) => {
      hello = (hello + bytes.toString()).slice(0, 256)
      if (hello.includes("\n")) connection.end()
    })
  })
  try {
    await withProbeCleanup(async () => {
      for (const path of [env.HOME, env.XDG_CONFIG_HOME, env.XDG_CACHE_HOME, env.XDG_DATA_HOME]) await mkdir(path)
      const version = async (executable) => (await runProbeCommand(executable, ["--version"], { env, input: "" })).stdout.trim().split(/\s+/).at(-1)
      assert.equal(await version(wezterm), await version(mux), "local CLI/mux versions match")
      endpoint.listen(0, "127.0.0.1")
      await once(endpoint, "listening")
      await writeFile(config, renderMuxClient({
        host: "127.0.0.1", sshPort: endpoint.address().port, remoteRoot: "C:/unused-fixture",
        socket, metadata: join(root, "state.json"), marker: "loopback-diagnostics", generation: "connection-1",
      }))
      child = spawn(mux, ["--config-file", config], { env, cwd: root, stdio: ["ignore", "pipe", "pipe"] })
      closed = new Promise((resolve) => child.once("close", resolve))
      child.on("error", () => {})
      for (const name of ["stdout", "stderr"]) {
        const capture = createMuxLogCapture((stage) => stages.push(stage))
        output[name] = capture
        child[name].on("data", (bytes) => capture.write(bytes))
        child[name].on("end", () => capture.end())
      }
      await waitForReadiness(child, () => hello.startsWith("SSH-2.0-")
        && stages.includes("libssh-callback-observed") && stages.includes("fixture-banner-observed"), { timeout: 5000, signal: t.signal })
      assert.ok(stages.includes("ssh-address-resolved"), "capture must distinguish resolution from socket connection")
      for (const stage of ["ssh-authenticated", "proxy-exec-requested", "codec-verified"]) {
        assert.equal(stages.includes(stage), false, "a banner trace must not imply authentication, proxy execution, or codec verification")
      }
      t.diagnostic("native libssh callback and server banner captured; no authentication or external SSH host used")
    }, [
      ["stop owned mux", async () => {
        if (!child) return
        if (child.exitCode === null) child.kill()
        const timer = setTimeout(() => child.kill("SIGKILL"), 2000)
        try { await closed } finally { clearTimeout(timer) }
      }],
      ["close owned loopback endpoint", async () => {
        for (const connection of connections) connection.destroy()
        if (endpoint.listening) await new Promise((resolve, reject) => endpoint.close((error) => error ? reject(error) : resolve()))
      }],
      ["remove owned files", () => rm(root, { recursive: true, force: true })],
    ])
  } catch (error) {
    throw new Error(`${error.message}\n${Object.values(output).map((capture) => capture.tail).join("\n")}`, { cause: error })
  }
})
