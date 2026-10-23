$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskPython = Join-Path $taskRoot '.venv\Scripts\python.exe'
if ($env:AIR3VIEW_PYTHON -and (Test-Path -LiteralPath $env:AIR3VIEW_PYTHON -PathType Leaf)) {
    $taskPython = $env:AIR3VIEW_PYTHON
}
if (-not (Test-Path -LiteralPath $taskPython)) {
    $taskConfig = Join-Path $taskRoot 'local-runtime.json'
    if (Test-Path -LiteralPath $taskConfig) {
        $taskPython = (Get-Content -LiteralPath $taskConfig -Raw | ConvertFrom-Json).python
    }
}
if (-not (Test-Path -LiteralPath $taskPython)) { throw 'Run scripts/setup-workers.ps1 first.' }
$taskPort = if ($env:VIENEU_PORT) { [int]$env:VIENEU_PORT } else { 8765 }
$taskHealthUrl = 'http://127.0.0.1:' + $taskPort + '/health'
$taskWorkerHash = (Get-FileHash -LiteralPath (Join-Path $taskRoot 'workers\vieneu_server.py') -Algorithm SHA256).Hash
try {
    $taskHealth = Invoke-RestMethod $taskHealthUrl -TimeoutSec 2
    if ($taskHealth.model -ne 'pnnbao-ump/VieNeu-TTS-v3-Turbo') { throw 'Port 8765 belongs to another service.' }
    if ($taskHealth.protocol -ge 3 -and $taskHealth.reference_audio -and $taskHealth.worker_sha256 -eq $taskWorkerHash) { Write-Host 'VieNeu is ready (reference voice supported).'; return }
    # Upgrade only this app's worker, never another Python service on the machine.
    $taskOwner = (Get-NetTCPConnection -LocalPort $taskPort -State Listen | Select-Object -First 1).OwningProcess
    $taskBackend = Get-CimInstance Win32_Process -Filter "ProcessId=$taskOwner"
    $taskParent = Get-CimInstance Win32_Process -Filter "ProcessId=$($taskBackend.ParentProcessId)"
    $taskKnownPython = [IO.Path]::GetFullPath($taskPython)
    $taskWorkerSuffix = 'workers[\\/]vieneu_server\.py'
    # A release may use a different virtualenv. A persistent worker identifies its
    # own PID/interpreter, then OS process metadata independently confirms both.
    $taskPreviousInstall = $taskHealth.app -eq 'AIR3view' -and $taskHealth.pid -eq $taskOwner -and
        $taskHealth.python -and ($taskParent.ExecutablePath -eq $taskHealth.python -or $taskBackend.ExecutablePath -eq $taskHealth.python)
    if ($taskBackend.CommandLine -notmatch $taskWorkerSuffix -or
        (-not $taskPreviousInstall -and $taskParent.ExecutablePath -ne $taskKnownPython -and $taskBackend.ExecutablePath -ne $taskKnownPython)) {
        throw 'Old VieNeu worker detected. Close that worker, then start this app again.'
    }
    Stop-Process -Id $taskOwner -ErrorAction Stop
    $taskExiting = Get-Process -Id $taskOwner -ErrorAction SilentlyContinue
    if ($taskExiting -and -not $taskExiting.WaitForExit(5000)) { throw 'Previous worker is still stopping. Retry in a moment.' }
    Write-Host 'Upgrading the AIR3view VieNeu worker.'
} catch {
    if ($taskHealth -or $_.Exception.Message -eq 'Port 8765 belongs to another service.') { throw }
}
$taskLogs = if ($env:REACTION_DATA_DIR) { Join-Path $env:REACTION_DATA_DIR 'logs' } else { Join-Path $env:LOCALAPPDATA 'ReactionStudio\logs' }
New-Item -ItemType Directory -Force -Path $taskLogs | Out-Null
$taskWorker = Join-Path $taskRoot 'workers\vieneu_server.py'
$taskProcess = Start-Process -FilePath $taskPython -ArgumentList @('"' + $taskWorker + '"') -WorkingDirectory $taskRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskLogs 'vieneu-out.log') -RedirectStandardError (Join-Path $taskLogs 'vieneu-err.log') -PassThru
Set-Content -LiteralPath (Join-Path $taskLogs 'vieneu.pid') -Value $taskProcess.Id
Write-Host 'VieNeu is starting. First launch downloads the model; logs: LocalAppData/ReactionStudio/logs.'
