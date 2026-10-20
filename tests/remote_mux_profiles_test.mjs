import assert from "node:assert/strict"
import test from "node:test"
import { selectMuxProfiles } from "./remote_mux_profiles.mjs"

const windows = {
  WISP_TEST_POWERSHELL: "C:/Tools/PowerShell/pwsh.exe",
  WISP_TEST_WINDOWS_POWERSHELL: "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
  WISP_TEST_GIT_BASH: "C:/Program Files/Git/bin/bash.exe",
}

test("an explicit Windows subset does not require or impersonate PowerShell 7", () => {
  assert.deepEqual(selectMuxProfiles("win32", {
    WISP_TEST_MUX_PROFILES: "windows-powershell,git-bash",
    WISP_TEST_WINDOWS_POWERSHELL: windows.WISP_TEST_WINDOWS_POWERSHELL,
    WISP_TEST_GIT_BASH: windows.WISP_TEST_GIT_BASH,
  }), [
    { name: "Windows PowerShell", executable: windows.WISP_TEST_WINDOWS_POWERSHELL, bash: false },
    { name: "Git Bash", executable: windows.WISP_TEST_GIT_BASH, bash: true },
  ])
})

test("Windows defaults still require all three shell profiles", () => {
  assert.deepEqual(selectMuxProfiles("win32", windows), [
    { name: "PowerShell 7", executable: windows.WISP_TEST_POWERSHELL, bash: false },
    { name: "Windows PowerShell", executable: windows.WISP_TEST_WINDOWS_POWERSHELL, bash: false },
    { name: "Git Bash", executable: windows.WISP_TEST_GIT_BASH, bash: true },
  ])
})

test("unknown profiles fail before starting a server", () => {
  assert.throws(() => selectMuxProfiles("win32", {
    ...windows, WISP_TEST_MUX_PROFILES: "no-such-shell",
  }), /unsupported mux profile.*no-such-shell/)
})

test("duplicate profile selection is rejected", () => {
  assert.throws(() => selectMuxProfiles("win32", {
    ...windows, WISP_TEST_MUX_PROFILES: "git-bash,git-bash",
  }), /duplicate mux profile.*git-bash/)
})

test("empty profile selection is rejected rather than using defaults", () => {
  for (const selection of ["", " ", "git-bash,", ",git-bash"]) {
    assert.throws(() => selectMuxProfiles("win32", {
      ...windows, WISP_TEST_MUX_PROFILES: selection,
    }), /empty.*profile|profile.*empty/)
  }
})

test("selected executables must be present and absolute for the target platform", () => {
  for (const path of [undefined, "pwsh.exe"]) {
    assert.throws(() => selectMuxProfiles("win32", {
      WISP_TEST_MUX_PROFILES: "powershell7", WISP_TEST_POWERSHELL: path,
    }), /WISP_TEST_POWERSHELL must be an absolute executable path/)
  }
  assert.throws(() => selectMuxProfiles("linux", {
    WISP_TEST_POWERSHELL: "C:/Tools/pwsh.exe",
  }), /WISP_TEST_POWERSHELL must be an absolute executable path/)
})

test("POSIX defaults include Bash and an explicitly supplied PowerShell", () => {
  assert.deepEqual(selectMuxProfiles("linux", {}), [
    { name: "Bash", executable: "/bin/bash", bash: true },
  ])
  assert.deepEqual(selectMuxProfiles("darwin", { WISP_TEST_POWERSHELL: "/opt/bin/pwsh" }), [
    { name: "Bash", executable: "/bin/bash", bash: true },
    { name: "PowerShell 7", executable: "/opt/bin/pwsh", bash: false },
  ])
})

test("explicit selection preserves order and ignores unselected executables", () => {
  assert.deepEqual(selectMuxProfiles("win32", {
    ...windows, WISP_TEST_POWERSHELL: "not-absolute", WISP_TEST_MUX_PROFILES: "git-bash, windows-powershell",
  }), [
    { name: "Git Bash", executable: windows.WISP_TEST_GIT_BASH, bash: true },
    { name: "Windows PowerShell", executable: windows.WISP_TEST_WINDOWS_POWERSHELL, bash: false },
  ])
})

test("profiles from a different execution platform are rejected", () => {
  assert.throws(() => selectMuxProfiles("linux", {
    WISP_TEST_MUX_PROFILES: "git-bash", WISP_TEST_GIT_BASH: "/bin/bash",
  }), /unsupported mux profile.*git-bash/)
})
