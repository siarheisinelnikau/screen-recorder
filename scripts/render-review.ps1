param([string]$Executable, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
if (-not $Executable) {
    $taskManifest = Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
    $taskVersion = [regex]::Match($taskManifest, '(?m)^version = "([^"]+)"').Groups[1].Value
    $Executable = Join-Path $taskRoot "dist/ScreenRecorder-v$taskVersion.exe"
}
$taskExecutable = [IO.Path]::GetFullPath($Executable)
if (-not (Test-Path -LiteralPath $taskExecutable -PathType Leaf)) { throw 'Package the current executable first' }
if (-not $OutputDirectory) {
    $taskParent = Join-Path $taskRoot '.test-output'
    New-Item -ItemType Directory -Path $taskParent -Force | Out-Null
    $OutputDirectory = Join-Path $taskParent ('render-review-' + [guid]::NewGuid().ToString('N'))
}
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $taskOutput) { throw 'Review output directory must be new' }
$taskStartInfo = [Diagnostics.ProcessStartInfo]::new($taskExecutable)
$taskStartInfo.UseShellExecute = $false
$taskStartInfo.CreateNoWindow = $true
$taskStartInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$taskStartInfo.WorkingDirectory = $taskRoot
$taskStartInfo.RedirectStandardOutput = $true
$taskStartInfo.RedirectStandardError = $true
$taskStartInfo.ArgumentList.Add('--render-review')
$taskStartInfo.ArgumentList.Add($taskOutput)
$taskProcess = [Diagnostics.Process]::new()
$taskProcess.StartInfo = $taskStartInfo
$taskTimer = [Diagnostics.Stopwatch]::StartNew()
try {
    if (-not $taskProcess.Start()) { throw 'Offscreen review did not start' }
    $taskPid = $taskProcess.Id
    $taskPeakWorkingSet = 0L
    while (-not $taskProcess.WaitForExit(25)) {
        if ($taskTimer.ElapsedMilliseconds -gt 45000) {
            # Only this disposable, isolated review process is terminated on timeout.
            $taskProcess.Kill($true)
            $taskProcess.WaitForExit()
            throw 'Offscreen review exceeded the external 45-second timeout'
        }
        try {
            $taskProcess.Refresh()
            $taskPeakWorkingSet = [Math]::Max($taskPeakWorkingSet, $taskProcess.WorkingSet64)
        } catch {
            if (-not $taskProcess.HasExited) { throw }
        }
    }
    $taskTimer.Stop()
    $taskError = $taskProcess.StandardError.ReadToEnd()
    if ($taskProcess.ExitCode -ne 0) { throw "Offscreen review failed: $taskError" }
    $taskCpu = $taskProcess.TotalProcessorTime.TotalMilliseconds
    $taskReport = [ordered]@{
        executable = $taskExecutable
        sha256 = (Get-FileHash -LiteralPath $taskExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
        pid = $taskPid
        exit_code = $taskProcess.ExitCode
        elapsed_ms = $taskTimer.ElapsedMilliseconds
        cpu_ms = $taskCpu
        cpu_percent_one_core = 100 * $taskCpu / [Math]::Max(1, $taskTimer.ElapsedMilliseconds)
        sampled_peak_working_set_bytes = $taskPeakWorkingSet
        scope = 'Finite CPU offscreen capture only; no window or idle/hidden runtime verification'
    }
    $taskReport | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput 'process.json') -Encoding utf8
    Write-Output "Offscreen review exited: $taskOutput"
    Write-Output ($taskReport | ConvertTo-Json -Compress)
} finally { $taskProcess.Dispose() }
