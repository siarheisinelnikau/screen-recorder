param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskOldFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
Push-Location -LiteralPath $taskRoot
try {
    $taskManifest = Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
    $taskVersion = [regex]::Match($taskManifest, '(?m)^version = "([^"]+)"').Groups[1].Value
    if (-not $taskVersion) { throw 'Cannot read application version' }
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
    $taskArgs = @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-msvc', '--jobs', '2', '--target-dir', (Join-Path $taskRoot 'target'))
    if ($Offline) { $taskArgs += '--offline' }
    & cargo @taskArgs
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $taskDist = Join-Path $taskRoot 'dist'
    New-Item -ItemType Directory -Path $taskDist -Force | Out-Null
    $taskAsset = "ScreenRecorder-v$taskVersion.exe"
    $taskOutput = Join-Path $taskDist $taskAsset
    Copy-Item -LiteralPath (Join-Path $taskRoot 'target/x86_64-pc-windows-msvc/release/ScreenRecorder.exe') -Destination $taskOutput
    $taskHash = (Get-FileHash -LiteralPath $taskOutput -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $taskDist 'SHA256SUMS.txt'), "$taskHash  $taskAsset`n", [Text.Encoding]::ASCII)
    Write-Output "Packaged $taskOutput"
    Write-Output "SHA-256 $taskHash"
} finally {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $taskOldFlags
    Pop-Location
}
