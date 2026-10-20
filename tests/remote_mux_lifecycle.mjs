import assert from "node:assert/strict"
import { open, writeFile } from "node:fs/promises"
import { basename } from "node:path"

export function powershellCommand(source) {
  const prefix = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
    + "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)\n"
  const command = "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand "
    + Buffer.from(prefix + source, "utf16le").toString("base64")
  assert.ok(command.length < 8000, "fixed Windows bootstrap exceeds the command-line budget")
  return command
}

export const probeInput = `$r=ConvertFrom-Json -InputObject ([Console]::In.ReadLine())
if($r.probe_version -ne 1){throw 'Unsupported SSH probe request'}
`
const rootGuard = String.raw`
if($r.marker -cnotmatch '^[a-f0-9]{24}$'){throw 'Invalid run marker'}
$root=[IO.Path]::GetFullPath($r.root)
$expected=Join-Path ([IO.Path]::GetTempPath()) ('w-ssh-'+$r.marker)
if(-not $root.Equals([IO.Path]::GetFullPath($expected),[StringComparison]::OrdinalIgnoreCase)){throw 'Not an owned test root'}
if((Test-Path -LiteralPath $root) -and ((Get-Item -LiteralPath $root -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Refusing a reparse-point root'}
`
const claimGuard = String.raw`
$claim=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText((Join-Path $root 'owner.json')))
if($claim.probe_version -ne 1){throw 'Unsupported owner version'}
if($claim.marker -cne $r.marker){throw 'Owner marker mismatch'}
$config=Join-Path $root 'server.lua'
`
const muxInventory = String.raw`
function Muxes { @(Get-CimInstance Win32_Process -Filter "Name='wezterm-mux-server.exe'") }
function OwnedMuxes { @(Muxes | Where-Object {$_.CommandLine -and ($_.CommandLine.Contains($config) -or $_.CommandLine.Contains($config.Replace('\','/')))}) }
`
const probeInventory = String.raw`
function Probes { @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" | Where-Object {$_.CommandLine -and ($_.CommandLine.Contains($root.Replace('\','/')+'/probe.mjs') -or $_.CommandLine.Contains((Join-Path $root 'probe.mjs')))}) }
`
const baseline = String.raw`
[pscustomobject]@{probe_version=1;baseline=@(Muxes | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;created=$_.CreationDate.ToUniversalTime().ToString('o')}})}|ConvertTo-Json -Depth 4 -Compress
`

const ownedGuard = rootGuard + claimGuard + muxInventory + "$owned=@(OwnedMuxes)\n"

// FileAttributeTagInfo is two DWORDs: attributes, then tag. All supported
// Windows targets are little-endian. The handle opens the reparse point itself
// (including directories), never its target, and is closed even on query error.
// Native constants: READ_ATTRIBUTES=128, shared read/write/delete=7,
// OPEN_EXISTING=3, OPEN_REPARSE_POINT|BACKUP_SEMANTICS=35651584,
// FileAttributeTagInfo=9, REPARSE_POINT attribute=1024; AF_UNIX tag=2147483683.
export const nativeReparseTagQuery = String.raw`
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using H=Microsoft.Win32.SafeHandles.SafeFileHandle;
public static class WispTag {
[DllImport("kernel32",CharSet=CharSet.Unicode)]public static extern H CreateFile(string p,uint a,uint s,IntPtr t,uint d,uint f,IntPtr h);
[DllImport("kernel32")]public static extern bool GetFileInformationByHandleEx(H h,int c,out ulong v,uint n);
}
'@
function Get-ProbeReparseTag($path){
 $h=[WispTag]::CreateFile($path,128,7,[IntPtr]::Zero,3,35651584,[IntPtr]::Zero)
 try{
  $v=[uint64]0
  if($h.IsInvalid -or -not [WispTag]::GetFileInformationByHandleEx($h,9,[ref]$v,8)){throw 'Tag query failed'}
  if($v -band 1024){$v -shr 32}else{0}
 }finally{$h.Dispose()}
}
`

