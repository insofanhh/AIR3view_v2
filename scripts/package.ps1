param([switch]$SkipBuild, [switch]$Archive, [string]$ReleaseTag, [string]$OutputPath)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskVersion = ((Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1) -split '"')[1]
if (-not $ReleaseTag) { $ReleaseTag = 'v' + $taskVersion }
if ($ReleaseTag -notmatch '^v[0-9A-Za-z.+-]+$') { throw 'Invalid release tag' }
if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.cmd') build --release --bins --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
}
$taskDist = if ($OutputPath) { [IO.Path]::GetFullPath($OutputPath) } else { Join-Path $taskRoot 'dist\AIR3view' }
New-Item -ItemType Directory -Force -Path $taskDist | Out-Null
if ((Get-Item -LiteralPath $taskDist).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package destination cannot be a link' }
$taskFiles = [ordered]@{
    'target\x86_64-pc-windows-msvc\release\reaction-studio.exe' = 'AIR3view.exe'
    'target\x86_64-pc-windows-msvc\release\studio-cli.exe' = 'studio-cli.exe'
}
foreach ($taskFile in @('Start-AIR3view.cmd', 'Setup-AIR3view.cmd', 'README.md', 'THIRD_PARTY_NOTICES.md', 'WHATLANG-LICENSE.txt',
    'scripts\start-worker.ps1', 'scripts\setup-workers.ps1', 'scripts\create-shortcuts.ps1',
    'workers\vieneu_server.py', 'workers\transcribe.py', 'workers\cartoon_render.py', 'workers\gesture_rig.py',
    'workers\requirements.txt', 'workers\requirements.lock', 'workers\PORTRAIT_ASSET.md',
    'workers\portrait-base.png', 'workers\cartoon-portrait.png', 'workers\cartoon-studio.png', 'workers\cartoon-analyst.png')) {
    $taskFiles[$taskFile] = $taskFile
}
foreach ($taskFile in Get-ChildItem -LiteralPath (Join-Path $taskRoot 'tools') -File) {
    if ($taskFile.Extension -in @('.exe', '.dll', '.txt') -or $taskFile.Name -in @('yt-dlp-SHA2-256SUMS','runtime-lock.json')) {
        $taskRelative = 'tools\' + $taskFile.Name
        $taskFiles[$taskRelative] = $taskRelative
    }
}
foreach ($taskRequired in @('tools\ffmpeg.exe', 'tools\ffprobe.exe', 'tools\yt-dlp.exe', 'tools\rhubarb\rhubarb.exe', 'tools\uv.exe', 'tools\node.exe', 'tools\NODE-LICENSE.txt', 'tools\UV-LICENSE-MIT.txt', 'tools\UV-LICENSE-APACHE.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $taskRoot $taskRequired) -PathType Leaf)) { throw "Missing runtime: $taskRequired" }
}
foreach ($taskFile in Get-ChildItem -LiteralPath (Join-Path $taskRoot 'tools\rhubarb') -File -Recurse) {
    $taskRelative = $taskFile.FullName.Substring($taskRoot.Length + 1)
    $taskFiles[$taskRelative] = $taskRelative
}
# Reject unexpected leftovers instead of silently shipping stale docs, credentials or backups.
$taskAllowed = @($taskFiles.Values) + @('package-manifest.json','release.json')
foreach ($taskItem in Get-ChildItem -LiteralPath $taskDist -Force -Recurse) {
    if ($taskItem.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package contains a link' }
    if (-not $taskItem.PSIsContainer) {
        $taskRelative = $taskItem.FullName.Substring($taskDist.Length + 1)
        if ($taskRelative -notin $taskAllowed) { throw "Unexpected package file: $taskRelative. Use an empty dist/AIR3view directory." }
    }
}
foreach ($taskPair in $taskFiles.GetEnumerator()) {
    $taskSource = Join-Path $taskRoot $taskPair.Key
    $taskTarget = Join-Path $taskDist $taskPair.Value
    if (-not (Test-Path -LiteralPath $taskSource -PathType Leaf)) { throw "Missing package input: $($taskPair.Key)" }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskTarget) | Out-Null
    if ((Test-Path -LiteralPath $taskTarget) -and
        (Get-FileHash -LiteralPath $taskSource).Hash -eq (Get-FileHash -LiteralPath $taskTarget).Hash) { continue }
    Copy-Item -LiteralPath $taskSource -Destination $taskTarget -Force
}
$taskCommit = git -C $taskRoot rev-parse HEAD 2>$null
if ($LASTEXITCODE -ne 0) { throw 'Commit source before creating a release package.' }
$taskBuildInfo = @{version=$taskVersion;tag=$ReleaseTag;commit=$taskCommit;built_at=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')} | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path $taskDist 'release.json'), $taskBuildInfo, [Text.UTF8Encoding]::new($false))
$taskManifest = @(foreach ($taskRelative in (@($taskFiles.Values) + @('release.json'))) {
    $taskFile = Get-Item -LiteralPath (Join-Path $taskDist $taskRelative)
    [pscustomobject]@{path=$taskRelative.Replace('\','/'); bytes=$taskFile.Length; sha256=(Get-FileHash -LiteralPath $taskFile.FullName).Hash}
})
$taskManifest | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $taskDist 'package-manifest.json') -Encoding UTF8
if ($Archive) {
    $taskZip = Join-Path $taskRoot ('dist\AIR3view-' + $ReleaseTag.TrimStart('v') + '-windows-x64.zip')
    Compress-Archive -Path (Join-Path $taskDist '*') -DestinationPath $taskZip -Force
    Write-Host "Archive: $taskZip"
}
Write-Host "Created $taskDist ($($taskManifest.Count) runtime files). Open AIR3view.exe. First run prepares Python/workers and creates Desktop/Start Menu icons."
