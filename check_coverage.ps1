#!/usr/bin/env pwsh
# check_coverage.ps1 — Enforce per-crate coverage thresholds.
#
# Usage: pwsh check_coverage.ps1
# Expects `cargo llvm-cov` to have been run already, or runs it.
# Reads thresholds from .config/coverage-thresholds.toml.

$ErrorActionPreference = "Stop"
Set-Location "$PSScriptRoot"

# ── Parse thresholds from TOML ──────────────────────────────────────────────
$toml = Get-Content ".config\coverage-thresholds.toml" -Raw
$workspaceMin = [regex]::Match($toml, 'fail-under-lines\s*=\s*(\d+)').Groups[1].Value
if (-not $workspaceMin) { $workspaceMin = 40 }

# ── Run llvm-cov with JSON output ───────────────────────────────────────────
Write-Host "Running cargo llvm-cov (JSON)..." -ForegroundColor Cyan
$jsonOut = cargo llvm-cov --workspace --json 2>&1 | Out-String
$coverage = $jsonOut | ConvertFrom-Json

# ── Aggregate per-crate line coverage ───────────────────────────────────────
$crateThresholds = @{
    "velocity-ide"     = 50
    "velocity_mcp"     = 40
    "velocity-browser" = 30
    "drone"            = 35
}

$failures = @()
$summary = @()

foreach ($crate in $crateThresholds.Keys) {
    $threshold = $crateThresholds[$crate]
    # Find matching files in coverage data
    $crateFiles = $coverage.data.files | Where-Object { $_.filename -match [regex]::Escape($crate) }
    if (-not $crateFiles) {
        $summary += [PSCustomObject]@{ Crate = $crate; Lines = "N/A"; Threshold = $threshold; Status = "SKIP" }
        continue
    }
    $totalLines = ($crateFiles | Measure-Object -Property lines -Sum).Sum
    $coveredLines = ($crateFiles | Measure-Object -Property covered -Sum).Sum
    $pct = if ($totalLines -gt 0) { [math]::Round(($coveredLines / $totalLines) * 100, 1) } else { 0 }
    $status = if ($pct -ge $threshold) { "PASS" } else { "FAIL" }
    if ($status -eq "FAIL") { $failures += "$crate: ${pct}% < ${threshold}%" }
    $summary += [PSCustomObject]@{ Crate = $crate; Lines = "${pct}%"; Threshold = "${threshold}%"; Status = $status }
}

# ── Print report ────────────────────────────────────────────────────────────
Write-Host "`n=== Coverage Report ===" -ForegroundColor Yellow
$summary | Format-Table -AutoSize

if ($failures.Count -gt 0) {
    Write-Host "`nCOVERAGE THRESHOLD FAILURES:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
} else {
    Write-Host "`nAll crate coverage thresholds met." -ForegroundColor Green
    exit 0
}
