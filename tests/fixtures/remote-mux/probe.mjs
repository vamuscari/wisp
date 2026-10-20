import { writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { sep } from "node:path"
import { pathToFileURL } from "node:url"

const [mode, ...input] = process.argv.slice(2)
let outputPath, stopPath, args
if (mode === "argv") {
  [outputPath, stopPath, ...args] = input
} else if (mode === "request") {
  const request = JSON.parse(Buffer.from(input[0], "base64").toString("utf8"))
  if (request.probe_version !== 1) throw new Error("unsupported probe request version")
  outputPath = request.output
  stopPath = request.stop
  args = request.args
} else {
  throw new Error("usage: probe.mjs argv OUTPUT STOP [ARGS...] | request BASE64_JSON")
}
if (!outputPath || !stopPath) throw new Error("usage: probe.mjs OUTPUT STOP [ARGS...]")

const cwdURL = pathToFileURL(process.cwd() + sep).href
const report = {
  probe_version: 1,
  argument_transport: mode === "argv" ? "argv" : "structured",
  // Node reports the native parent PID; Git Bash's $$ can be an MSYS PID.
  shell_pid: process.env.WISP_MUX_BASH_PROBE === "1" ? process.ppid : process.pid,
  cwd: process.cwd(),
  cwd_url: cwdURL,
  home: process.env.HOME,
  args,
  pane_id: process.env.WEZTERM_PANE,
  marker: process.env.WISP_MUX_PROBE_MARKER,
}
process.stdout.write(`\x1b]7;${cwdURL}\x1b\\`)
process.stdout.write(`WISP_MUX_PROBE_READY ${report.marker}\r\n`)
await writeFile(outputPath, JSON.stringify(report))

const timeout = Date.now() + 180_000
const timer = setInterval(() => {
  if (existsSync(stopPath) || Date.now() >= timeout) clearInterval(timer)
}, 100)
