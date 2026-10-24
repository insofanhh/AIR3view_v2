# Exercises the shipped installer against isolated portable installations.
param([string]$Installer = (Join-Path $PSScriptRoot 'apply-update.ps1'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskScratch = Join-Path $env:TEMP ('AIR3view-updater-qa-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskScratch | Out-Null
$taskInstaller = [IO.Path]::GetFullPath($Installer)
foreach ($taskVersion in @('old','new')) {
    $taskCode = @'
using System;
using System.IO;
using System.Threading;
public class AppVERSION {
  public static void Main(string[] args) {
    string root = AppDomain.CurrentDomain.BaseDirectory;
    if (args.Length > 0 && args[0] == "owner") {
      File.WriteAllText(Path.Combine(root,"owner-ready"),"ready");
      while (!File.Exists(Path.Combine(root,"owner-exit"))) Thread.Sleep(50);
    } else File.WriteAllText(Path.Combine(root,"restarted.txt"),"VERSION");
  }
}
'@
    Add-Type -TypeDefinition $taskCode.Replace('VERSION',$taskVersion) -OutputAssembly (Join-Path $taskScratch ($taskVersion + '.exe')) -OutputType WindowsApplication
}
function New-Fixture([string]$Name) {
    $taskRoot = Join-Path $taskScratch ($Name + " cập nhật & 'app'")
    $taskStage = Join-Path $taskRoot ('.air3view-update-' + [Guid]::NewGuid().ToString())
    $taskInput = Join-Path $taskScratch ($Name + '-input')
    New-Item -ItemType Directory -Path $taskStage,$taskInput,(Join-Path $taskRoot '.venv'),(Join-Path $taskRoot 'workers') | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskScratch 'old.exe') -Destination (Join-Path $taskRoot 'AIR3view.exe')
    [IO.File]::WriteAllText((Join-Path $taskRoot 'workers/z-locked.txt'),'old-worker')
    [IO.File]::WriteAllText((Join-Path $taskRoot '.venv/keep.txt'),'python-runtime')
    [IO.File]::WriteAllText((Join-Path $taskRoot 'settings.json'),'user-secret-fixture')
    $taskPaths = @('AIR3view.exe','studio-cli.exe','release.json','workers/requirements.lock','scripts/setup-workers.ps1','tools/uv.exe','workers/a-new.txt','workers/z-locked.txt')
    foreach ($taskRelative in $taskPaths) {
        $taskFile = Join-Path $taskInput $taskRelative
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskFile) | Out-Null
        if ($taskRelative -eq 'AIR3view.exe') { Copy-Item -LiteralPath (Join-Path $taskScratch 'new.exe') -Destination $taskFile }
        elseif ($taskRelative -eq 'release.json') { [IO.File]::WriteAllText($taskFile,'{"tag":"v99.0.0"}') }
        else { [IO.File]::WriteAllText($taskFile,'new-content') }
    }
    $taskManifest = @(foreach ($taskRelative in $taskPaths) {
        $taskFile = Get-Item -LiteralPath (Join-Path $taskInput $taskRelative)
        @{path=$taskRelative;bytes=$taskFile.Length;sha256=(Get-FileHash -LiteralPath $taskFile.FullName).Hash}
    })
    [IO.File]::WriteAllText((Join-Path $taskInput 'package-manifest.json'),(ConvertTo-Json -InputObject $taskManifest -Depth 4))
    [IO.Compression.ZipFile]::CreateFromDirectory($taskInput,(Join-Path $taskStage 'release.zip'))
    $taskJob = @{root=$taskRoot;stage=$taskStage;tag='v99.0.0';parent_pid=0;old_exe=(Join-Path $taskRoot 'AIR3view.exe');result=(Join-Path $taskRoot 'result.json')}
    [IO.File]::WriteAllText((Join-Path $taskStage 'job.json'),($taskJob | ConvertTo-Json))
    return [pscustomobject]@{Root=$taskRoot;Stage=$taskStage;Input=$taskInput;Job=$taskJob;JobFile=(Join-Path $taskStage 'job.json')}
}
function Prepare($Fixture, [bool]$Success) {
    $taskPreviousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $taskInstaller -JobFile $Fixture.JobFile -Mode Prepare *> (Join-Path $Fixture.Stage 'prepare.log')
    $ErrorActionPreference = $taskPreviousPreference
    if (($LASTEXITCODE -eq 0) -ne $Success) { throw "Unexpected preparation result: $(Get-Content -LiteralPath (Join-Path $Fixture.Stage 'prepare.log') -Raw)" }
}
function Wait-File([string]$Path) {
    $taskDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $Path)) {
        if ([DateTime]::UtcNow -gt $taskDeadline) { throw "Timed out waiting for $Path" }
        Start-Sleep -Milliseconds 80
    }
}
function Install($Fixture, [bool]$Success, [bool]$Locked = $false) {
    Prepare $Fixture $true
    $taskOwner = Start-Process -FilePath $Fixture.Job.old_exe -ArgumentList owner -WindowStyle Hidden -PassThru
    $taskHelper = $null
    $taskHeld = $null
    try {
        Wait-File (Join-Path $Fixture.Root 'owner-ready')
        $Fixture.Job.parent_pid = $taskOwner.Id
        [IO.File]::WriteAllText($Fixture.JobFile,($Fixture.Job | ConvertTo-Json))
        if ($Locked) { $taskHeld = [IO.File]::Open((Join-Path $Fixture.Root 'workers/z-locked.txt'),'Open','Read','Read') }
        $taskHelper = Start-Process -FilePath powershell.exe -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"'+$taskInstaller+'"'),'-JobFile',('"'+$Fixture.JobFile+'"'),'-Mode','Install') -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Fixture.Stage 'install-out.log') -RedirectStandardError (Join-Path $Fixture.Stage 'install-err.log') -PassThru
        Wait-File (Join-Path $Fixture.Stage 'waiting')
        if ((Get-FileHash -LiteralPath $Fixture.Job.old_exe).Hash -ne (Get-FileHash -LiteralPath (Join-Path $taskScratch 'old.exe')).Hash) { throw 'Installation mutated before owner exited' }
        [IO.File]::WriteAllText((Join-Path $Fixture.Root 'owner-exit'),'exit')
        if (-not $taskHelper.WaitForExit(90000)) { throw 'Installer timed out' }
        $taskReport = Get-Content -LiteralPath $Fixture.Job.result -Raw -Encoding UTF8 | ConvertFrom-Json
        if (($taskReport.status -eq 'success') -ne $Success) { throw ($taskReport | ConvertTo-Json -Compress) }
        Wait-File (Join-Path $Fixture.Root 'restarted.txt')
        $taskExpected = if ($Success) { 'new' } else { 'old' }
        if ((Get-Content -LiteralPath (Join-Path $Fixture.Root 'restarted.txt') -Raw) -ne $taskExpected) { throw 'Wrong executable restarted' }
        if ((Get-Content -LiteralPath (Join-Path $Fixture.Root '.venv/keep.txt') -Raw) -ne 'python-runtime' -or
            (Get-Content -LiteralPath (Join-Path $Fixture.Root 'settings.json') -Raw) -ne 'user-secret-fixture') { throw 'User data/runtime modified' }
        if (-not $Success -and ((Test-Path -LiteralPath (Join-Path $Fixture.Root 'workers/a-new.txt')) -or
            (Get-Content -LiteralPath (Join-Path $Fixture.Root 'workers/z-locked.txt') -Raw) -ne 'old-worker')) { throw 'Rollback did not restore original files' }
    } finally {
        if ($taskHeld) { $taskHeld.Dispose() }
        if ($taskOwner -and -not $taskOwner.HasExited) { [IO.File]::WriteAllText((Join-Path $Fixture.Root 'owner-exit'),'exit'); $null = $taskOwner.WaitForExit(5000) }
        if ($taskHelper -and -not $taskHelper.HasExited) { $taskHelper.Kill(); $null = $taskHelper.WaitForExit(5000) }
    }
}
Install (New-Fixture 'success') $true
Write-Host 'PASS: actual EXE exit, changed/missing files installed, new EXE restarted, runtime/settings preserved; Unicode and special paths.'
Install (New-Fixture 'locked') $false $true
Write-Host 'PASS: locked file triggers rollback after partial installation and restarts old executable.'
foreach ($taskCase in @('corrupt','traversal','duplicate','unlisted','tag','user-data')) {
    $taskFixture = New-Fixture $taskCase
    $taskZip = [IO.Compression.ZipFile]::Open((Join-Path $taskFixture.Stage 'release.zip'),[IO.Compression.ZipArchiveMode]::Update)
    try {
        if ($taskCase -eq 'corrupt') {
            $taskOldEntry = $taskZip.Entries | Where-Object { $_.FullName.Replace('\','/') -eq 'workers/z-locked.txt' } | Select-Object -First 1
            $taskOldEntry.Delete()
            $taskEntry = $taskZip.CreateEntry('workers/z-locked.txt')
            $taskWriter = New-Object IO.StreamWriter ($taskEntry.Open()); $taskWriter.Write('tampered'); $taskWriter.Dispose()
        } elseif ($taskCase -eq 'tag') { $taskFixture.Job.tag = 'v98'; [IO.File]::WriteAllText($taskFixture.JobFile,($taskFixture.Job | ConvertTo-Json)) }
        else {
            $taskPath = switch ($taskCase) { 'traversal' {'../outside.txt'} 'duplicate' {'air3view.EXE'} 'unlisted' {'workers/unlisted.txt'} 'user-data' {'.venv/overwrite.txt'} }
            $null = $taskZip.CreateEntry($taskPath)
        }
    } finally { $taskZip.Dispose() }
    Prepare $taskFixture $false
    if ((Get-FileHash -LiteralPath $taskFixture.Job.old_exe).Hash -ne (Get-FileHash -LiteralPath (Join-Path $taskScratch 'old.exe')).Hash) { throw 'Failed validation modified current executable' }
    Write-Host "PASS: rejected $taskCase package before installation."
}
Write-Host ('Updater verification PASS. Artifacts: ' + $taskScratch)
exit 0