export const remoteProbeScripts = {
  // This preflight is read-only. The caller stores the identity before claim.
  prepare: muxInventory + String.raw`
if($r.marker -cnotmatch '^[a-f0-9]{24}$'){throw 'Invalid run marker'}
$root=Join-Path ([IO.Path]::GetTempPath()) ('w-ssh-'+$r.marker)
$v=(@(& wezterm.exe --version)-join ' ').Trim()
if($LASTEXITCODE -ne 0){throw 'Remote CLI version failed'}
$m=(@(& wezterm-mux-server.exe --version)-join ' ').Trim()
if($LASTEXITCODE -ne 0 -or ($v -split ' ')[-1] -cne ($m -split ' ')[-1]){throw 'Remote CLI/mux version mismatch'}
[pscustomobject]@{probe_version=1;root=$root.Replace('\','/');node=(Get-Command node.exe -CommandType Application | Select-Object -First 1).Source;version=$v;baseline=@(Muxes | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;created=$_.CreationDate.ToUniversalTime().ToString('o')}})}|ConvertTo-Json -Depth 4 -Compress
`,
  claim: rootGuard + String.raw`
if(Test-Path -LiteralPath $root){throw 'Staging already exists; not claimed'}
$null=New-Item -ItemType Directory -Path $root
try {
 [IO.File]::WriteAllText((Join-Path $root 'owner.json'),(ConvertTo-Json -InputObject @{probe_version=1;marker=$r.marker} -Compress))
} catch {
 if(-not [IO.Directory]::GetFileSystemEntries($root).Length){[IO.Directory]::Delete($root)}
 throw
}
'{"probe_version":1,"claimed":true}'
`,
  stage: ownedGuard + String.raw`
$names=@($r.files.PSObject.Properties.Name)
if($names.Count -ne 1 -or $names[0] -cnotin @('probe.mjs','server.lua','target-spawn.lua','target-observe.lua')){throw 'Stage exactly one fixed fixture'}
foreach($name in $names){
  $bytes=[Convert]::FromBase64String($r.files.$name)
  if($bytes.Length -gt 5120){throw 'Oversized fixture'}
 $target=Join-Path $root $name
 if(Test-Path -LiteralPath $target){throw 'Fixture already exists'}
 [IO.File]::WriteAllBytes($target,$bytes)
}
$cwd=Join-Path $root ('work area '+[char]0x96ea)
if(-not (Test-Path -LiteralPath $cwd)){$null=New-Item -ItemType Directory -Path $cwd}
[pscustomobject]@{probe_version=1;staged=$true;cwd=$cwd}|ConvertTo-Json -Compress
`,
  bootstrap: ownedGuard + String.raw`
if($owned.Count){throw 'Owned mux already exists'}
foreach($name in @('cache','data','config','run')){$null=New-Item -ItemType Directory -Path (Join-Path $root $name)}
$env:XDG_CACHE_HOME=Join-Path $root 'cache';$env:XDG_DATA_HOME=Join-Path $root 'data'
$env:XDG_CONFIG_HOME=Join-Path $root 'config';$env:XDG_RUNTIME_DIR=Join-Path $root 'run'
$mux=(Get-Command wezterm-mux-server.exe -CommandType Application | Select-Object -First 1).Source
$argv=@('--config-file',$config,'--daemonize','--cwd',$root,'--','node.exe',(Join-Path $root 'probe.mjs'),(Join-Path $root 'keeper.json'),(Join-Path $root 'stop'),'keeper')
& $mux @argv
if($LASTEXITCODE -ne 0){throw 'Owned mux launcher exited unsuccessfully'}
`,
  status: ownedGuard + String.raw`
$env:WEZTERM_UNIX_SOCKET=Join-Path $root 's'
$report=$null;$process=$null
if($r.report -notin @('keeper.json','argv.json')){throw 'Unknown report'}
$file=Join-Path $root $r.report
if(Test-Path -LiteralPath $file){
 $report=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($file))
 if($report.probe_version -ne 1){throw 'Unsupported keeper version'}
 if($report.marker -cne $r.marker){throw 'Foreign keeper'}
 $process=Get-CimInstance Win32_Process -Filter ('ProcessId='+[uint32]$report.pid)
 if($process -and -not $process.CommandLine.Contains($root.Replace('\','/')+'/probe.mjs') -and -not $process.CommandLine.Contains((Join-Path $root 'probe.mjs'))){throw 'Keeper PID was reused'}
}
$listening=$false
if($owned.Count -eq 1){
 $ErrorActionPreference='Continue'
 $null=& wezterm.exe --config-file $config cli --prefer-mux --no-auto-start list --format json 2>$null
 $listening=($LASTEXITCODE -eq 0)
 $ErrorActionPreference='Stop'
}
[pscustomobject]@{probe_version=1;ready=($listening -and [bool]$process);report=$report;mux_pid=if($owned.Count -eq 1){$owned[0].ProcessId}else{$null};process_created=if($process){$process.CreationDate.ToUniversalTime().ToString('o')}else{$null}}|ConvertTo-Json -Depth 5 -Compress
`,
  publish: rootGuard + claimGuard + String.raw`
if($r.report -notin @('keeper.json','argv.json')){throw 'Unknown publication report'}
$p=$r.publication
if($p.probe_version -ne 1){throw 'Unsupported publication request version'}
if($p.action -cnotin @('publish','barrier')){throw 'Invalid publication action'}
$file=Join-Path $root $r.report
if((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse-point report'}
$report=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($file))
if($report.probe_version -ne 1){throw 'Unsupported probe report version'}
if($report.marker -cne $r.marker -or $p.marker -cne $r.marker -or $p.cookie -cne $report.cookie){throw 'Publication identity mismatch'}
if($p.generation -cnotmatch '^[a-f0-9]{24}$' -or $p.sequence -isnot [long] -and $p.sequence -isnot [int] -or $p.sequence -lt 1){throw 'Invalid publication generation or sequence'}
$target=$file+'.request.json';$temp=$target+'.tmp'
foreach($path in @($target,$temp)){
 if((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Refusing a reparse-point request'}
}
[IO.File]::WriteAllText($temp,(ConvertTo-Json -InputObject $p -Compress),[Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $temp -Destination $target -Force
'{"probe_version":1,"requested":true}'
`,
  spawn: ownedGuard + String.raw`
if($owned.Count -ne 1){throw 'Expected one owned target mux'}
$p=$r.command
if($p.probe_version -ne 1){throw 'Unsupported target spawn version'}
if($p.action -cne 'spawn' -or $p.marker -cne $r.marker){throw 'Target spawn identity mismatch'}
if($p.generation -cnotmatch '^[a-f0-9]{24}$' -or $p.sequence -lt 1){throw 'Invalid target spawn identity'}
$report=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText((Join-Path $root 'keeper.json')))
if($report.probe_version -ne 1){throw 'Unsupported keeper version'}
if($report.marker -cne $r.marker -or $report.cookie -cne $p.cookie){throw 'Foreign target keeper'}
$ack=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText((Join-Path $root 'keeper.json.ack.json')))
if($ack.probe_version -ne 1){throw 'Unsupported keeper acknowledgement version'}
if($ack.action -cne 'publish' -or $ack.status -cne 'published'){throw 'Invalid keeper acknowledgement'}
foreach($name in @('marker','generation','cookie','sequence')){
 if($ack.$name -cne $p.$name){throw 'Stale keeper acknowledgement'}
}
$target=Join-Path $root 'spawn.request.json';$temp=$target+'.tmp'
foreach($path in @($target,$temp)){if(Test-Path -LiteralPath $path){throw 'Target spawn request already exists'}}
$source=ConvertTo-Json -InputObject $p -Depth 6 -Compress
if([Text.Encoding]::UTF8.GetByteCount($source) -gt 4096){throw 'Oversized target spawn request'}
[IO.File]::WriteAllText($temp,$source,[Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $temp -Destination $target
'{"probe_version":1,"requested":true}'
`,
  spawn_status: rootGuard + claimGuard + String.raw`
$path=Join-Path $root 'spawn.result.json';$result=$null
if(Test-Path -LiteralPath $path){
 if((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse-point result'}
 $result=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($path))
 if($result.probe_version -ne 1){throw 'Unsupported target result version'}
 if($result.marker -cne $r.marker){throw 'Foreign target result'}
}
[pscustomobject]@{probe_version=1;result=$result}|ConvertTo-Json -Depth 5 -Compress
`,
  observe: rootGuard + claimGuard + String.raw`
$p=$r.observation
if($p.probe_version -ne 1){throw 'Unsupported observation version'}
if($p.action -cne 'observe' -or $p.marker -cne $r.marker -or $p.phase -cnotin @('before','after')){throw 'Invalid observation identity or phase'}
if($p.generation -cnotmatch '^[a-f0-9]{24}$' -or $p.sequence -lt 1){throw 'Invalid observation generation or sequence'}
function ReadOwned($name){
 $path=Join-Path $root $name
 if(-not (Test-Path -LiteralPath $path)){return $null}
 if((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse-point observation'}
 $v=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($path))
 if($v.probe_version -ne 1){throw 'Unsupported observation source version'}
 return $v
}
$report=ReadOwned 'keeper.json'
if(-not $report -or $report.marker -cne $r.marker -or $report.cookie -cne $p.cookie){throw 'Foreign observation keeper'}
if($p.phase -ceq 'after'){
 $ack=ReadOwned 'keeper.json.ack.json'
 $ready=($ack -and $ack.action -ceq 'publish' -and $ack.status -ceq 'published')
 foreach($name in @('marker','generation','cookie','sequence')){if(-not $ack -or $ack.$name -cne $p.$name){$ready=$false}}
 if(-not $ready){'{"probe_version":1,"requested":false}';return}
}
$target=Join-Path $root 'observation.request.json';$temp=$target+'.tmp'
foreach($path in @($target,$temp)){
 if((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Refusing a reparse-point request'}
}
[IO.File]::WriteAllText($temp,(ConvertTo-Json -InputObject $p -Compress),[Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $temp -Destination $target -Force
'{"probe_version":1,"requested":true}'
`,
  observation_status: rootGuard + claimGuard + String.raw`
if($r.phase -cnotin @('startup','timer','before','after')){throw 'Invalid observation phase'}
$result=$null
foreach($name in @('error',$r.phase)){
 $path=Join-Path $root ('observation.'+$name+'.json')
 if(-not (Test-Path -LiteralPath $path)){continue}
 if((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse-point result'}
 $result=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($path))
 if($result.probe_version -ne 1){throw 'Unsupported observation result version'}
 if($result.marker -cne $r.marker){throw 'Foreign observation result'}
 if($name -ceq 'error'){throw 'Target observation failed'}
}
[pscustomobject]@{probe_version=1;result=$result}|ConvertTo-Json -Depth 8 -Compress
`,
  evidence: rootGuard + String.raw`
if(-not (Test-Path -LiteralPath $root)){'{"probe_version":1,"missing":true,"files":[]}';return}
` + claimGuard + String.raw`
$files=@()
foreach($name in @('mux.stdout','mux.stderr','launcher.stdout','launcher.stderr','keeper.json','argv.json','keeper.json.ack.json','argv.json.ack.json','keeper.json.emission.json','argv.json.emission.json','spawn.claim.json','spawn.result.json','observation.startup.json','observation.timer.json','observation.before.json','observation.after.json','observation.error.json')){
 $path=Join-Path $root $name
 if(-not (Test-Path -LiteralPath $path -PathType Leaf)){continue}
 if((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse-point log'}
 $f=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
 try {
  $length=$f.Length;$null=$f.Seek([Math]::Max(0,$length-4096),[IO.SeekOrigin]::Begin)
  $bytes=New-Object byte[] 4096;$count=$f.Read($bytes,0,4096)
  $files += [pscustomobject]@{name=$name;data_base64=[Convert]::ToBase64String($bytes,0,$count);truncated=($length -gt 4096)}
 } finally {$f.Dispose()}
}
[pscustomobject]@{probe_version=1;files=$files}|ConvertTo-Json -Depth 4 -Compress
`,
  stop: rootGuard + muxInventory + `if(-not (Test-Path -LiteralPath $root)){${baseline};return}\n` + claimGuard + probeInventory + String.raw`
[IO.File]::WriteAllText((Join-Path $root 'stop'),'stop')
$deadline=[DateTime]::UtcNow.AddSeconds(5)
while(@(Probes).Count -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
if(@(Probes).Count){throw "Owned probes still running; staging retained: $root"}
foreach($p in @(OwnedMuxes)){
 $live=Get-Process -Id $p.ProcessId -ErrorAction Stop
 try {
  $null=$live.Handle
  $now=Get-CimInstance Win32_Process -Filter ('ProcessId='+$p.ProcessId)
  if($now.CreationDate -ne $p.CreationDate -or $now.CommandLine -cne $p.CommandLine){throw 'Owned mux identity changed'}
  $live.Kill()
  if(-not $live.WaitForExit(5000)){throw "Owned mux still running; staging retained: $root"}
 } finally {$live.Dispose()}
}
if(@(OwnedMuxes).Count -or @(Probes).Count){throw "Owned processes remain; staging retained: $root"}
` + baseline,
  remove: rootGuard + String.raw`
if(-not (Test-Path -LiteralPath $root)){'{"probe_version":1,"removed":true}';return}
` + claimGuard + muxInventory + probeInventory + String.raw`
if(@(OwnedMuxes).Count -or @(Probes).Count){throw "Owned processes remain; staging retained: $root"}
$links=@(Get-ChildItem -LiteralPath $root -Force -Recurse -Attributes ReparsePoint)
if($links){
` + nativeReparseTagQuery + String.raw`
}
foreach($link in $links){
 if($link.FullName -cne (Join-Path $root 's')){throw 'Refusing a reparse-point tree'}
 if((Get-ProbeReparseTag $link.FullName) -ne 2147483683){throw 'Refusing a non-AF_UNIX reparse-point'}
 Remove-Item -LiteralPath $link.FullName -Force
}
Remove-Item -LiteralPath $root -Recurse -Force
if(Test-Path -LiteralPath $root){throw 'Owned staging removal failed'}
'{"probe_version":1,"removed":true}'
`,
}

