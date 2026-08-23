<#
.SYNOPSIS
    Jarvis dev loop: rebuild what changed, sync resources into target, run.

.DESCRIPTION
    Stops running instances (the linker cannot replace a locked .exe),
    rebuilds the frontend only when its sources are newer than the built
    bundle, builds the cargo workspace, syncs resources/ into the target
    directory, and launches the binary in the foreground so the log is
    visible. Ctrl+C stops it.

.EXAMPLE
    .\dev.ps1                 debug build, run jarvis-app
.EXAMPLE
    .\dev.ps1 -Gui            debug build, run jarvis-gui
.EXAMPLE
    .\dev.ps1 -Res            no compiling: sync resources and run
.EXAMPLE
    .\dev.ps1 -Release        build with the release profile
.EXAMPLE
    .\dev.ps1 -NoRun          build and sync, do not launch
#>
param(
    [switch]$Gui,
    [switch]$Res,
    [switch]$Release,
    [switch]$NoRun,
    [switch]$Front
)

$root = $PSScriptRoot
Set-Location $root

function Step($msg) {
    Write-Host ""
    Write-Host ">> $msg" -ForegroundColor Cyan
}

function Fail($msg) {
    Write-Host ""
    Write-Host "[FAILED] $msg" -ForegroundColor Red
    Write-Host "If the linker reported 'Access denied' on an .exe, Windows may" -ForegroundColor DarkGray
    Write-Host "still be holding the file. Just run the script again." -ForegroundColor DarkGray
    exit 1
}

$profileName = if ($Release) { 'release' } else { 'debug' }
$binName = if ($Gui) { 'jarvis-gui' } else { 'jarvis-app' }
$exe = Join-Path $root "target\$profileName\$binName.exe"

# --- stop running instances, otherwise the linker hits a locked exe ---
Step "Stopping running instances"
$running = Get-Process jarvis-app, jarvis-gui -ErrorAction SilentlyContinue
if ($running) {
    foreach ($p in $running) {
        Write-Host ("   killing {0} (pid {1})" -f $p.ProcessName, $p.Id)
        Stop-Process -Id $p.Id -Force
    }
    Start-Sleep -Milliseconds 500
} else {
    Write-Host "   nothing running"
}

# --- frontend: rebuild only when sources changed ---
if (-not $Res) {
    $needFront = [bool]$Front

    if (-not $needFront) {
        $dist = Join-Path $root 'frontend\dist\client\index.html'
        if (-not (Test-Path $dist)) {
            $needFront = $true
        } else {
            $distTime = (Get-Item $dist).LastWriteTime
            $watched = @()
            $watched += Get-ChildItem (Join-Path $root 'frontend\src') -Recurse -File -ErrorAction SilentlyContinue
            $watched += Get-ChildItem (Join-Path $root 'frontend') -File -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^(package\.json|vite\.config\.ts|tsconfig.*\.json)$' }
            if ($watched.Count -gt 0) {
                $newest = ($watched | Measure-Object LastWriteTime -Maximum).Maximum
                if ($newest -gt $distTime) { $needFront = $true }
            }
        }
    }

    if ($needFront) {
        Step "Building frontend"
        npm --prefix .\frontend run build
        if ($LASTEXITCODE -ne 0) { Fail "frontend build" }
    } else {
        Step "Frontend up to date, skipping"
    }
}

# --- rust ---
if (-not $Res) {
    Step "Building workspace ($profileName)"
    if ($Release) {
        cargo build --release
    } else {
        cargo build
    }
    if ($LASTEXITCODE -ne 0) { Fail "cargo build" }
}

# --- resources -> target\<profile>\resources ---
# the app reads the copy under target, never the sources
Step "Syncing resources"
python post_build.py --sync
if ($LASTEXITCODE -ne 0) { Fail "post_build.py" }

# --- run ---
if ($NoRun) {
    Step "Done, not launching"
    Write-Host "   $exe"
    exit 0
}

if (-not (Test-Path $exe)) { Fail "binary not found: $exe" }

Step "Running $binName ($profileName) - Ctrl+C to stop"
Write-Host ""
& $exe
