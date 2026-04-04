param([string]$Executable,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path -Parent $PSScriptRoot
if (-not $Executable) {
    $taskManifest=Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
    $taskVersion=[regex]::Match($taskManifest,'(?m)^version = "([^"]+)"').Groups[1].Value
    $Executable=Join-Path $taskRoot "dist/ScreenRecorder-v$taskVersion.exe"
}
if (-not $OutputDirectory) {
    $taskParent=Join-Path $taskRoot '.test-output'
    New-Item -ItemType Directory -Path $taskParent -Force | Out-Null
    $OutputDirectory=Join-Path $taskParent ('native-render-'+[guid]::NewGuid().ToString('N'))
}
$taskOutput=[IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $taskOutput) {throw 'Diagnostic output must be new'}
$taskExecutable=[IO.Path]::GetFullPath($Executable)
$taskInfo=[Diagnostics.ProcessStartInfo]::new($taskExecutable)
$taskInfo.UseShellExecute=$false
$taskInfo.CreateNoWindow=$true
$taskInfo.WindowStyle=[Diagnostics.ProcessWindowStyle]::Normal
$taskInfo.WorkingDirectory=$taskRoot
$taskInfo.RedirectStandardError=$true
$taskInfo.RedirectStandardOutput=$true
$taskInfo.ArgumentList.Add('--diagnose-render')
$taskInfo.ArgumentList.Add($taskOutput)
$taskProcess=[Diagnostics.Process]::new()
$taskProcess.StartInfo=$taskInfo
$taskTimer=[Diagnostics.Stopwatch]::StartNew()
$taskGpu=@()
$taskGpuTargets=@(5000,16000,22000)
$taskGpuIndex=0
$taskPeak=0L
$taskModules=$null
try {
    if (-not $taskProcess.Start()) {throw 'Native diagnostic did not start'}
    $taskPid=$taskProcess.Id
    Write-Output "Isolated native GUI diagnostic PID $taskPid; about 25 seconds; no capture/audio/network"
    while (-not $taskProcess.WaitForExit(100)) {
        if ($taskTimer.ElapsedMilliseconds -gt 35000) {
            # Only the disposable instance started above may be terminated.
            $taskProcess.Kill($true)
            $taskProcess.WaitForExit()
            throw 'Native diagnostic exceeded its external 35-second limit'
        }
        try {$taskProcess.Refresh();$taskPeak=[Math]::Max($taskPeak,$taskProcess.WorkingSet64)} catch {if (-not $taskProcess.HasExited){throw}}
        if ($null -eq $taskModules -and $taskTimer.ElapsedMilliseconds -ge 2000) {
            try {
                $taskGraphics=@($taskProcess.Modules | Where-Object {$_.ModuleName -match '^(opengl32|d3d\d+|dxgi|vulkan-1|nvoglv64|atio6axx)\.dll$'} | ForEach-Object {$_.ModuleName})
                $taskModules=@{available=$true;graphics_modules=$taskGraphics;scope='Own diagnostic PID only; supporting evidence, not a standalone backend proof'}
            } catch {$taskModules=@{available=$false;error=$_.Exception.Message}}
        }
        if ($taskGpuIndex -lt $taskGpuTargets.Count -and $taskTimer.ElapsedMilliseconds -ge $taskGpuTargets[$taskGpuIndex]) {
            try {
                $taskEngines=@(Get-CimInstance -ClassName Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -Filter "Name LIKE 'pid_$($taskPid)_%'" -OperationTimeoutSec 2 -ErrorAction Stop | Where-Object {$_.Name -match "^pid_$($taskPid)_"} | Select-Object Name,UtilizationPercentage)
                $taskGpu+=@{elapsed_ms=$taskTimer.ElapsedMilliseconds;available=($taskEngines.Count -gt 0);engines=$taskEngines;scope='Per-engine formatted GPU counter snapshot, not whole-GPU utilization'}
            } catch {
                $taskGpu+=@{elapsed_ms=$taskTimer.ElapsedMilliseconds;available=$false;error=$_.Exception.Message}
                # A denied counter namespace is not made available by repeated queries.
                $taskGpuIndex=$taskGpuTargets.Count
            }
            $taskGpuIndex++
        }
    }
    $taskTimer.Stop()
    $taskError=$taskProcess.StandardError.ReadToEnd()
    $taskReport=@{executable=$taskExecutable;sha256=(Get-FileHash -LiteralPath $taskExecutable -Algorithm SHA256).Hash.ToLowerInvariant();pid=$taskPid;exit_code=$taskProcess.ExitCode;elapsed_ms=$taskTimer.ElapsedMilliseconds;sampled_peak_working_set_bytes=$taskPeak;gpu_samples=$taskGpu;module_probe=$taskModules}
    if (Test-Path -LiteralPath $taskOutput) {$taskReport|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $taskOutput 'process.json') -Encoding utf8}
    Write-Output "Native diagnostic exited: $taskOutput"
    Write-Output ($taskReport|ConvertTo-Json -Depth 8 -Compress)
    if ($taskProcess.ExitCode -ne 0) {throw "Native render gate failed: $taskError"}
    Get-Content -LiteralPath (Join-Path $taskOutput 'result.json') -Raw
} finally {$taskProcess.Dispose()}
