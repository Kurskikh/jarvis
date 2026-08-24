<#
.SYNOPSIS
    Run the speech sidecar Jarvis talks to.

.DESCRIPTION
    Two engines live here, each in its own Python environment because their
    dependencies cannot share a room: CosyVoice pins transformers 4.51.3 and
    qwen-tts wants 4.57.3. They listen on different ports, so both can run at
    once and Jarvis picks one with the "speech sidecar" setting.

    Qwen is the default: measured on this machine it reaches first audio in
    about 430 ms against CosyVoice's 1900, and its speaking rate never sags.

    Jarvis only ever CONNECTS to whatever is listening - leave the interpreter
    and script fields in its settings empty and this process stays yours to
    start and stop.

.EXAMPLE
    .\tts.ps1                 start Qwen (port 8772) and open the console
.EXAMPLE
    .\tts.ps1 -Engine cosy    start CosyVoice instead (port 8771)
.EXAMPLE
    .\tts.ps1 -Status         what is running, and how much video memory is free
.EXAMPLE
    .\tts.ps1 -Stop           stop the sidecars
.EXAMPLE
    .\tts.ps1 -Unload         keep serving but hand the video memory back
.EXAMPLE
    .\tts.ps1 -Preload        load the model at startup, the way it used to be
.EXAMPLE
    .\tts.ps1 -Foreground     run in this window instead of a detached one
#>
param(
    [ValidateSet('qwen', 'cosy', 'both')]
    [string]$Engine = 'qwen',
    [switch]$Status,
    [switch]$Stop,
    [switch]$Unload,
    [switch]$Reload,
    [switch]$Preload,
    [switch]$Foreground,
    [switch]$NoOpen,
    [switch]$ResetConfig
)

$root = $PSScriptRoot
Set-Location $root

function Step($msg) {
    Write-Host ""
    Write-Host ">> $msg" -ForegroundColor Cyan
}

function Note($msg) { Write-Host "   $msg" -ForegroundColor DarkGray }

function Fail($msg) {
    Write-Host ""
    Write-Host "[FAILED] $msg" -ForegroundColor Red
    exit 1
}

$engines = @{
    qwen = @{
        Name   = 'Qwen3-TTS'
        Python = Join-Path $root 'venv-qwen\Scripts\python.exe'
        Script = Join-Path $root 'sidecar_qwen.py'
        Port   = 8772
        Match  = 'sidecar_qwen'
    }
    cosy = @{
        Name   = 'CosyVoice'
        Python = Join-Path $root 'venv\Scripts\python.exe'
        Script = Join-Path $root 'sidecar.py'
        Port   = 8771
        Match  = 'sidecar\.py'
    }
}

function Get-SidecarProcesses($match) {
    Get-CimInstance Win32_Process -Filter "Name='python.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match $match }
}

function Get-Health($port) {
    try {
        Invoke-RestMethod -Uri "http://127.0.0.1:$port/health" -TimeoutSec 5 -ErrorAction Stop
    } catch { $null }
}

function Show-Status {
    Step "Speech sidecars"
    foreach ($key in 'qwen', 'cosy') {
        $e = $engines[$key]
        $h = Get-Health $e.Port
        if ($null -eq $h) {
            Write-Host ("   {0,-11} port {1}  not running" -f $e.Name, $e.Port) -ForegroundColor DarkGray
            continue
        }
        # what THIS sidecar holds, then what the card has left. The first is
        # the number that matters when something else wants the card, and it
        # was the one not being shown.
        $mem = if ($h.vram) {
            "holds {0} MB, card free {1} of {2}" -f $h.vram.ours_held_mb, $h.vram.card_free_mb, $h.vram.card_total_mb
        } else { '' }
        $state = if ($h.ok) { 'model in memory' } else { 'model unloaded' }
        Write-Host ("   {0,-11} port {1}  {2}   {3}" -f $e.Name, $e.Port, $state, $mem) -ForegroundColor Green
    }
}

# --------------------------------------------------------------------- stop
if ($Stop) {
    Step "Stopping sidecars"
    $any = $false
    foreach ($key in 'qwen', 'cosy') {
        foreach ($p in Get-SidecarProcesses $engines[$key].Match) {
            Note ("killing {0} (pid {1})" -f $engines[$key].Name, $p.ProcessId)
            Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
            $any = $true
        }
    }
    if (-not $any) { Note 'nothing was running' }
    exit 0
}

