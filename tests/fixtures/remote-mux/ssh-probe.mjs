import { existsSync, writeSync } from "node:fs"
import { createHash } from "node:crypto"
import { readFile, rename, writeFile } from "node:fs/promises"
import { pathToFileURL } from "node:url"
import { sep } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const [output, stop, ...args] = process.argv.slice(2)
if (!output || !stop || !process.env.WISP_MUX_PROBE_MARKER) throw new Error("missing owned SSH probe inputs")
const cookie = `${process.env.WISP_MUX_PROBE_MARKER}:${process.pid}:${Date.now()}`
const cwdURL = pathToFileURL(process.cwd() + sep).href
const report = {
  probe_version: 1, marker: process.env.WISP_MUX_PROBE_MARKER,
  pid: process.pid, pane_id: process.env.WEZTERM_PANE,
  cwd: process.cwd(), cwd_url: cwdURL, args, cookie,
  target_env: process.env.WISP_TEST_TARGET_ENV ?? null,
}
const frame = (name, value) => `\x1b]1337;SetUserVar=${name}=${Buffer.from(value).toString("base64")}\x07`
const write = (value) => new Promise((resolve, reject) => process.stdout.write(value, (error) => error ? reject(error) : resolve()))
async function publishFrames(ack) {
  const source = ack.action === "publish"
    ? frame("WISP_TEST_ATTACHMENT", cookie) + frame("WISP_TEST_ATTACHMENT_ACK", JSON.stringify(ack))
    : frame("WISP_TEST_BARRIER", JSON.stringify(ack))
  const diagnostics = process.env.WISP_TEST_EMISSION_DIAGNOSTICS === "1"
  const measured = { ...ack, node: process.version, uv: process.versions.uv, platform: process.platform,
    stdout_tty: Boolean(process.stdout.isTTY), routes: [] }
  const save = async () => {
    await writeFile(output + ".emission.json.tmp", JSON.stringify(measured))
    await rename(output + ".emission.json.tmp", output + ".emission.json")
  }
  for (const [route, bytes] of [["stream", source], ...(diagnostics && ack.action === "publish"
    ? [["descriptor", frame("WISP_TEST_RAW_ACK", JSON.stringify(ack))]] : [])]) {
    const value = Buffer.from(bytes)
    const observation = { route, bytes: value.length, sha256: createHash("sha256").update(value).digest("hex"), status: "pending" }
    measured.routes.push(observation)
    await save()
    try {
      if (route === "stream") await write(value)
      else for (let offset = 0; offset < value.length;) {
        const count = writeSync(1, value, offset)
        if (count <= 0) throw new Error("descriptor write made no progress")
        offset += count
      }
      observation.status = "completed"
    } catch (error) {
      observation.status = "failed"; observation.code = error.code ?? "write-failed"
      await save()
      if (route === "stream") throw error
    }
    await save()
  }
}
// The initial token deliberately precedes attachment. Recovery requires an
// explicit request to this same process, never passive replay or a new token.
process.stdout.write(`\x1b]7;${cwdURL}\x1b\\`)
await write(frame("WISP_TEST_ATTACHMENT", cookie))
process.stdout.write(`WISP_SSH_READY ${cookie}\r\n`)
await writeFile(output + ".tmp", JSON.stringify(report))
await rename(output + ".tmp", output)
const deadline = Date.now() + 600_000
let previous, sequence = 0
while (!existsSync(stop) && Date.now() < deadline) {
  let source
  try { source = await readFile(output + ".request.json", "utf8") } catch (error) { if (error.code !== "ENOENT") throw error }
  if (source !== undefined && source !== previous) {
    previous = source
    let ack
    try {
      if (Buffer.byteLength(source) > 4096) throw new Error("oversized publication request")
      const request = JSON.parse(source)
      if (request?.probe_version !== 1) throw new Error("unsupported publication request version")
      if (!["publish", "barrier"].includes(request.action)) throw new Error("invalid publication action")
      if (request.marker !== report.marker || request.cookie !== cookie) throw new Error("publication identity mismatch")
      if (!/^[a-f0-9]{24}$/.test(request.generation)) throw new Error("invalid connection generation")
      if (!Number.isSafeInteger(request.sequence) || request.sequence <= sequence) throw new Error("stale publication sequence")
      sequence = request.sequence
      ack = { probe_version: 1, action: request.action, marker: report.marker, cookie, generation: request.generation,
        sequence, status: request.action === "publish" ? "published" : "barrier" }
      // A barrier emits neither the attachment token nor its acknowledgement.
      // The optional raw descriptor variable is diagnostic only, never identity.
      await publishFrames(ack)
    } catch (error) {
      ack = { probe_version: 1, marker: report.marker, cookie, status: "rejected", error: error.message }
    }
    await writeFile(output + ".ack.json.tmp", JSON.stringify(ack))
    await rename(output + ".ack.json.tmp", output + ".ack.json")
  }
  await delay(100)
}
