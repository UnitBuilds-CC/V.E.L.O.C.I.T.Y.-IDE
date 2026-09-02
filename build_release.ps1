#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Build V.E.L.O.C.I.T.Y. release binaries for distribution.

.DESCRIPTION
    Compiles all production binaries in release mode and stages them
    into the dist/ directory for packaging into an installer.

.PARAMETER SkipBuild
    Skip cargo build and only stage existing binaries.

.PARAMETER ArchiveOnly
    Create a ZIP archive instead of an installer (no Inno Setup needed).
#>

param(
    [switch]$SkipBuild,
    [switch]$ArchiveOnly
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$DistDir = Join-Path $RepoRoot "dist"
$BinDir = Join-Path $DistDir "bin"

# ── Version from git ──────────────────────────────────────────────────
$Version = (git -C $RepoRoot describe --tags --always 2>$null)
if (-not $Version) { $Version = "0.1.0-dev" }
$Version = $Version -replace '^v', ''
Write-Host "Building V.E.L.O.C.I.T.Y. $Version" -ForegroundColor Cyan

# ── Clean dist ────────────────────────────────────────────────────────
if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
New-Item -ItemType Directory -Path $BinDir -Force | Out-Null

# ── Build ─────────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    Write-Host "`n==> Building release binaries..." -ForegroundColor Yellow
    Push-Location $RepoRoot
    try {
        cargo build --release `
            -p velocity_mcp `
            -p velocity-ide `
            -p velocity-ide-gui `
            -p velocity-drone `
            2>&1 | ForEach-Object { Write-Host $_ }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally { Pop-Location }
}

# ── Stage binaries ────────────────────────────────────────────────────
Write-Host "`n==> Staging binaries to dist/" -ForegroundColor Yellow

$ReleaseDir = Join-Path $RepoRoot "target\release"
$Binaries = @(
    @{ Src = "velocity_mcp.exe";    Dst = "velocity_mcp.exe"    },
    @{ Src = "velocity_ide.exe";    Dst = "velocity_ide.exe"    },
    @{ Src = "velocity_ide_gui.exe"; Dst = "velocity_ide_gui.exe" },
    @{ Src = "velocity-drone.exe";  Dst = "velocity-drone.exe"  }
)

foreach ($bin in $Binaries) {
    $src = Join-Path $ReleaseDir $bin.Src
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $BinDir $bin.Dst)
        $size = [math]::Round((Get-Item $src).Length / 1MB, 1)
        Write-Host "  $($bin.Dst) ($size MB)" -ForegroundColor Green
    } else {
        Write-Host "  WARNING: $($bin.Src) not found" -ForegroundColor Red
    }
}

# ── Stage support files ───────────────────────────────────────────────
Write-Host "`n==> Staging support files..." -ForegroundColor Yellow

Copy-Item (Join-Path $RepoRoot "LICENSE") (Join-Path $DistDir "LICENSE.txt")
Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $DistDir "README.md") -ErrorAction SilentlyContinue

# ── Summary ───────────────────────────────────────────────────────────
$totalSize = (Get-ChildItem $DistDir -Recurse | Measure-Object -Property Length -Sum).Sum
$totalMB = [math]::Round($totalSize / 1MB, 1)
Write-Host "`n==> Distribution staged to dist/ ($totalMB MB total)" -ForegroundColor Cyan
Write-Host "    Binaries: $($BinDir)" -ForegroundColor Gray
Write-Host "    Version:  $Version" -ForegroundColor Gray

# ── Optional: ZIP archive ─────────────────────────────────────────────
if ($ArchiveOnly) {
    $zipName = "VELOCITY-$Version-win-x64.zip"
    $zipPath = Join-Path $RepoRoot $zipName
    if (Test-Path $zipPath) { Remove-Item $zipPath }
    Write-Host "`n==> Creating ZIP archive: $zipName" -ForegroundColor Yellow
    Compress-Archive -Path "$DistDir\*" -DestinationPath $zipPath
    $zipSize = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
    Write-Host "  $zipName ($zipSize MB)" -ForegroundColor Green
}

# ── Optional: Inno Setup installer ────────────────────────────────────
$issPath = Join-Path $RepoRoot "installer.iss"
if ((-not $ArchiveOnly) -and (Test-Path $issPath)) {
    $iscc = $null
    foreach ($ver in @('7', '6')) {
        foreach ($base in @("${env:ProgramFiles(x86)}", $env:ProgramFiles)) {
            $candidate = Join-Path $base "Inno Setup $ver\ISCC.exe"
            if (Test-Path $candidate) { $iscc = $candidate; break }
        }
        if ($iscc) { break }
    }
    if (Test-Path $iscc) {
        Write-Host "`n==> Building installer with Inno Setup..." -ForegroundColor Yellow
        & $iscc /DMyAppVersion=$Version $issPath
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup compile failed" }
        Write-Host "  Installer created in output/" -ForegroundColor Green
    } else {
        Write-Host "`n==> Inno Setup not found. Install it from https://jrsoftware.org/isinfo.php" -ForegroundColor Yellow
        Write-Host "    Or run: .\build_release.ps1 -ArchiveOnly  (for ZIP distribution)" -ForegroundColor Yellow
    }
}

Write-Host "`nDone." -ForegroundColor Green
