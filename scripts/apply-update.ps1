param([Parameter(Mandatory=$true)][string]$JobFile, [ValidateSet('Prepare','Install')][string]$Mode = 'Install')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskJob = Get-Content -LiteralPath $JobFile -Raw -Encoding UTF8 | ConvertFrom-Json
$taskRoot = [IO.Path]::GetFullPath($taskJob.root).TrimEnd('\')
$taskStage = [IO.Path]::GetFullPath($taskJob.stage).TrimEnd('\')
$taskPayload = Join-Path $taskStage 'payload'
$taskBackup = Join-Path $taskStage 'backup'
$taskApplied = New-Object 'System.Collections.Generic.List[object]'
$taskLock = $null
$taskWaiting = $false

function Assert-NoLink([string]$Path) {
    $taskCursor = [IO.Path]::GetFullPath($Path)
    while ($taskCursor) {
        if (Test-Path -LiteralPath $taskCursor) {
            if ((Get-Item -LiteralPath $taskCursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Link/junction rejected: $taskCursor" }
        }
        $taskCursor = Split-Path -Parent $taskCursor
    }
}
function Get-SafePath([string]$Base, [string]$Relative) {
    $taskRelative = $Relative.Replace('\','/')
    if (-not $taskRelative -or $taskRelative -match '[:\x00-\x1f]' -or $taskRelative.StartsWith('/') -or
        @($taskRelative.Split('/') | Where-Object { $_ -in @('','.','..') -or $_ -match '[. ]$|[<>"|?*]' -or $_ -match '^(?i:CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(\.|$)' }).Count) { throw "Unsafe package path: $Relative" }
    $taskResolved = [IO.Path]::GetFullPath((Join-Path $Base $taskRelative))
    if (-not $taskResolved.StartsWith($Base.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw "Path escapes package: $Relative" }
    Assert-NoLink $taskResolved
    return $taskResolved
}
function Assert-Managed([string]$Relative) {
    if ($Relative -notmatch '^(tools|workers|scripts)/' -and $Relative -notin @('AIR3view.exe','studio-cli.exe','Start-AIR3view.cmd','Setup-AIR3view.cmd','README.md','THIRD_PARTY_NOTICES.md','WHATLANG-LICENSE.txt','release.json','package-manifest.json')) {
        throw "Package attempted to change user/runtime data: $Relative"
    }
}
function Write-Result([string]$Status, [string]$Message) {
    $taskResult = @{status=$Status;message=$Message;tag=$taskJob.tag;stage=$taskStage;time=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json
    [IO.File]::WriteAllText($taskJob.result, $taskResult, [Text.UTF8Encoding]::new($false))
}
function Read-Package {
    $taskManifest = Get-Content -LiteralPath (Join-Path $taskPayload 'package-manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $taskManifest = @($taskManifest)
    if ($taskManifest.Count -lt 3 -or $taskManifest.Count -gt 20000) { throw 'Invalid manifest size' }
    $taskNames = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    foreach ($taskEntry in $taskManifest) {
        $taskRelative = ([string]$taskEntry.path).Replace('\','/')
        Assert-Managed $taskRelative
        $taskFile = Get-SafePath $taskPayload $taskRelative
        $null = Get-SafePath $taskRoot $taskRelative
        if (-not $taskNames.Add($taskRelative) -or $taskRelative -eq 'package-manifest.json') { throw "Duplicate/reserved manifest path: $taskRelative" }
        if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf) -or (Get-Item -LiteralPath $taskFile).Length -ne $taskEntry.bytes -or
            $taskEntry.sha256 -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash -LiteralPath $taskFile -Algorithm SHA256).Hash -ne $taskEntry.sha256) { throw "File verification failed: $taskRelative" }
    }
    foreach ($taskRequired in @('AIR3view.exe','studio-cli.exe','release.json','workers/requirements.lock','scripts/setup-workers.ps1','tools/uv.exe')) {
        if (-not $taskNames.Contains($taskRequired)) { throw "Missing required runtime: $taskRequired" }
    }
    $taskInfo = Get-Content -LiteralPath (Join-Path $taskPayload 'release.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($taskInfo.tag -ne $taskJob.tag) { throw 'Release tag mismatch' }
    foreach ($taskFile in Get-ChildItem -LiteralPath $taskPayload -File -Recurse -Force) {
        $taskRelative = $taskFile.FullName.Substring($taskPayload.Length+1).Replace('\','/')
        if ($taskRelative -ne 'package-manifest.json' -and -not $taskNames.Contains($taskRelative)) { throw "Unlisted package file: $taskRelative" }
    }
    return $taskManifest
}
function Set-File([string]$Source, [string]$Destination, [bool]$Restore = $false) {
    Assert-NoLink $Destination
    $taskTemp = $Destination + '.air3view-new'
    if (Test-Path -LiteralPath $taskTemp) { throw "Pending replacement exists: $taskTemp" }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
    try {
        if ($Restore) { [IO.File]::Move($Source, $taskTemp) }
        else { [IO.File]::Copy($Source, $taskTemp, $false) }
        $taskDeadline = [DateTime]::UtcNow.AddSeconds(20)
        while ($true) {
            try {
                if (Test-Path -LiteralPath $Destination -PathType Leaf) { [IO.File]::Replace($taskTemp, $Destination, [NullString]::Value) }
                else { [IO.File]::Move($taskTemp, $Destination) }
                break
            } catch {
                if ([DateTime]::UtcNow -gt $taskDeadline) { throw }
                Start-Sleep -Milliseconds 200
            }
        }
    } finally {
        if (Test-Path -LiteralPath $taskTemp) {
            if ($Restore) { [IO.File]::Move($taskTemp, $Source) }
            else { Remove-Item -LiteralPath $taskTemp -Force }
        }
    }
}
try {
    Assert-NoLink $taskRoot
    Assert-NoLink $taskStage
    if ((Split-Path -Parent $taskStage) -ne $taskRoot -or (Split-Path -Leaf $taskStage) -notmatch '^\.air3view-update-[0-9a-f-]{36}$' -or
        [IO.Path]::GetFullPath($JobFile) -ne (Join-Path $taskStage 'job.json')) { throw 'Invalid staging directory' }
    if ($Mode -eq 'Prepare') {
        if (Test-Path -LiteralPath $taskPayload) { throw 'Payload already exists' }
        New-Item -ItemType Directory -Path $taskPayload | Out-Null
        $taskZip = [IO.Compression.ZipFile]::OpenRead((Join-Path $taskStage 'release.zip'))
        try {
            $taskSeen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
            [long]$taskExpanded = 0
            if ($taskZip.Entries.Count -gt 22000) { throw 'Too many ZIP entries' }
            foreach ($taskEntry in $taskZip.Entries) {
                $taskRelative = $taskEntry.FullName.Replace('\','/')
                $taskDirectory = $taskRelative.EndsWith('/')
                $taskRelative = $taskRelative.TrimEnd('/')
                $taskFile = Get-SafePath $taskPayload $taskRelative
                if (-not $taskSeen.Add($taskRelative) -or (($taskEntry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'ZIP duplicate/link rejected' }
                $taskExpanded += $taskEntry.Length
                if ($taskExpanded -gt 4GB) { throw 'Expanded ZIP exceeds 4 GB' }
                if ($taskDirectory) { New-Item -ItemType Directory -Force -Path $taskFile | Out-Null }
                else {
                    Assert-Managed $taskRelative
                    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskFile) | Out-Null
                    [IO.Compression.ZipFileExtensions]::ExtractToFile($taskEntry, $taskFile, $false)
                }
            }
        } finally { $taskZip.Dispose() }
        $null = Read-Package
        Write-Host 'Package verified; current installation unchanged.'
        exit 0
    }
    # Serialize installers; hold the lock until files have been committed/rolled back.
    $taskLock = [IO.File]::Open((Join-Path $taskRoot '.air3view-update.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $taskManifest = @(Read-Package)
    $taskOwner = Get-Process -Id $taskJob.parent_pid -ErrorAction SilentlyContinue
    if (-not $taskOwner -or $taskOwner.Path -ne $taskJob.old_exe) { throw 'AIR3view owner process changed; refusing update' }
    [IO.File]::WriteAllText((Join-Path $taskStage 'waiting'), 'ready')
    $taskWaiting = $true
    if (-not $taskOwner.WaitForExit(120000)) { throw 'AIR3view did not close within two minutes; update cancelled' }
    $taskChanges = @($taskManifest) + @([pscustomobject]@{path='package-manifest.json';sha256=(Get-FileHash -LiteralPath (Join-Path $taskPayload 'package-manifest.json')).Hash})
    # EXE last: all supporting files exist before the new application becomes visible.
    $taskChanges = @($taskChanges | Sort-Object @{Expression={if ($_.path -eq 'AIR3view.exe') { 1 } else { 0 }}})
    $taskOperations = @()
    foreach ($taskEntry in $taskChanges) {
        $taskRelative = ([string]$taskEntry.path).Replace('\','/')
        $taskDestination = Get-SafePath $taskRoot $taskRelative
        $taskExisted = Test-Path -LiteralPath $taskDestination -PathType Leaf
        if ($taskExisted -and (Get-FileHash -LiteralPath $taskDestination).Hash -eq $taskEntry.sha256) { continue }
        $taskSaved = Get-SafePath $taskBackup $taskRelative
        if ($taskExisted) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskSaved) | Out-Null
            [IO.File]::Copy($taskDestination, $taskSaved, $false)
        }
        $taskOperations += [pscustomobject]@{path=$taskRelative;existed=$taskExisted}
    }
    # Durable journal and all backups precede any replacement.
    [IO.File]::WriteAllText((Join-Path $taskStage 'journal.json'), (ConvertTo-Json -InputObject @($taskOperations) -Depth 4))
    foreach ($taskOperation in $taskOperations) {
        Set-File (Get-SafePath $taskPayload $taskOperation.path) (Get-SafePath $taskRoot $taskOperation.path)
        $taskApplied.Add($taskOperation)
    }
    Write-Result 'success' ('Đã cập nhật ' + $taskJob.tag + '. Dự án và cài đặt được giữ nguyên.')
    $taskLock.Dispose(); $taskLock = $null
    $null = Start-Process -FilePath (Join-Path $taskRoot 'AIR3view.exe') -WorkingDirectory $taskRoot -PassThru
    # Only these verified staging paths are removed, never installation/user data.
    foreach ($taskDisposable in @('release.zip','payload','backup')) {
        $taskCleanup = Get-SafePath $taskStage $taskDisposable
        if (Test-Path -LiteralPath $taskCleanup) {
            try { Remove-Item -LiteralPath $taskCleanup -Recurse -Force } catch { Write-Warning $_.Exception.Message }
        }
    }
    exit 0
} catch {
    $taskFailure = $_.Exception.Message
    $taskRollbackErrors = @()
    for ($taskIndex = $taskApplied.Count-1; $taskIndex -ge 0; $taskIndex--) {
        try {
            $taskOperation = $taskApplied[$taskIndex]
            $taskDestination = Get-SafePath $taskRoot $taskOperation.path
            if ($taskOperation.existed) { Set-File (Get-SafePath $taskBackup $taskOperation.path) $taskDestination $true }
            elseif (Test-Path -LiteralPath $taskDestination) { Remove-Item -LiteralPath $taskDestination -Force }
        } catch { $taskRollbackErrors += $_.Exception.Message }
    }
    if ($taskLock) { $taskLock.Dispose() }
    $taskMessage = 'Cập nhật lỗi: ' + $taskFailure
    if ($taskRollbackErrors.Count) { $taskMessage += '. Khôi phục chưa đủ; giữ bản sao tại ' + $taskBackup + ': ' + ($taskRollbackErrors -join '; ') }
    else { $taskMessage += '. Bản cũ được giữ nguyên/khôi phục.' }
    if ($Mode -eq 'Install') {
        Write-Result 'error' $taskMessage
        if ($taskWaiting -and -not (Get-Process -Id $taskJob.parent_pid -ErrorAction SilentlyContinue) -and -not $taskRollbackErrors.Count) {
            $null = Start-Process -FilePath $taskJob.old_exe -WorkingDirectory $taskRoot -PassThru
        }
    }
    Write-Error $taskMessage -ErrorAction Continue
    exit 1
}
