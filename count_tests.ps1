Get-ChildItem -Recurse "C:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\velocity-ide\src\*.rs" | ForEach-Object {
    $lines = (Get-Content $_.FullName | Measure-Object -Line).Lines
    $tests = (Select-String -Path $_.FullName -Pattern '#\[test\]' | Measure-Object).Count
    if ($tests -gt 0) {
        $ratio = [math]::Round($lines / $tests, 1)
        Write-Output ("{0,-50} {1,5} lines  {2,4} tests  ratio={3}" -f $_.Name, $lines, $tests, $ratio)
    }
} | Sort-Object { [double]($_ -split 'ratio=')[1] } -Descending
