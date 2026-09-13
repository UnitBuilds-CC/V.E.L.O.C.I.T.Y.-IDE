#!/usr/bin/env pwsh
# bench_regression.ps1 — Run benchmarks and detect regressions vs. baseline.
#
# Usage:
#   pwsh bench_regression.ps1                  # Compare against saved baseline
#   pwsh bench_regression.ps1 -SaveBaseline    # Save current as new baseline
#
# Regression threshold: any benchmark slower by >15% triggers failure.
# Results are saved to target/bench-results/ for CI artifact upload.

param(
    [switch]$SaveBaseline,
    [double]$Threshold = 15.0  # percent regression to flag
)

$ErrorActionPreference = "Stop"
Set-Location "$PSScriptRoot"

$baselineDir = "target\bench-baselines"
$resultsDir  = "target\bench-results"
$baselineFile = "$baselineDir\baseline.json"
$resultsFile  = "$resultsDir\current.json"

New-Item -ItemType Directory -Force -Path $baselineDir | Out-Null
New-Item -ItemType Directory -Force -Path $resultsDir  | Out-Null

# ── Run benchmarks with machine-parseable output ────────────────────────────
Write-Host "Running benchmarks (this may take a few minutes)..." -ForegroundColor Cyan

# Criterion outputs JSON when --message-format=json is passed via cargo bench
$env:CARGO_TARGET_DIR = "target"
cargo bench --workspace -- --noplot --message-format json 2>$null | Out-File $resultsFile -Encoding utf8

if (-not (Test-Path $resultsFile) -or (Get-Item $resultsFile).Length -eq 0) {
    Write-Host "ERROR: Benchmark produced no output." -ForegroundColor Red
    exit 1
}

# ── Save baseline mode ──────────────────────────────────────────────────────
if ($SaveBaseline) {
    Copy-Item $resultsFile $baselineFile -Force
    Write-Host "Baseline saved to $baselineFile" -ForegroundColor Green
    exit 0
}

# ── Compare against baseline ────────────────────────────────────────────────
if (-not (Test-Path $baselineFile)) {
    Write-Host "No baseline found. Run with -SaveBaseline first." -ForegroundColor Yellow
    Write-Host "Saving current run as initial baseline..." -ForegroundColor Yellow
    Copy-Item $resultsFile $baselineFile -Force
    exit 0
}

# Parse criterion JSON output (one JSON object per line)
function Parse-BenchResults($file) {
    $results = @{}
    Get-Content $file | ForEach-Object {
        try {
            $obj = $_ | ConvertFrom-Json
            if ($obj.reason -eq "benchmark-complete") {
                $results[$obj.id] = @{
                    MedianNs = $obj.median.raw
                    StdDevNs = if ($obj.stddev) { $obj.stddev.raw } else { 0 }
                }
            }
        } catch {
            # Skip non-JSON lines
        }
    }
    return $results
}

$current  = Parse-BenchResults $resultsFile
$baseline = Parse-BenchResults $baselineFile

# ── Regression detection ────────────────────────────────────────────────────
$regressions = @()
$improvements = @()
$summary = @()

foreach ($benchId in $current.Keys) {
    $curMedian = $current[$benchId].MedianNs
    if ($baseline.ContainsKey($benchId)) {
        $baseMedian = $baseline[$benchId].MedianNs
        if ($baseMedian -gt 0) {
            $changePct = [math]::Round((($curMedian - $baseMedian) / $baseMedian) * 100, 1)
        } else {
            $changePct = 0
        }
    } else {
        $changePct = $null  # New benchmark, no baseline
    }

    $summary += [PSCustomObject]@{
        Benchmark = $benchId
        CurrentNs = $curMedian
        BaselineNs = if ($baseline.ContainsKey($benchId)) { $baseline[$benchId].MedianNs } else { "NEW" }
        Change = if ($null -ne $changePct) { "${changePct}%" } else { "N/A" }
    }

    if ($null -ne $changePct -and $changePct -gt $Threshold) {
        $regressions += "$benchId : +${changePct}% (threshold: ${Threshold}%)"
    }
    if ($null -ne $changePct -and $changePct -lt -5.0) {
        $improvements += "$benchId : ${changePct}%"
    }
}

# ── Print report ────────────────────────────────────────────────────────────
Write-Host "`n=== Benchmark Results ===" -ForegroundColor Yellow
$summary | Format-Table -AutoSize

if ($improvements.Count -gt 0) {
    Write-Host "`nImprovements:" -ForegroundColor Green
    $improvements | ForEach-Object { Write-Host "  + $_" -ForegroundColor Green }
}

if ($regressions.Count -gt 0) {
    Write-Host "`nREGRESSIONS DETECTED (>${Threshold}% slower):" -ForegroundColor Red
    $regressions | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host "`nTo update baseline after intentional changes, run:" -ForegroundColor Yellow
    Write-Host "  pwsh bench_regression.ps1 -SaveBaseline" -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "`nNo regressions detected." -ForegroundColor Green
    exit 0
}
