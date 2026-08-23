<#
.SYNOPSIS
    Jarvis release build: frontend + full workspace in release, resources synced.

.DESCRIPTION
    Always rebuilds the frontend (a release build should never reuse a stale
    bundle), builds every crate in the workspace with the release profile,
    syncs resources/ into target\release, then reports the artifacts.

.EXAMPLE
    .\release.ps1             build everything
.EXAMPLE
    .\release.ps1 -Clean      cargo clean first, full rebuild from scratch
.EXAMPLE
    .\release.ps1 -Run        build, then launch jarvis-app
#>
param(
    [switch]$Clean,
    [switch]$Run
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
    exit 1
}

$started = Get-Date

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

if ($Clean) {
    Step "cargo clean (this makes the next build slow)"
    cargo clean
    if ($LASTEXITCODE -ne 0) { Fail "cargo clean" }
}

Step "Building frontend"
npm --prefix .\frontend run build
if ($LASTEXITCODE -ne 0) { Fail "frontend build" }

Step "Building workspace (release)"
cargo build --release
if ($LASTEXITCODE -ne 0) { Fail "cargo build --release" }

Step "Syncing resources"
python post_build.py --sync
if ($LASTEXITCODE -ne 0) { Fail "post_build.py" }

# --- report ---
$outDir = Join-Path $root 'target\release'

Step "Artifacts in target\release"
Get-ChildItem $outDir -Filter '*.exe' -File -ErrorAction SilentlyContinue |
    Sort-Object Name |
    ForEach-Object {
        Write-Host ("   {0,-20} {1,8:N1} MB   {2}" -f $_.Name, ($_.Length / 1MB), $_.LastWriteTime)
    }

$dlls = @(Get-ChildItem $outDir -Filter '*.dll' -File -ErrorAction SilentlyContinue)
Write-Host ("   native libraries: {0}" -f $dlls.Count)

$resDir = Join-Path $outDir 'resources'
if (Test-Path $resDir) {
    $names = (Get-ChildItem $resDir -Directory | Select-Object -ExpandProperty Name) -join ', '
    Write-Host "   resources: $names"
} else {
    Write-Host "   resources: MISSING - the app will not start" -ForegroundColor Yellow
}

$elapsed = (Get-Date) - $started
Write-Host ""
Write-Host ("Done in {0:mm\:ss}" -f $elapsed) -ForegroundColor Green

if ($Run) {
    $exe = Join-Path $outDir 'jarvis-app.exe'
    if (-not (Test-Path $exe)) { Fail "binary not found: $exe" }
    Step "Running jarvis-app (release) - Ctrl+C to stop"
    Write-Host ""
    & $exe
}
