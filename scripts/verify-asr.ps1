param([Parameter(Mandatory=$true)][string]$PackagePath,
      [ValidateSet('cpu','cuda')][string]$Device='cpu', [string]$Model='base',
      [ValidateSet('auto','int8_float16','float16','int8_float32','float32')][string]$ComputeType='auto',
      [string]$PythonPath, [switch]$UpstreamAudio)
$ErrorActionPreference='Stop'
$taskPackage=(Resolve-Path -LiteralPath $PackagePath).Path
$taskPython=if($PythonPath){(Resolve-Path -LiteralPath $PythonPath).Path}else{Join-Path $taskPackage '.venv\Scripts\python.exe'}
$taskScratch=Join-Path ([IO.Path]::GetTempPath()) ('AIR3view-asr-verify-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskScratch | Out-Null
$taskWav=Join-Path $taskScratch 'english.wav'
$taskExpectedWords=@('video','police','car')
if($UpstreamAudio){
    # Hosted Windows runners need not have a Windows SAPI voice installed.
    # Use the upstream faster-whisper v1.2.1 public speech test fixture instead.
    $taskWav=Join-Path $taskScratch 'jfk.flac'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://raw.githubusercontent.com/SYSTRAN/faster-whisper/v1.2.1/tests/data/jfk.flac' -OutFile $taskWav -TimeoutSec 60
    if((Get-FileHash -LiteralPath $taskWav -Algorithm SHA256).Hash -ne '63A4B1E4C1DC655AC70961FFBF518ACD249DF237E5A0152FAAE9A4A836949715'){throw 'Upstream speech fixture SHA256 mismatch'}
    $taskExpectedWords=@('Americans','country')
}else{
    Add-Type -AssemblyName System.Speech
    $taskSpeech=[System.Speech.Synthesis.SpeechSynthesizer]::new()
    try {
        $taskSpeech.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::NotSet,[System.Speech.Synthesis.VoiceAge]::NotSet,0,[Globalization.CultureInfo]::GetCultureInfo('en-US'))
        $taskSpeech.SetOutputToWaveFile($taskWav)
        $taskSpeech.Speak('Hello everyone. Today we are testing video review software. The police officer is standing beside a black car.')
    } finally { $taskSpeech.Dispose() }
}
$taskJobs=@(@{input=$taskWav;output=(Join-Path $taskScratch 'english.json');language='en'},
            @{input=$taskWav;output=(Join-Path $taskScratch 'automatic.json');language=''})
$taskBatch=Join-Path $taskScratch 'batch.json'
[IO.File]::WriteAllText($taskBatch,($taskJobs | ConvertTo-Json),[Text.UTF8Encoding]::new($false))
& $taskPython (Join-Path $taskPackage 'workers\transcribe.py') --batch $taskBatch --device $Device --model $Model --compute-type $ComputeType
if($LASTEXITCODE -ne 0){throw 'Real ASR inference failed'}
foreach($taskJob in $taskJobs){
    $taskWords=@(Get-Content -LiteralPath $taskJob.output -Raw | ConvertFrom-Json)
    if($taskWords.Count -lt 8){throw 'ASR did not recognize sufficient words'}
    $taskText=($taskWords | ForEach-Object text) -join ' '
    foreach($taskExpected in $taskExpectedWords){if($taskText -notmatch $taskExpected){throw ('Missing expected speech content: '+$taskExpected+' in '+$taskText)}}
    foreach($taskWord in $taskWords){if($taskWord.start_ms -lt 0 -or $taskWord.end_ms -le $taskWord.start_ms -or $taskWord.end_ms -gt 20000){throw 'Invalid ASR timestamps'}}
    $taskReport=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($taskJob.output,'asr.json')) -Raw | ConvertFrom-Json
    if($taskReport.status -ne 'complete' -or $taskReport.model -ne $Model -or $taskReport.device -ne $Device){throw 'ASR used an unexpected model/device'}
    if($ComputeType -ne 'auto' -and $taskReport.compute_type -ne $ComputeType){throw 'ASR silently changed the requested compute type'}
    if($Device -eq 'cuda' -and (-not $taskReport.gpu -or -not $taskReport.cublas)){throw 'Missing actual GPU/runtime identity'}
    Write-Host ('PASS: '+$taskReport.model+' / '+$taskReport.device+' / '+$taskReport.compute_type+' / '+$taskWords.Count+' words: '+$taskText)
}
Write-Host ('ASR reports: '+$taskScratch)
