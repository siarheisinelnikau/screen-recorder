param([string]$Executable,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path -Parent $PSScriptRoot
if (-not $Executable) {
    $taskManifest=Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
    $taskVersion=[regex]::Match($taskManifest,'(?m)^version = "([^"]+)"').Groups[1].Value
    $Executable=Join-Path $taskRoot "dist/ScreenRecorder-v$taskVersion.exe"
}
if (-not $OutputDirectory) {$OutputDirectory=Join-Path $taskRoot ('.test-output/icons-'+[guid]::NewGuid().ToString('N'))}
$taskOutput=[IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $taskOutput) {throw 'Choose a new icon review directory'}
New-Item -ItemType Directory -Path $taskOutput | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ScreenRecorderIconResource {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr LoadLibraryExW(string path,IntPtr file,uint flags);
 [DllImport("kernel32")] public static extern bool FreeLibrary(IntPtr module);
 [DllImport("user32",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr LoadImageW(IntPtr module,IntPtr name,uint type,int x,int y,uint flags);
 [DllImport("user32")] public static extern bool DestroyIcon(IntPtr icon);
}
'@
# Data/image resource mapping only: this never executes the target's entry point.
$taskModule=[ScreenRecorderIconResource]::LoadLibraryExW([IO.Path]::GetFullPath($Executable),[IntPtr]::Zero,0x22)
if ($taskModule -eq [IntPtr]::Zero) {throw 'Cannot map executable icon resources'}
$taskResults = @()
try {
    foreach ($taskSize in @(16,20,24,32,48,64,128,256)) {
        $taskHandle=[ScreenRecorderIconResource]::LoadImageW($taskModule,[IntPtr]1,1,$taskSize,$taskSize,0)
        if ($taskHandle -eq [IntPtr]::Zero) {throw "Cannot load packaged icon at $taskSize pixels"}
        try {
            $taskIcon=[Drawing.Icon]::FromHandle($taskHandle)
            $taskBitmap=$taskIcon.ToBitmap()
            try {
                if ($taskBitmap.Width -ne $taskSize -or $taskBitmap.Height -ne $taskSize) { throw 'Wrong loaded icon dimensions' }
                $taskLeft=$taskSize; $taskTop=$taskSize; $taskRight=-1; $taskBottom=-1; $taskVisible=0
                for ($taskY=0; $taskY -lt $taskSize; $taskY++) {
                    for ($taskX=0; $taskX -lt $taskSize; $taskX++) {
                        if ($taskBitmap.GetPixel($taskX,$taskY).A -ge 32) {
                            $taskLeft=[Math]::Min($taskLeft,$taskX); $taskTop=[Math]::Min($taskTop,$taskY)
                            $taskRight=[Math]::Max($taskRight,$taskX); $taskBottom=[Math]::Max($taskBottom,$taskY)
                            $taskVisible++
                        }
                    }
                }
                if ($taskVisible -eq 0 -or ($taskRight-$taskLeft) -lt ($taskSize/3)) { throw 'Blank or optically undersized icon' }
                $taskResults += [ordered]@{size=$taskSize;visible_pixels=$taskVisible;alpha_bounds=@($taskLeft,$taskTop,$taskRight,$taskBottom)}
                $taskBitmap.Save((Join-Path $taskOutput "icon-$taskSize.png"),[Drawing.Imaging.ImageFormat]::Png)
            } finally {$taskBitmap.Dispose();$taskIcon.Dispose()}
        } finally {[void][ScreenRecorderIconResource]::DestroyIcon($taskHandle)}
    }
} finally {[void][ScreenRecorderIconResource]::FreeLibrary($taskModule)}
Write-Output "Loaded and rendered packaged Windows icon resources without launching: $taskOutput"
$taskResults | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $taskOutput 'resources.json') -Encoding utf8