export function createRemoteProbe(control) {
  let info
  const invoke = async (action, signal) => {
    const response = await control(action, info ? { root: info.root } : {}, signal)
    assert.equal(response?.probe_version, 1, "unsupported remote lifecycle response version")
    return response
  }
  return {
    get info() { return info },
    async prepare(signal) {
      const response = await invoke("prepare", signal)
      assert.match(response.root, /^[A-Za-z]:\/[A-Za-z0-9_./-]+\/w-ssh-[a-f0-9]{24}$/, "unsupported remote temp path")
      assert.ok(Array.isArray(response.baseline), "missing original mux inventory")
      info = response
      await invoke("claim", signal)
      return info
    },
    async evidence(signal) { return info ? invoke("evidence", signal) : undefined },
    async cleanup(signal) {
      if (!info) return
      try {
        const stopped = await invoke("stop", signal)
        assert.ok(Array.isArray(stopped.baseline) && info.baseline.every((existing) => stopped.baseline
          .some((current) => current.pid === existing.pid && current.created === existing.created)),
        "existing Windows mux was not preserved; staging retained")
        assert.equal((await invoke("remove", signal)).removed, true, "owned staging removal was not confirmed")
      } catch (error) {
        throw new Error(`remote cleanup for ${info.root}: ${error.message}`, { cause: error })
      }
    },
  }
}

