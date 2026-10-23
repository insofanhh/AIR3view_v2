param([Parameter(Mandatory=$true)][string]$AppPath)
$ErrorActionPreference = 'Stop'
$taskExe = (Resolve-Path -LiteralPath $AppPath).Path
if ([IO.Path]::GetExtension($taskExe) -ne '.exe') { throw 'Shortcut target must be the AIR3view executable.' }
$taskShell = New-Object -ComObject WScript.Shell
$taskDesktop = [Environment]::GetFolderPath('DesktopDirectory')
$taskPrograms = Join-Path ([Environment]::GetFolderPath('Programs')) 'AIR3view'
if ($env:AIR3VIEW_SHORTCUT_ROOT) {
    $taskDesktop = Join-Path $env:AIR3VIEW_SHORTCUT_ROOT 'Desktop'
    $taskPrograms = Join-Path $env:AIR3VIEW_SHORTCUT_ROOT 'Programs\AIR3view'
}
New-Item -ItemType Directory -Force -Path $taskDesktop | Out-Null
New-Item -ItemType Directory -Force -Path $taskPrograms | Out-Null
foreach ($taskFolder in @($taskDesktop, $taskPrograms)) {
    $taskLinkPath = Join-Path $taskFolder 'AIR3view.lnk'
    $taskLink = $taskShell.CreateShortcut($taskLinkPath)
    $taskLink.TargetPath = $taskExe
    $taskLink.WorkingDirectory = Split-Path -Parent $taskExe
    $taskLink.IconLocation = $taskExe + ',0'
    $taskLink.Description = 'AIR3view - AI video review editor'
    $taskLink.Save()
    Write-Host ('Created shortcut: ' + $taskLinkPath)
}
