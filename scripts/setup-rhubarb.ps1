$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskDestination = Join-Path $taskRoot 'tools\rhubarb'
$taskArchive = Join-Path $taskRoot 'tools\rhubarb-1.14.0.zip'
$taskExpected = '62FA416A8D5E382A3828EE4BEF358CE520D0B4CABDEAEA75A7AC266D098D1FE3'
New-Item -ItemType Directory -Force -Path $taskDestination | Out-Null
Invoke-WebRequest -Uri 'https://github.com/DanielSWolf/rhubarb-lip-sync/releases/download/v1.14.0/Rhubarb-Lip-Sync-1.14.0-Windows.zip' -OutFile $taskArchive
if ((Get-FileHash -LiteralPath $taskArchive -Algorithm SHA256).Hash -ne $taskExpected) { throw 'Rhubarb archive SHA256 mismatch' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskZip = [IO.Compression.ZipFile]::OpenRead($taskArchive)
try {
    foreach ($taskEntry in $taskZip.Entries) {
        $taskRelative = ($taskEntry.FullName -split '/', 2)[1]
        if (-not $taskRelative -or $taskRelative.EndsWith('/')) { continue }
        if ($taskRelative -ne 'rhubarb.exe' -and $taskRelative -ne 'LICENSE.md' -and $taskRelative -ne 'README.adoc' -and -not $taskRelative.StartsWith('res/')) { continue }
        $taskTarget = [IO.Path]::GetFullPath((Join-Path $taskDestination $taskRelative))
        if (-not $taskTarget.StartsWith([IO.Path]::GetFullPath($taskDestination) + [IO.Path]::DirectorySeparatorChar)) { throw 'Invalid archive path' }
        New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($taskTarget)) | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($taskEntry, $taskTarget, $true)
    }
} finally { $taskZip.Dispose() }
Remove-Item -LiteralPath $taskArchive
& (Join-Path $taskDestination 'rhubarb.exe') --version