export async function stopProbeChild(child, closed, { signal = "SIGTERM", killAfter = 5000, timeout = 6000 } = {}) {
  if (!child) return
  assert.ok(closed && typeof closed.then === "function", "untracked owned channel")
  assert.ok(Number.isSafeInteger(killAfter) && killAfter > 0 && Number.isSafeInteger(timeout) && timeout > killAfter,
    "invalid owned channel cleanup deadline")
  const running = () => child.exitCode === null && !child.signalCode
  let killTimer, closeTimer
  try {
    if (running()) {
      child.kill(signal)
      killTimer = setTimeout(() => { if (running()) child.kill("SIGKILL") }, killAfter)
    }
    await Promise.race([closed, new Promise((_, reject) => {
      closeTimer = setTimeout(() => {
        reject(new Error(`owned channel ${child.pid ?? "without PID"} exit/stdio close timed out after ${timeout}ms`))
        // Exit does not imply close: descendants may retain inherited pipes.
        // Release only our endpoints, never signal an unverified descendant.
        child.stdin?.destroy(); child.stdout?.destroy(); child.stderr?.destroy()
      }, timeout)
    })])
  } finally {
    clearTimeout(killTimer); clearTimeout(closeTimer)
  }
}

export async function withProbeCleanup(run, steps) {
  const errors = []
  let result, original
  try { result = await run() } catch (error) { original = error; errors.push(error) }
  for (const [name, cleanup] of steps) {
    const failure = errors.length > 1
      ? new AggregateError([...errors], errors.map((error) => error.message).join("\n"), { cause: original })
      : errors[0]
    try { await cleanup(failure) } catch (error) { errors.push(new Error(`${name}: ${error.message}`, { cause: error })) }
  }
  if (errors.length === 1) throw errors[0]
  if (errors.length) throw new AggregateError(errors, errors.map((error) => error.message).join("\n"), { cause: original })
  return result
}

export async function readProbeTail(path) {
  const file = await open(path, "r")
  try {
    const { size } = await file.stat()
    const bytes = Buffer.alloc(4096)
    const { bytesRead } = await file.read(bytes, 0, bytes.length, Math.max(0, size - bytes.length))
    return { name: basename(path), data_base64: bytes.subarray(0, bytesRead).toString("base64"), truncated: size > bytes.length }
  } finally { await file.close() }
}

export async function retainProbeEvidence(path, evidence) {
  assert.equal(evidence.probe_version, 1, "unsupported probe evidence version")
  const bytes = Buffer.from(JSON.stringify(evidence, null, 2) + "\n")
  assert.ok(bytes.length <= 512 * 1024, "probe evidence exceeds 512 KiB")
  await writeFile(path, bytes, { flag: "wx", mode: 0o600 })
}