# ------------------------------------------------------- memory, not process
if ($Unload -or $Reload) {
    $verb = if ($Unload) { 'unload' } else { 'reload' }
    Step ("Asking every running sidecar to $verb")
    $any = $false
    foreach ($key in 'qwen', 'cosy') {
        $e = $engines[$key]
        if ($null -eq (Get-Health $e.Port)) { continue }
        $any = $true
        try {
            $r = Invoke-RestMethod -Uri "http://127.0.0.1:$($e.Port)/$verb" -Method Post -TimeoutSec 120
            $free = if ($r.vram) { "{0} MB free on the card" -f $r.vram.card_free_mb } else { 'done' }
            Write-Host ("   {0,-11} {1}" -f $e.Name, $free) -ForegroundColor Green
        } catch { Note ("{0}: {1}" -f $e.Name, $_.Exception.Message) }
    }
    if (-not $any) { Note 'no sidecar is running' }
    Show-Status
    exit 0
}

if ($Status) { Show-Status; exit 0 }

# -------------------------------------------------------------------- start
$wanted = if ($Engine -eq 'both') { @('qwen', 'cosy') } else { @($Engine) }

foreach ($key in $wanted) {
    $e = $engines[$key]

    if (-not (Test-Path $e.Python)) {
        Fail ("no Python environment at {0}. The two engines need separate ones - see the note at the top of this script." -f $e.Python)
    }
    if (-not (Test-Path $e.Script)) { Fail ("missing {0}" -f $e.Script) }

    if ($null -ne (Get-Health $e.Port)) {
        Step ("{0} is already up on port {1}" -f $e.Name, $e.Port)
        continue
    }
    # answering nothing but still holding the port: a half-dead process would
    # make the new one fail with an error about the address, which reads like
    # a configuration problem rather than a stale process
    $stale = Get-SidecarProcesses $e.Match
    if ($stale) {
        Step ("Clearing a {0} process that is not answering" -f $e.Name)
        foreach ($p in $stale) {
            Note ("killing pid {0}" -f $p.ProcessId)
            Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
        }
        Start-Sleep -Milliseconds 500
    }

    # $argv, not $args: the latter is an automatic PowerShell variable and
    # assigning to it is asking for trouble.
    #
    # No transcript is passed in. It used to be, and it broke exactly the way
    # that idea deserves: Start-Process joins -ArgumentList into one command
    # line WITHOUT quoting, so a transcript with spaces in it arrived as a
    # dozen separate arguments and argparse killed the process before it said
    # anything. The transcript cache is keyed on the audio's own bytes now, so
    # both sidecars find the same entry and nobody has to hand it over.
    $argv = @($e.Script)
    # saved settings normally beat the command line, so there has to be a way
    # to say "no, actually, start over"
    if ($ResetConfig -and $key -eq 'qwen') { $argv += '--reset-config' }
    # off by default: a 5 GB model taken at startup competes with whatever is
    # already on the card, and a load that cannot finish looks like a sidecar
    # that will not start
    if ($Preload -and $key -eq 'qwen') { $argv += '--preload' }

    Step ("Starting {0} on port {1}" -f $e.Name, $e.Port)
    if ($Preload) {
        Note 'loading the model now - takes 10-15 seconds on a free card'
    } else {
        Note 'the console opens at once; load the model there, or it loads on first use'
    }

    if ($Foreground) {
        & $e.Python @argv
        exit $LASTEXITCODE
    }

    # Hidden, with its output on disk.
    #
    # Start-Process opens a console window unless told not to, and that window
    # then sits on the desktop for the life of the sidecar showing its startup
    # chatter - including a SoX banner from a tokenizer this model does not
    # use. The process was always detached correctly; the window was noise.
    #
    # But a hidden process with nowhere to write is a process you cannot
    # diagnose, so both streams go to files beside this script. Two files,
    # because Start-Process refuses to point stdout and stderr at one.
    $outLog = Join-Path $root ("sidecar-{0}.out.log" -f $key)
    $errLog = Join-Path $root ("sidecar-{0}.err.log" -f $key)
    Start-Process -FilePath $e.Python -ArgumentList $argv -WorkingDirectory $root `
                  -WindowStyle Hidden `
                  -RedirectStandardOutput $outLog -RedirectStandardError $errLog

    $deadline = (Get-Date).AddSeconds(180)
    $up = $false
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        if ($null -ne (Get-Health $e.Port)) { $up = $true; break }
    }
    if (-not $up) {
        Fail ("{0} did not answer within three minutes. See {1}, or run with -Foreground." -f $e.Name, $outLog)
    }
    Write-Host ("   {0} ready on http://127.0.0.1:{1}" -f $e.Name, $e.Port) -ForegroundColor Green
}

Show-Status

# the Qwen sidecar serves its own console: text, sampling controls, and the
# unload button, all against the warm model rather than a copy of it
if ($Engine -ne 'cosy' -and -not $NoOpen) {
    $url = "http://127.0.0.1:$($engines.qwen.Port)/"
    Write-Host ""
    Write-Host "   console: $url" -ForegroundColor Cyan
    Start-Process $url
}
