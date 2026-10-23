param([switch]$Apply, [switch]$IncludeLegacyPackage)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
# Fixed artifact roots only. Never include .venv, tools, AppData, or user exports.
$taskRoots = @('test-output', 'tests', 'workers\__pycache__')
if ($IncludeLegacyPackage) { $taskRoots += 'dist\ReactionStudio' }
$taskEntries = @()
foreach ($taskRelative in $taskRoots) {
    $taskTarget = [IO.Path]::GetFullPath((Join-Path $taskRoot $taskRelative))
    if (-not $taskTarget.StartsWith($taskRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup path outside workspace' }
    if (-not (Test-Path -LiteralPath $taskTarget)) { continue }
    $taskEntries += Get-Item -LiteralPath $taskTarget -Force
    $taskEntries += Get-ChildItem -LiteralPath $taskTarget -Force -Recurse
}
if ($taskEntries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'Refusing cleanup containing filesystem links' }
$taskFiles = @($taskEntries | Where-Object { -not $_.PSIsContainer })
Write-Host ('Artifacts: {0} files, {1:N2} MiB (logical size)' -f $taskFiles.Count, (($taskFiles | Measure-Object Length -Sum).Sum / 1MB))
Write-Host ('Roots: ' + ($taskRoots -join ', '))
if (-not $Apply) { Write-Host 'Preview only. Add -Apply to delete these artifacts; Cargo build cache is cleaned separately with cargo clean.'; return }
$taskAppPrefix = Join-Path $taskRoot 'dist\ReactionStudio\'
if ($IncludeLegacyPackage) {
    $taskRunning = Get-Process -Name AIR3view,ReactionStudio,studio-cli -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($taskAppPrefix, [StringComparison]::OrdinalIgnoreCase) }
    if ($taskRunning) { throw 'Close AIR3view from the legacy package before cleanup. The app is not stopped automatically.' }
}
foreach ($taskFile in $taskFiles) { Remove-Item -LiteralPath $taskFile.FullName -Force -ErrorAction Stop }
foreach ($taskDirectory in ($taskEntries | Where-Object PSIsContainer | Sort-Object { $_.FullName.Length } -Descending)) {
    Remove-Item -LiteralPath $taskDirectory.FullName -Force -ErrorAction Stop
}
Write-Host 'Artifact cleanup complete.'
