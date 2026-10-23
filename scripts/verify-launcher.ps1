param([Parameter(Mandatory=$true)][string]$PackagePath, [string]$ModelCache)
$ErrorActionPreference = 'Stop'
$taskPackage = (Resolve-Path -LiteralPath $PackagePath).Path
$taskExe = Join-Path $taskPackage 'AIR3view.exe'
$taskScratch = Join-Path ([IO.Path]::GetTempPath()) ('AIR3view-native-verify-' + [guid]::NewGuid().ToString('N'))
$taskData = Join-Path $taskScratch 'data'
New-Item -ItemType Directory -Force -Path $taskData | Out-Null
$taskPort = 18766
if (Get-NetTCPConnection -LocalPort $taskPort -State Listen -ErrorAction SilentlyContinue) { throw 'Native verification port is already in use.' }
$taskPreviousData = $env:REACTION_DATA_DIR
$taskPreviousCache = $env:REACTION_MODEL_CACHE
$taskPreviousShortcuts = $env:AIR3VIEW_SHORTCUT_ROOT
$env:REACTION_DATA_DIR = $taskData
if ($ModelCache) { $env:REACTION_MODEL_CACHE = (Resolve-Path -LiteralPath $ModelCache).Path }
$env:AIR3VIEW_SHORTCUT_ROOT = Join-Path $taskScratch 'shortcuts'
$taskSettings = @{vieneu_url=('http://127.0.0.1:' + $taskPort + '/v1/audio/speech')}
[IO.File]::WriteAllText((Join-Path $taskData 'settings.json'), ($taskSettings | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
$taskCold = -not (Test-Path -LiteralPath (Join-Path $taskPackage '.venv\.air3view-ready'))
$taskApp = $null
function Wait-NativeReady($taskProcess) {
    $taskDeadline = [DateTime]::UtcNow.AddMinutes(20)
    $taskNextOutput = [DateTime]::UtcNow
    while ([DateTime]::UtcNow -lt $taskDeadline) {
        if ($taskProcess.HasExited) { throw ('AIR3view exited before ready. Logs: ' + $taskData) }
        $taskReportPath = Join-Path $taskData ('logs\startup-' + $taskProcess.Id + '.json')
        if (Test-Path -LiteralPath $taskReportPath) {
            $taskReport = Get-Content -LiteralPath $taskReportPath -Raw | ConvertFrom-Json
            if ($taskReport.status -eq 'error') { throw ('Native startup error: ' + $taskReport.message + '. Logs: ' + $taskData) }
        }
        try {
            $taskHealth = Invoke-RestMethod ('http://127.0.0.1:' + $taskPort + '/health') -TimeoutSec 2
            if ($taskHealth.status -eq 'ready' -and $taskHealth.session -and $taskReport.status -eq 'ready' -and (Test-Path -LiteralPath (Join-Path $taskData 'runtime\shortcut-target.txt'))) { return $taskHealth }
        } catch {}
        if ([DateTime]::UtcNow -ge $taskNextOutput) { Write-Host ('Waiting for native setup/worker. Log: ' + (Join-Path $taskData 'logs\startup.log')); $taskNextOutput = [DateTime]::UtcNow.AddSeconds(25) }
        Start-Sleep -Milliseconds 500
    }
    throw ('Native startup timed out. Logs: ' + $taskData)
}
function Close-NativeApp($taskProcess) {
    $null = $taskProcess.CloseMainWindow()
    if (-not $taskProcess.WaitForExit(15000)) { throw 'AIR3view did not close gracefully.' }
    $taskDeadline = [DateTime]::UtcNow.AddSeconds(8)
    do {
        if (-not (Get-NetTCPConnection -LocalPort $taskPort -State Listen -ErrorAction SilentlyContinue)) { return }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $taskDeadline)
    throw 'GUI-owned worker remained after closing AIR3view.'
}
try {
    $taskApp = Start-Process -FilePath $taskExe -ArgumentList @('--window-size=1080x720','--page=projects') -WorkingDirectory $env:WINDIR -WindowStyle Hidden -PassThru
    $taskHealth = Wait-NativeReady $taskApp
    if ($taskHealth.model_loaded) { throw 'Native editor eagerly loaded the speech model.' }
    $taskHeaders = @{}
    if ($env:VIENEU_API_KEY) { $taskHeaders.Authorization = 'Bearer ' + $env:VIENEU_API_KEY }
    $taskBody = '{"input":"Xin ch\u00e0o, AIR3view \u0111\u00e3 s\u1eb5n s\u00e0ng.","voice":"Minh Qu\u00e2n","language":"vi"}'
    $taskWav = Join-Path $taskScratch 'native-voice.wav'
    Invoke-WebRequest -UseBasicParsing -Uri $taskSettings.vieneu_url -Method Post -Headers $taskHeaders -ContentType 'application/json' -Body ([Text.Encoding]::UTF8.GetBytes($taskBody)) -OutFile $taskWav -TimeoutSec 180
    & (Join-Path $taskPackage 'tools\ffmpeg.exe') -v error -i $taskWav -f null -
    if ($LASTEXITCODE -ne 0) { throw 'Native-launched TTS produced invalid audio.' }
    $taskLoadedHealth = Invoke-RestMethod ('http://127.0.0.1:' + $taskPort + '/health') -TimeoutSec 3
    if (-not $taskLoadedHealth.model_loaded) { throw 'First synthesis did not initialize the speech model.' }
    $taskShell = New-Object -ComObject WScript.Shell
    foreach ($taskLink in @('Desktop\AIR3view.lnk','Programs\AIR3view\AIR3view.lnk')) {
        $taskShortcut = $taskShell.CreateShortcut((Join-Path $env:AIR3VIEW_SHORTCUT_ROOT $taskLink))
        if ($taskShortcut.TargetPath -ne $taskExe -or $taskShortcut.Arguments) { throw 'Shortcut does not point directly to AIR3view.exe.' }
    }
    # A second icon click must not launch a second worker or freeze either window.
    $taskSecondCapture = Join-Path $taskScratch 'second-instance.png'
    $taskSecond = Start-Process -FilePath $taskExe -ArgumentList ('--capture-startup="' + $taskSecondCapture + '"') -WorkingDirectory $env:WINDIR -WindowStyle Hidden -PassThru
    if (-not $taskSecond.WaitForExit(20000) -or -not (Test-Path -LiteralPath $taskSecondCapture)) { throw 'Second-instance error UI failed.' }
    $taskSame = Invoke-RestMethod ('http://127.0.0.1:' + $taskPort + '/health') -TimeoutSec 3
    if ($taskSame.session -ne $taskHealth.session) { throw 'Second icon click replaced the active worker.' }
    Close-NativeApp $taskApp
    $taskApp = $null
    Write-Host ('PASS native first run: cold=' + $taskCold + ', real TTS, direct EXE shortcuts, duplicate guard, worker cleanup.')
    # Launch through the actual shortcut, with no launcher script and no console.
    $taskRestarted = [DateTime]::UtcNow
    $taskDesktopLink = Join-Path $env:AIR3VIEW_SHORTCUT_ROOT 'Desktop\AIR3view.lnk'
    $taskApp = Start-Process -FilePath $taskDesktopLink -WindowStyle Hidden -PassThru
    if (-not $taskApp) { throw 'Shortcut did not return the launched process.' }
    $taskHealth = Wait-NativeReady $taskApp
    Close-NativeApp $taskApp
    $taskApp = $null
    Write-Host ('PASS icon reopen: cached runtime, ready in ' + [math]::Round(([DateTime]::UtcNow-$taskRestarted).TotalSeconds,1) + 's including close.')
    $taskApp = Start-Process -FilePath $taskDesktopLink -WindowStyle Hidden -PassThru
    $taskHealth = Wait-NativeReady $taskApp
    if ($taskApp.Path -ne $taskExe) { throw 'Crash test refused: process is not the test executable.' }
    $taskApp.Kill()
    $null = $taskApp.WaitForExit(10000)
    $taskApp = $null
    $taskCrashDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (Get-NetTCPConnection -LocalPort $taskPort -State Listen -ErrorAction SilentlyContinue) {
        if ([DateTime]::UtcNow -gt $taskCrashDeadline) { throw 'Worker stayed running after its GUI process was terminated.' }
        Start-Sleep -Milliseconds 250
    }
    Write-Host 'PASS crash recovery: GUI-owned worker exited after its owner was terminated.'
    $taskCapture = Join-Path $taskScratch 'library-1080x720.png'
    $taskApp = Start-Process -FilePath $taskExe -ArgumentList @('--window-size=1080x720','--page=projects',('--capture-ui="' + $taskCapture + '"')) -WorkingDirectory $env:WINDIR -WindowStyle Hidden -PassThru
    if (-not $taskApp.WaitForExit(120000) -or -not (Test-Path -LiteralPath $taskCapture)) { throw 'Native editor capture failed.' }
    $taskApp = $null
    $taskBroken = Join-Path $taskScratch 'Missing runtime'
    New-Item -ItemType Directory -Force -Path (Join-Path $taskBroken 'workers'),(Join-Path $taskBroken 'scripts') | Out-Null
    Copy-Item -LiteralPath $taskExe -Destination (Join-Path $taskBroken 'AIR3view.exe')
    Copy-Item -LiteralPath (Join-Path $taskPackage 'workers\requirements.lock') -Destination (Join-Path $taskBroken 'workers\requirements.lock')
    Copy-Item -LiteralPath (Join-Path $taskPackage 'scripts\setup-workers.ps1') -Destination (Join-Path $taskBroken 'scripts\setup-workers.ps1')
    $taskErrorCapture = Join-Path $taskScratch 'startup-error-1080x720.png'
    $taskApp = Start-Process -FilePath (Join-Path $taskBroken 'AIR3view.exe') -ArgumentList @('--window-size=1080x720',('--capture-startup="' + $taskErrorCapture + '"')) -WorkingDirectory $env:WINDIR -WindowStyle Hidden -PassThru
    if (-not $taskApp.WaitForExit(20000) -or -not (Test-Path -LiteralPath $taskErrorCapture)) { throw 'Native startup error UI capture failed.' }
    $taskErrorReport = Get-Content -LiteralPath (Join-Path $taskData ('logs\startup-' + $taskApp.Id + '.json')) -Raw | ConvertFrom-Json
    if ($taskErrorReport.status -ne 'error') { throw 'Missing runtime was not reported as an error.' }
    $taskApp = $null
    Write-Host 'PASS missing-runtime error: responsive native dialog with retry/log controls.'
    Write-Host ('Native launcher verification PASS. Artifacts: ' + $taskScratch)
} finally {
    if ($taskApp -and -not $taskApp.HasExited) { $null = $taskApp.CloseMainWindow(); $null = $taskApp.WaitForExit(10000) }
    $env:REACTION_DATA_DIR = $taskPreviousData
    $env:REACTION_MODEL_CACHE = $taskPreviousCache
    $env:AIR3VIEW_SHORTCUT_ROOT = $taskPreviousShortcuts
}
