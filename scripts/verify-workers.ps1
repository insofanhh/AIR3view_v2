param([Parameter(Mandatory=$true)][string]$PackagePath)
$ErrorActionPreference='Stop'
$taskPackage=(Resolve-Path -LiteralPath $PackagePath).Path
$taskScratch=Join-Path ([IO.Path]::GetTempPath()) ('AIR3view-worker-verify-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskScratch | Out-Null
$taskPython=Join-Path $taskPackage '.venv\Scripts\python.exe'
$taskPreviousPort=$env:VIENEU_PORT
$env:VIENEU_PORT='18765'
$taskProcess=$null
try {
    $taskListener=Get-NetTCPConnection -LocalPort 18765 -State Listen -ErrorAction SilentlyContinue
    if ($taskListener) { throw 'Verification port 18765 is already in use' }
    $taskWorker=Join-Path $taskPackage 'workers\vieneu_server.py'
    $taskProcess=Start-Process -FilePath $taskPython -ArgumentList ('"' + $taskWorker + '"') -WorkingDirectory $taskPackage -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskScratch 'stdout.log') -RedirectStandardError (Join-Path $taskScratch 'stderr.log') -PassThru
    $taskReady=$false
    for ($taskAttempt=0;$taskAttempt -lt 150;$taskAttempt++) {
        try { $taskHealth=Invoke-RestMethod 'http://127.0.0.1:18765/health' -TimeoutSec 2; $taskReady=$taskHealth.status -eq 'ready'; if ($taskReady) { break } } catch {}
        if ($taskProcess.HasExited) { Get-Content -LiteralPath (Join-Path $taskScratch 'stderr.log') -Tail 35; throw 'Fresh worker exited during startup' }
        if ($taskAttempt % 10 -eq 0) { Write-Host 'Waiting for the fresh VieNeu worker/model download...' }
        Start-Sleep -Seconds 2
    }
    if (-not $taskReady) { Get-Content -LiteralPath (Join-Path $taskScratch 'stderr.log') -Tail 35; throw 'Fresh worker startup timed out' }
    $taskExpectedHash=(Get-FileHash -LiteralPath $taskWorker -Algorithm SHA256).Hash
    if ($taskHealth.worker_sha256 -ne $taskExpectedHash) { throw 'Verification reached a different worker' }
    $taskHeaders=@{}
    if ($env:VIENEU_API_KEY) { $taskHeaders.Authorization='Bearer ' + $env:VIENEU_API_KEY }
    # ASCII source keeps this check valid on Windows PowerShell 5.1 code pages.
    $taskBody='{"input":"Xin ch\u00e0o, \u0111\u00e2y l\u00e0 video review c\u1ee7a AIR3view.","voice":"Minh Qu\u00e2n","language":"vi"}'
    $taskWav=Join-Path $taskScratch 'voice.wav'
    Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:18765/v1/audio/speech' -Method Post -Headers $taskHeaders -ContentType 'application/json' -Body ([Text.Encoding]::UTF8.GetBytes($taskBody)) -OutFile $taskWav -TimeoutSec 180
    $taskFfprobe=Join-Path $taskPackage 'tools\ffprobe.exe'
    $taskDuration=(& $taskFfprobe -v error -show_entries format=duration -of default=nw=1:nk=1 $taskWav).Trim()
    if ($LASTEXITCODE -ne 0 -or [double]$taskDuration -lt 0.1) { throw 'TTS produced invalid audio' }
    $taskCues=Join-Path $taskScratch 'cues.json'
    & (Join-Path $taskPackage 'tools\rhubarb\rhubarb.exe') -r phonetic -f json -o $taskCues $taskWav
    if ($LASTEXITCODE -ne 0) { throw 'Packaged Rhubarb failed' }
    $taskAvatar=Join-Path $taskScratch 'avatar.mkv'
    $taskJob=@{cues=$taskCues;output=$taskAvatar;ffmpeg=(Join-Path $taskPackage 'tools\ffmpeg.exe');duration_ms=[math]::Round([double]$taskDuration*1000);fps=25;size=512;character='portrait';emotion='curious';gesture='auto';narration='AIR3view local avatar verification';words=@()}
    $taskJobPath=Join-Path $taskScratch 'avatar.json'
    [IO.File]::WriteAllText($taskJobPath,($taskJob | ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
    & $taskPython (Join-Path $taskPackage 'workers\cartoon_render.py') --job $taskJobPath
    if ($LASTEXITCODE -ne 0) { throw 'Packaged cartoon worker failed' }
    & (Join-Path $taskPackage 'tools\ffmpeg.exe') -v error -i $taskAvatar -f null -
    if ($LASTEXITCODE -ne 0) { throw 'Generated avatar decode failed' }
    Write-Host ('Fresh worker verification PASS: real VieNeu WAV (' + $taskDuration + 's), phonetic lip sync, half-body portrait avatar and full decode.')
} finally {
    if ($taskProcess) {
        Get-CimInstance Win32_Process -Filter ('ParentProcessId=' + $taskProcess.Id) | Where-Object { $_.CommandLine -like ('*' + $taskPackage + '*vieneu_server.py*') } | ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }
        if (-not $taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id -ErrorAction SilentlyContinue }
    }
    $env:VIENEU_PORT=$taskPreviousPort
}
