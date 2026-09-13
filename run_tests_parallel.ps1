# Run tests across all workspace crates in parallel
# Usage: .\run_tests_parallel.ps1 [-CrateFilter "velocity-mcp,velocity-browser"]
#        .\run_tests_parallel.ps1 -LibOnly          # only --lib tests (no doctests)
#        .\run_tests_parallel.ps1 -Verbose           # show live output per crate

param(
    [string]$CrateFilter = "",
    [switch]$LibOnly,
    [switch]$Verbose
)

$ErrorActionPreference = "Stop"
$workspaceRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

# All workspace crates (from Cargo.toml members)
$allCrates = @("velocity-mcp", "velocity-ide", "velocity-ide-gui", "velocity-browser", "drone", "e2e")

if ($CrateFilter) {
    $crates = $CrateFilter -split "," | ForEach-Object { $_.Trim() }
} else {
    $crates = $allCrates
}

Write-Host "=============================================" -ForegroundColor Magenta
Write-Host " Velocity IDE — Parallel Test Runner" -ForegroundColor Magenta
Write-Host "=============================================" -ForegroundColor Magenta
Write-Host "Crates under test: $($crates -join ', ')" -ForegroundColor Cyan
Write-Host "Lib only: $LibOnly" -ForegroundColor Cyan
Write-Host ""

$startTime = Get-Date
$jobs = @()

foreach ($crate in $crates) {
    Write-Host "  Starting tests for $crate..." -ForegroundColor Cyan

    $jobScript = {
        param($crateName, $workspaceRoot, $libOnly)
        Set-Location $workspaceRoot
        $args = @("test", "-p", $crateName)
        if ($libOnly) { $args += "--lib" }
        $args += "--"
        if (-not $libOnly) { $args += "--quiet" }
        $output = & cargo @args 2>&1
        $exitCode = $LASTEXITCODE
        [PSCustomObject]@{
            Crate    = $crateName
            ExitCode = $exitCode
            Output   = ($output -join "`n")
        }
    }

    $jobs += Start-Job -Name $crate -ScriptBlock $jobScript -ArgumentList $crate, $workspaceRoot, $LibOnly
}

# Wait for all jobs and collect results
Write-Host ""
Write-Host "Waiting for $($jobs.Count) test jobs to complete..." -ForegroundColor Yellow
Write-Host ""

$results = $jobs | Wait-Job | Receive-Job

# Display per-crate results
$passed = 0
$failed = 0
$failedCrates = @()

foreach ($result in $results) {
    if ($result.ExitCode -eq 0) {
        Write-Host "  [PASS] $($result.Crate)" -ForegroundColor Green
        $passed++
    } else {
        Write-Host "  [FAIL] $($result.Crate)  (exit code $($result.ExitCode))" -ForegroundColor Red
        $failed++
        $failedCrates += $result.Crate
        if ($Verbose) {
            Write-Host $result.Output -ForegroundColor Red
        }
    }
}

# Summary
$elapsed = (Get-Date) - $startTime
Write-Host ""
Write-Host "=============================================" -ForegroundColor Magenta
Write-Host " Summary" -ForegroundColor Magenta
Write-Host "=============================================" -ForegroundColor Magenta
Write-Host "  Passed : $passed" -ForegroundColor Green
Write-Host "  Failed : $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Time   : $($elapsed.ToString('mm\:ss\.ff'))" -ForegroundColor Yellow

if ($failed -gt 0) {
    Write-Host ""
    Write-Host "Failed crates:" -ForegroundColor Red
    $failedCrates | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host ""
    Write-Host "Re-run a failed crate with:" -ForegroundColor Yellow
    Write-Host "  cargo test -p <crate-name> -- --nocapture" -ForegroundColor Yellow
    exit 1
}

Write-Host ""
Write-Host "All tests passed!" -ForegroundColor Green
exit 0
