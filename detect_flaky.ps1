#!/usr/bin/env pwsh
# detect_flaky.ps1 — Analyze nextest JUnit XML output to detect flaky tests.
#
# A test is considered "flaky" if it:
#   - Failed on first attempt but passed on retry (nextest retries are enabled
#     in .config/nextest.toml [profile.ci])
#   - Has a pass rate < 95% across multiple CI runs (requires historical data)
#
# Usage:
#   pwsh detect_flaky.ps1 [-JUnitDir target/nextest/ci] [-HistoryDir .flaky-history]
#
# Output: Prints a flaky-test report and exits non-zero if new flaky tests found.

param(
    [string]$JUnitDir = "target\nextest\ci",
    [string]$HistoryDir = ".flaky-history",
    [int]$FlakyThreshold = 95  # pass rate % below which a test is flagged
)

$ErrorActionPreference = "Continue"
Set-Location "$PSScriptRoot"

# ── Parse JUnit XML ─────────────────────────────────────────────────────────
function Parse-JUnitXml {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        Write-Host "No JUnit XML found at $Path" -ForegroundColor Yellow
        return @()
    }

    [xml]$xml = Get-Content $Path -Raw
    $results = @()

    foreach ($suite in $xml.testsuites.testsuite) {
        foreach ($tc in $suite.testcase) {
            $status = if ($tc.failure) { "FAIL" } elseif ($tc.skipped) { "SKIP" } else { "PASS" }
            $results += [PSCustomObject]@{
                Suite    = $suite.name
                Name     = $tc.name
                Status   = $status
                Duration = [double]$tc.time
                Message  = if ($tc.failure) { $tc.failure.message } else { "" }
            }
        }
    }
    return $results
}

# ── Load historical results ─────────────────────────────────────────────────
New-Item -ItemType Directory -Force -Path $HistoryDir | Out-Null
$historyFile = Join-Path $HistoryDir "test-history.json"

$history = @{}
if (Test-Path $historyFile) {
    $raw = Get-Content $historyFile -Raw | ConvertFrom-Json
    foreach ($prop in $raw.PSObject.Properties) {
        $history[$prop.Name] = $prop.Value
    }
}

# ── Analyze current run ─────────────────────────────────────────────────────
$junitFiles = Get-ChildItem -Path $JUnitDir -Filter "junit.xml" -Recurse -ErrorAction SilentlyContinue
$allResults = @()
foreach ($jf in $junitFiles) {
    $allResults += Parse-JUnitXml $jf.FullName
}

if ($allResults.Count -eq 0) {
    Write-Host "No test results found in $JUnitDir" -ForegroundColor Yellow
    exit 0
}

# ── Update history ──────────────────────────────────────────────────────────
foreach ($r in $allResults) {
    $key = "$($r.Suite)::$($r.Name)"
    if (-not $history.ContainsKey($key)) {
        $history[$key] = @{ passes = 0; fails = 0; total = 0; lastSeen = "" }
    }
    $entry = $history[$key]
    $entry.total++
    if ($r.Status -eq "PASS") { $entry.passes++ }
    if ($r.Status -eq "FAIL") { $entry.fails++ }
    $entry.lastSeen = (Get-Date -Format "yyyy-MM-dd")
}

# Save updated history
$history | ConvertTo-Json -Depth 5 | Set-Content $historyFile -Encoding utf8

# ── Detect flaky tests ──────────────────────────────────────────────────────
$flaky = @()
$newlyFlaky = @()

foreach ($key in $history.Keys) {
    $entry = $history[$key]
    if ($entry.total -ge 3) {  # Need at least 3 runs to judge
        $passRate = [math]::Round(($entry.passes / $entry.total) * 100, 1)
        if ($passRate -lt $FlakyThreshold -and $passRate -gt 0) {
            $flaky += [PSCustomObject]@{
                Test      = $key
                PassRate  = "${passRate}%"
                Runs      = $entry.total
                Failures  = $entry.fails
                LastSeen  = $entry.lastSeen
            }
        }
    }
}

# ── Print report ────────────────────────────────────────────────────────────
Write-Host "`n=== Flaky Test Report ===" -ForegroundColor Yellow
Write-Host "Total tests analyzed: $($allResults.Count)" -ForegroundColor Cyan
Write-Host "Historical entries: $($history.Count)" -ForegroundColor Cyan

if ($flaky.Count -gt 0) {
    Write-Host "`nFlaky tests detected (pass rate < ${FlakyThreshold}%):" -ForegroundColor Red
    $flaky | Format-Table -AutoSize

    # Check if these are new (not already in quarantine)
    $quarantineFile = ".config\flaky-quarantine.toml"
    if (Test-Path $quarantineFile) {
        $quarantineContent = Get-Content $quarantineFile -Raw
        foreach ($f in $flaky) {
            $testName = ($f.Test -split "::")[-1]
            if ($quarantineContent -notmatch [regex]::Escape($testName)) {
                $newlyFlaky += $f
            }
        }
    }

    if ($newlyFlaky.Count -gt 0) {
        Write-Host "`nNEW flaky tests (not yet quarantined):" -ForegroundColor Red
        $newlyFlaky | ForEach-Object {
            Write-Host "  - $($_.Test) ($($_.PassRate) pass rate)" -ForegroundColor Red
        }
        Write-Host "`nAdd these to .config/flaky-quarantine.toml to quarantine." -ForegroundColor Yellow
    }
} else {
    Write-Host "`nNo flaky tests detected." -ForegroundColor Green
}

# ── Summary ─────────────────────────────────────────────────────────────────
$passCount = ($allResults | Where-Object { $_.Status -eq "PASS" }).Count
$failCount = ($allResults | Where-Object { $_.Status -eq "FAIL" }).Count
$skipCount = ($allResults | Where-Object { $_.Status -eq "SKIP" }).Count

Write-Host "`n--- Current Run Summary ---" -ForegroundColor Cyan
Write-Host "  Passed:  $passCount" -ForegroundColor Green
Write-Host "  Failed:  $failCount" -ForegroundColor $(if ($failCount -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $skipCount" -ForegroundColor Yellow

if ($newlyFlaky.Count -gt 0) { exit 1 } else { exit 0 }
