$files = @(
    'velocity-ide\src\credential_guard.rs',
    'velocity-ide\src\errors.rs',
    'velocity-ide\src\main.rs',
    'velocity-ide\src\provider_usage.rs',
    'velocity-ide\src\lib.rs',
    'velocity-ide\src\pipeline_execution.rs',
    'velocity-ide\src\pipeline_bridge.rs',
    'velocity-ide\src\nda.rs',
    'velocity-ide\src\tokenizer.rs',
    'velocity-ide\src\safety.rs'
)
foreach ($f in $files) {
    $lines = (Get-Content $f).Count
    $tests = (Select-String -Path $f -Pattern '#\[test\]').Count
    if ($tests -gt 0) {
        $ratio = [math]::Round($lines / $tests, 1)
    } else {
        $ratio = 'inf'
    }
    Write-Host "$f : $lines lines, $tests tests, ratio $ratio"
}
