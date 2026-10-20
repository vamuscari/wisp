import { posix, win32 } from "node:path"

// Test-only selection: a missing shell is never silently substituted.
export function selectMuxProfiles(platform, env) {
  const powerShell = { name: "PowerShell 7", variable: "WISP_TEST_POWERSHELL", bash: false }
  const definitions = platform === "win32" ? {
    powershell7: powerShell,
    "windows-powershell": { name: "Windows PowerShell", variable: "WISP_TEST_WINDOWS_POWERSHELL", bash: false },
    "git-bash": { name: "Git Bash", variable: "WISP_TEST_GIT_BASH", bash: true },
  } : {
    bash: { name: "Bash", executable: "/bin/bash", bash: true },
    powershell7: powerShell,
  }
  const defaults = platform === "win32"
    ? ["powershell7", "windows-powershell", "git-bash"]
    : ["bash", ...(env.WISP_TEST_POWERSHELL ? ["powershell7"] : [])]
  const selected = env.WISP_TEST_MUX_PROFILES === undefined
    ? defaults : env.WISP_TEST_MUX_PROFILES.split(",").map((id) => id.trim())
  if (selected.some((id) => id === "")) throw new Error("WISP_TEST_MUX_PROFILES contains an empty profile")
  const seen = new Set()
  return selected.map((id) => {
    if (!Object.hasOwn(definitions, id)) throw new Error(`unsupported mux profile '${id}' for ${platform}`)
    if (seen.has(id)) throw new Error(`duplicate mux profile '${id}'`)
    seen.add(id)
    const definition = definitions[id]
    const executable = definition.variable ? env[definition.variable] : definition.executable
    if (!executable || !(platform === "win32" ? win32 : posix).isAbsolute(executable)) {
      throw new Error(`${definition.variable} must be an absolute executable path`)
    }
    return { name: definition.name, executable, bash: definition.bash }
  })
}
