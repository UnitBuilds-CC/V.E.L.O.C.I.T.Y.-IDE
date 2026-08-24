$files = @(
    "velocity-ide\src\compiler\driver\layer_gpu_gemvs.rs",
    "velocity-ide\src\compiler\driver\packing.rs",
    "velocity-ide\src\compiler\driver\vulkan_benchmark.rs",
    "velocity-ide\src\compiler\driver\gemv.rs",
    "velocity-ide\src\tokenizer.rs",
    "velocity-ide\src\nda_int\gemv.rs",
    "velocity-ide\src\compiler\driver\qwen_layer.rs",
    "velocity-ide\src\compiler\driver\bitnet_layer.rs",
    "velocity-ide\src\compiler\driver\nda_gemv.rs",
    "velocity-ide\src\compiler\driver\nda_bitnet_layer.rs",
    "velocity-ide\src\compiler\driver\model_pipeline.rs",
    "velocity-ide\src\compiler\driver\vulkan_init.rs",
    "velocity-ide\src\nda_int\ops.rs",
    "velocity-ide\src\model\weights.rs",
    "velocity-ide\src\errors.rs",
    "velocity-ide\src\credential_guard.rs",
    "velocity-ide\src\velocity_client.rs",
    "velocity-ide\src\main.rs"
)
foreach ($f in $files) {
    $path = Join-Path "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Kimi-Code" $f
    $lines = (Get-Content $path).Count
    $content = [IO.File]::ReadAllText($path)
    $tests = ([regex]::Matches($content, '#\[test\]')).Count
    if ($tests -gt 0) {
        $ratio = [math]::Round($lines / $tests, 1)
        Write-Output "$ratio`t$tests`t$lines`t$f"
    }
}
