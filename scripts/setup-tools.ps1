$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskTools = Join-Path $taskRoot 'tools'
$taskLock = Get-Content -LiteralPath (Join-Path $taskTools 'runtime-lock.json') -Raw | ConvertFrom-Json
$taskScratch = Join-Path ([IO.Path]::GetTempPath()) ('AIR3view-tools-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskScratch | Out-Null
function Get-VerifiedArchive($taskEntry, [string]$taskName) {
    $taskDownload = Join-Path $taskScratch $taskName
    Invoke-WebRequest -UseBasicParsing -Uri $taskEntry.url -OutFile $taskDownload
    if ((Get-FileHash -LiteralPath $taskDownload -Algorithm SHA256).Hash -ne $taskEntry.sha256) { throw "Checksum mismatch: $taskName" }
    return $taskDownload
}
function Test-Tool([string]$taskName, [string]$taskHash) {
    $taskPath = Join-Path $taskTools $taskName
    return (Test-Path -LiteralPath $taskPath -PathType Leaf) -and ((Get-FileHash -LiteralPath $taskPath).Hash -eq $taskHash)
}
try {
    if (-not (Test-Tool 'ffmpeg.exe' $taskLock.ffmpeg.probe_sha256)) {
        $taskSevenZip = Get-Command 7z.exe -ErrorAction SilentlyContinue
        if (-not $taskSevenZip) { $taskSevenZip = Get-Item 'C:\Program Files\7-Zip\7z.exe' -ErrorAction SilentlyContinue }
        if (-not $taskSevenZip) { throw 'Install 7-Zip to extract the pinned FFmpeg source-build dependency.' }
        $taskArchive = Get-VerifiedArchive $taskLock.ffmpeg 'ffmpeg.7z'
        $taskExtractor = if ($taskSevenZip -is [IO.FileInfo]) { $taskSevenZip.FullName } else { $taskSevenZip.Source }
        & $taskExtractor x $taskArchive ('-o' + (Join-Path $taskScratch 'ffmpeg')) -y | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'FFmpeg extraction failed' }
        $taskFfmpeg = Get-ChildItem -LiteralPath (Join-Path $taskScratch 'ffmpeg') -Filter ffmpeg.exe -Recurse -File | Select-Object -First 1
        if (-not $taskFfmpeg) { throw 'Missing FFmpeg binary in archive' }
        Get-ChildItem -LiteralPath $taskFfmpeg.DirectoryName -File | Where-Object { $_.Name -in @('ffmpeg.exe','ffprobe.exe') -or $_.Extension -eq '.dll' } | Copy-Item -Destination $taskTools -Force
        if (-not (Test-Tool 'ffmpeg.exe' $taskLock.ffmpeg.probe_sha256)) { throw 'FFmpeg binary checksum mismatch' }
    }
    if (-not (Test-Tool 'yt-dlp.exe' $taskLock.ytdlp.sha256)) {
        Copy-Item -LiteralPath (Get-VerifiedArchive $taskLock.ytdlp 'yt-dlp.exe') -Destination (Join-Path $taskTools 'yt-dlp.exe') -Force
    }
    if (-not (Test-Tool 'uv.exe' $taskLock.uv.probe_sha256)) {
        $taskArchive = Get-VerifiedArchive $taskLock.uv 'uv.zip'
        Expand-Archive -LiteralPath $taskArchive -DestinationPath (Join-Path $taskScratch 'uv')
        $taskUv = Get-ChildItem -LiteralPath (Join-Path $taskScratch 'uv') -Filter uv.exe -File -Recurse | Select-Object -First 1
        Copy-Item -LiteralPath $taskUv.FullName -Destination (Join-Path $taskTools 'uv.exe') -Force
    }
    if (-not (Test-Tool 'node.exe' $taskLock.node.probe_sha256)) {
        $taskArchive = Get-VerifiedArchive $taskLock.node 'node.zip'
        Expand-Archive -LiteralPath $taskArchive -DestinationPath (Join-Path $taskScratch 'node')
        $taskNode = Get-ChildItem -LiteralPath (Join-Path $taskScratch 'node') -Filter node.exe -File -Recurse | Select-Object -First 1
        Copy-Item -LiteralPath $taskNode.FullName -Destination (Join-Path $taskTools 'node.exe') -Force
        Copy-Item -LiteralPath (Join-Path $taskNode.DirectoryName 'LICENSE') -Destination (Join-Path $taskTools 'NODE-LICENSE.txt') -Force
    }
    if (-not (Test-Path -LiteralPath (Join-Path $taskTools 'rhubarb\rhubarb.exe'))) { & (Join-Path $PSScriptRoot 'setup-rhubarb.ps1') }
    Write-Host 'Pinned FFmpeg, yt-dlp, uv, Node.js and Rhubarb are ready.'
} finally {
    if ([IO.Path]::GetFileName($taskScratch).StartsWith('AIR3view-tools-') -and [IO.Path]::GetDirectoryName($taskScratch) -eq [IO.Path]::GetTempPath().TrimEnd('\')) {
        Remove-Item -LiteralPath $taskScratch -Recurse -Force
    }
}
