param([Parameter(Mandatory=$true)][string]$PackagePath)
$ErrorActionPreference = 'Stop'
$taskPackage = (Resolve-Path -LiteralPath $PackagePath).Path
$taskScratch = Join-Path ([IO.Path]::GetTempPath()) ('AIR3view-verify-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskScratch | Out-Null
$taskPreviousData = $env:REACTION_DATA_DIR
$env:REACTION_DATA_DIR = Join-Path $taskScratch 'data'
try {
    $taskManifest = Get-Content -LiteralPath (Join-Path $taskPackage 'package-manifest.json') -Raw | ConvertFrom-Json
    foreach ($taskEntry in $taskManifest) {
        $taskFile = [IO.Path]::GetFullPath((Join-Path $taskPackage $taskEntry.path))
        if (-not $taskFile.StartsWith($taskPackage + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid manifest path' }
        if ((Get-FileHash -LiteralPath $taskFile -Algorithm SHA256).Hash -ne $taskEntry.sha256) { throw "Package hash mismatch: $($taskEntry.path)" }
    }
    $taskBuild = Get-Content -LiteralPath (Join-Path $taskPackage 'release.json') -Raw | ConvertFrom-Json
    if ($taskBuild.commit -notmatch '^[0-9a-f]{40}$') { throw 'Missing release commit' }
    $taskFfmpeg = Join-Path $taskPackage 'tools\ffmpeg.exe'
    $taskFfprobe = Join-Path $taskPackage 'tools\ffprobe.exe'
    $taskCli = Join-Path $taskPackage 'studio-cli.exe'
    $taskInput = Join-Path $taskScratch 'source.mp4'
    & $taskFfmpeg -y -v error -f lavfi -i 'testsrc2=size=640x360:rate=24:duration=6' -f lavfi -i 'sine=frequency=440:sample_rate=48000:duration=6' -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -shortest $taskInput
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic source creation failed' }
    foreach ($taskPortrait in @($true,$false)) {
        $taskProject = @{
            id='release-verification';title='AIR3view packaged renderer';root=$taskScratch
            rules=@{min_seconds=5;max_seconds=7;hook_enabled=$false;intercuts_enabled=$false}
            output=@{portrait=$taskPortrait;resolution=360;fps=24}
            subtitles=@{enabled=$true;size=24;bottom_margin=55}
            sources=@(@{id='src1';path=$taskInput;duration_ms=6000;width=640;height=360;has_audio=$true})
            segments=@(
                @{id='scene1';kind='source';source_id='src1';in_ms=0;out_ms=3000;duration_ms=3000;caption='AIR3view release';audio_gain=1.0},
                @{id='scene2';kind='source';source_id='src1';in_ms=3000;out_ms=6000;duration_ms=3000;caption='Video and audio verified';audio_gain=1.0}
            )
        }
        $taskProjectPath=Join-Path $taskScratch 'project.json'
        [IO.File]::WriteAllText($taskProjectPath,($taskProject | ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
        $taskOutput=Join-Path $taskScratch ('output-' + $taskPortrait + '.mp4')
        & $taskCli render $taskProjectPath $taskOutput
        if ($LASTEXITCODE -ne 0) { throw 'Packaged render failed' }
        & $taskFfmpeg -v error -i $taskOutput -f null -
        if ($LASTEXITCODE -ne 0) { throw 'Full video decode failed' }
        $taskProbe=& $taskFfprobe -v error -show_entries 'stream=codec_name,codec_type,width,height:format=duration' -of json $taskOutput | ConvertFrom-Json
        $taskVideo=$taskProbe.streams | Where-Object codec_type -eq 'video'
        $taskAudio=$taskProbe.streams | Where-Object codec_type -eq 'audio'
        if (-not $taskAudio -or [math]::Abs([double]$taskProbe.format.duration-6) -gt 0.2) { throw 'Incorrect output duration/audio' }
        if ($taskPortrait -and ($taskVideo.width -ne 360 -or $taskVideo.height -ne 640)) { throw 'Incorrect portrait dimensions' }
        if (-not $taskPortrait -and ($taskVideo.width -ne 640 -or $taskVideo.height -ne 360)) { throw 'Incorrect landscape dimensions' }
        Write-Host ('PASS render/decode: {0}x{1}, audio={2}, duration={3}' -f $taskVideo.width,$taskVideo.height,$taskAudio.codec_name,$taskProbe.format.duration)
    }
    Write-Host ('Package verification PASS: ' + $taskManifest.Count + ' hashes, both aspect ratios, full audio/video decode.')
} finally { $env:REACTION_DATA_DIR=$taskPreviousData }
