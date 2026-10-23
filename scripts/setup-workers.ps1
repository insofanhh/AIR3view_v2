$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskUv = Join-Path $taskRoot 'tools\uv.exe'
if (-not (Test-Path -LiteralPath $taskUv)) { throw 'Missing tools/uv.exe. Run scripts/setup-tools.ps1 when building from source.' }
$taskPython = Join-Path $taskRoot '.venv\Scripts\python.exe'
$taskRequirements = Join-Path $taskRoot 'workers\requirements.lock'
$taskHash = (Get-FileHash -LiteralPath $taskRequirements -Algorithm SHA256).Hash
$taskMarker = Join-Path $taskRoot '.venv\.air3view-ready'
if ((Test-Path -LiteralPath $taskPython) -and (Test-Path -LiteralPath $taskMarker) -and
    (Get-Content -LiteralPath $taskMarker -Raw).Trim() -eq $taskHash) {
    Write-Host 'AIR3view Python runtime is ready.'
    return
}
if (-not $env:UV_PYTHON_INSTALL_DIR) { $env:UV_PYTHON_INSTALL_DIR = Join-Path $env:LOCALAPPDATA 'ReactionStudio\runtime\python' }
$env:UV_LINK_MODE = 'copy'
if (-not (Test-Path -LiteralPath $taskPython)) {
    Write-Host 'First run: downloading managed Python 3.12. No system Python or administrator access required.'
    & $taskUv venv --python 3.12 --managed-python --no-config (Join-Path $taskRoot '.venv')
    if ($LASTEXITCODE -ne 0) { throw 'Python setup failed. Check internet access, free disk space, then retry.' }
}
Write-Host 'Installing the pinned AIR3view worker dependencies. Please keep this window open.'
& $taskUv pip install --no-config --python $taskPython -r $taskRequirements
if ($LASTEXITCODE -ne 0) { throw 'Worker dependency installation failed. Retry setup to continue.' }
& $taskPython -c 'from vieneu import Vieneu; import faster_whisper, fastapi, uvicorn, PIL, soundfile; print("Worker imports: PASS")'
if ($LASTEXITCODE -ne 0) { throw 'Worker runtime import check failed' }
Set-Content -LiteralPath $taskMarker -Value $taskHash -Encoding ASCII
Write-Host 'Runtime ready. Voice and transcription models download separately on first use.'
