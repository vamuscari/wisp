$ErrorActionPreference = 'Stop'
$request = ConvertFrom-Json -InputObject ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encodedRequest)))
if ($request.probe_version -ne 1) { throw 'Unsupported probe request version' }
$outputPath = $request.output
$stopPath = $request.stop
$probeArgs = @($request.args)
$cwd = (Get-Location).ProviderPath
$cwdURL = ([UriBuilder]::new('file', '', -1, ($cwd + [IO.Path]::DirectorySeparatorChar))).Uri.AbsoluteUri
$report = @{
    probe_version = 1
    argument_transport = 'structured'
    shell_pid = $PID
    cwd = $cwd
    cwd_url = $cwdURL
    home = $env:HOME
    args = $probeArgs
    pane_id = $env:WEZTERM_PANE
    marker = $env:WISP_MUX_PROBE_MARKER
}
[Console]::Write("$([char]27)]7;$cwdURL$([char]27)\")
[Console]::Write("WISP_MUX_PROBE_READY $env:WISP_MUX_PROBE_MARKER`r`n")
$json = ConvertTo-Json -InputObject $report -Compress
[IO.File]::WriteAllText($outputPath, $json, [Text.UTF8Encoding]::new($false))

$timeout = [DateTime]::UtcNow.AddSeconds(180)
while (-not [IO.File]::Exists($stopPath) -and [DateTime]::UtcNow -lt $timeout) {
    Start-Sleep -Milliseconds 100
}
