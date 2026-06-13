param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    $taskArgs = @('metadata', '--locked', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc')
    if ($Offline) { $taskArgs += '--offline' }
    $taskMetadataText = (& cargo @taskArgs | Out-String)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve dependency notices' }
    $taskMetadata = $taskMetadataText | ConvertFrom-Json
    $taskText = [Text.StringBuilder]::new()
    [void]$taskText.AppendLine('ScreenRecorder third-party dependency and bundled font notices')
    [void]$taskText.AppendLine('Includes the locked Windows dependency graph, build tools and test dependencies.')
    foreach ($taskPackage in ($taskMetadata.packages | Sort-Object name,version)) {
        if (-not $taskPackage.source -and $taskPackage.name -ne 'rust-auto-updator') { continue }
        $taskFolder = Split-Path -Parent $taskPackage.manifest_path
        [void]$taskText.AppendLine("`n===== $($taskPackage.name) $($taskPackage.version) =====")
        [void]$taskText.AppendLine("Declared license: $($taskPackage.license)")
        [void]$taskText.AppendLine("Source: $($taskPackage.repository)")
        $taskLicenseFiles = Get-ChildItem -LiteralPath $taskFolder -File -Recurse | Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)' -or
            ($_.Directory.Name -eq 'fonts' -and $_.Extension -eq '.txt') -or
            ($taskPackage.license_file -and $_.FullName -eq (Join-Path $taskFolder $taskPackage.license_file))
        }
        if ($taskPackage.name -eq 'rust-auto-updator') {
            [void]$taskText.AppendLine("`n--- Workspace LICENSE ---")
            [void]$taskText.AppendLine([IO.File]::ReadAllText((Join-Path $taskFolder '../../LICENSE')))
        }
        foreach ($taskLicenseFile in $taskLicenseFiles) {
            [void]$taskText.AppendLine("`n--- $($taskLicenseFile.FullName.Substring($taskFolder.Length + 1)) ---")
            [void]$taskText.AppendLine([IO.File]::ReadAllText($taskLicenseFile.FullName))
        }
    }
    $taskDist = Join-Path $taskRoot 'dist'
    New-Item -ItemType Directory -Path $taskDist -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $taskDist 'THIRD_PARTY_NOTICES.txt'), $taskText.ToString(), [Text.UTF8Encoding]::new($false))
    Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination (Join-Path $taskDist 'LICENSE.txt')
    Write-Output 'Generated locked dependency and bundled font notices'
} finally { Pop-Location }
