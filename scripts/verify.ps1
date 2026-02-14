param([switch]$Offline,[switch]$NativeMedia)
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
    if ($NativeMedia) {
        & cargo test --test media_foundation @taskCommon -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) { throw 'Synthetic Media Foundation round trip failed' }
    }
} finally { Pop-Location }
