param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    & cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting failed' }
    $taskCommon = @('--locked', '--jobs', '2', '--target-dir', (Join-Path $taskRoot 'target'))
    if ($Offline) { $taskCommon += '--offline' }
    & cargo clippy --all-targets @taskCommon -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    & cargo test --all-targets @taskCommon
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
} finally { Pop-Location }
