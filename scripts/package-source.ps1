$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskVersion = ((Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1) -split '"')[1]
$taskFiles = @()
foreach ($taskName in @('.gitignore', '.gitattributes', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain.toml',
    'README.md', 'THIRD_PARTY_NOTICES.md', 'WHATLANG-LICENSE.txt', 'Start-AIR3view.cmd', 'Setup-AIR3view.cmd')) {
    $taskFiles += Get-Item -LiteralPath (Join-Path $taskRoot $taskName)
}
foreach ($taskFolder in @('src', 'assets', 'resources', 'scripts', '.github', '.cargo')) {
    $taskFiles += Get-ChildItem -LiteralPath (Join-Path $taskRoot $taskFolder) -File -Recurse
}
foreach ($taskName in @('vieneu_server.py', 'transcribe.py', 'cuda_runtime.py', 'requirements-cuda.txt', 'cartoon_render.py', 'gesture_rig.py',
    'requirements.txt', 'requirements.lock', 'PORTRAIT_ASSET.md', 'portrait-base.png', 'cartoon-portrait.png', 'cartoon-studio.png', 'cartoon-analyst.png')) {
    $taskFiles += Get-Item -LiteralPath (Join-Path $taskRoot ('workers\' + $taskName))
}
foreach ($taskName in @('FFMPEG-LICENSE.txt', 'FFMPEG-BUILDINFO.txt', 'yt-dlp-SHA2-256SUMS', 'runtime-lock.json', 'NODE-LICENSE.txt', 'UV-LICENSE-MIT.txt', 'UV-LICENSE-APACHE.txt')) {
    $taskFiles += Get-Item -LiteralPath (Join-Path $taskRoot ('tools\' + $taskName))
}
$taskDist = Join-Path $taskRoot 'dist'
New-Item -ItemType Directory -Force -Path $taskDist | Out-Null
$taskOutput = Join-Path $taskDist ('AIR3view-' + $taskVersion + '-source.zip')
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskStream = [IO.File]::Open($taskOutput, [IO.FileMode]::Create, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$taskZip = [IO.Compression.ZipArchive]::new($taskStream, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($taskFile in ($taskFiles | Sort-Object FullName -Unique)) {
        if ($taskFile.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Source archive cannot include links' }
        $taskRelative = $taskFile.FullName.Substring($taskRoot.Length + 1).Replace('\', '/')
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($taskZip, $taskFile.FullName, $taskRelative, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally { $taskZip.Dispose(); $taskStream.Dispose() }
Write-Host "Created $taskOutput ($($taskFiles.Count) files)."
