use super::cpu::*;
use super::gpu::*;
use velocity_ide::compiler::driver::VulkanDriver;
use crate::ipc::shmem::SharedMemoryBuffer;
use crate::protocol::nmcp_binary::NmcpBinaryFrame;
use std::hint::black_box;
use std::time::Instant;

pub fn run_benchmarks() {
    println!("============================================================");
    println!("         V.E.L.O.C.I.T.Y.-MCP Performance Benchmark Suite");
    println!("============================================================");

    // 1. Benchmark JSON-RPC String Parsing (serde_json)
    let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/invoices/inv-001.nda"}},"id":101}"#;
    let json_iterations = 200_000;

    print!("Running JSON-RPC Parse Benchmark...");
    let start = Instant::now();
    for _ in 0..json_iterations {
        let val: serde_json::Value = serde_json::from_str(black_box(json_req)).unwrap();
        let _method = black_box(val["method"].as_str());
    }
    let duration_json = start.elapsed();
    let json_avg_ns = (duration_json.as_nanos() as f64) / (json_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} ns", json_avg_ns);

    // 2. Benchmark NMCP Zero-Alloc Binary Frame Parsing
    let mut binary_buffer = Vec::new();
    binary_buffer.extend_from_slice(b"NMCP");
    binary_buffer.extend_from_slice(&[0u8; 32]);
    binary_buffer.extend_from_slice(b"read_nda C:/invoices/inv-001.nda");
    let binary_iterations = 500_000;

    print!("Running NMCP Zero-Alloc Binary Frame Parse Benchmark...");
    let start_bin = Instant::now();
    for _ in 0..binary_iterations {
        let frame = NmcpBinaryFrame::parse(black_box(&binary_buffer)).unwrap();
        let _magic = black_box(frame.magic);
    }
    let duration_bin = start_bin.elapsed();
    let bin_avg_ns = (duration_bin.as_nanos() as f64) / (binary_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} ns", bin_avg_ns);

    // 3. Benchmark Shared Memory Mapped Operations (Read/Write)
    let temp_shmem_path = "temp_bench_shmem.bin";
    let shmem_iterations = 100_000;

    let mut buffer =
        SharedMemoryBuffer::create_or_open(temp_shmem_path).expect("Failed to create temp shmem");

    print!("Running Shared Memory Read/Write Operation Benchmark...");
    let start_shmem = Instant::now();
    for _ in 0..shmem_iterations {
        buffer.write_output(black_box(json_req)).unwrap();
        let _input = black_box(buffer.read_input().unwrap());
    }
    let duration_shmem = start_shmem.elapsed();
    let shmem_avg_ns = (duration_shmem.as_nanos() as f64) / (shmem_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} ns", shmem_avg_ns);
    let _ = std::fs::remove_file(temp_shmem_path);

    // 4. Kernel Benchmark: INT4 Quantized GEMM vs. Ternary (BitNet b1.58) GEMM (1024x1024)
    let matrix_size = 1024;
    let gemm_iterations = 5_000;

    print!("Running INT4 Quantized GEMM Benchmark (1024x1024)...");
    let start_int4 = Instant::now();
    for _ in 0..gemm_iterations {
        let _res = black_box(benchmark_int4_gemm(black_box(matrix_size)));
    }
    let duration_int4 = start_int4.elapsed();
    let int4_avg_ms = (duration_int4.as_micros() as f64) / (gemm_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", int4_avg_ms);

    print!("Running Ternary (BitNet b1.58 Popcount) GEMM Benchmark (1024x1024)...");
    let start_ternary = Instant::now();
    for _ in 0..gemm_iterations {
        let _res = black_box(benchmark_ternary_gemm(black_box(matrix_size)));
    }
    let duration_ternary = start_ternary.elapsed();
    let ternary_avg_ms = (duration_ternary.as_micros() as f64) / (gemm_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", ternary_avg_ms);

    // 5. Model-Level Benchmark: Qwen-3B Layer vs. BitNet-3B Layer on CPU
    let model_iterations = 200;
    println!("\nPreparing Model-Level CPU Data Structures...");
    let mut qwen_data = Qwen3BLayerData::new();
    let mut bitnet_data = BitNet3BLayerData::new();

    print!("Running Qwen-3B Layer (INT4) CPU Benchmark...");
    let start_qwen_layer = Instant::now();
    for _ in 0..model_iterations {
        bench_qwen_3b_layer_int4(&mut qwen_data);
        black_box(());
    }
    let duration_qwen_layer = start_qwen_layer.elapsed();
    let qwen_layer_avg_us = (duration_qwen_layer.as_micros() as f64) / (model_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", qwen_layer_avg_us);

    print!("Running BitNet-3B Layer (Ternary) CPU Benchmark...");
    let start_bitnet_layer = Instant::now();
    for _ in 0..model_iterations {
        bench_bitnet_3b_layer_ternary(&mut bitnet_data);
        black_box(());
    }
    let duration_bitnet_layer = start_bitnet_layer.elapsed();
    let bitnet_layer_avg_us =
        (duration_bitnet_layer.as_micros() as f64) / (model_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", bitnet_layer_avg_us);

    print!("Running BitNet-3B Layer (V.E.L.O.C.I.T.Y. NDA) CPU Benchmark...");
    let start_bitnet_nda = Instant::now();
    for _ in 0..model_iterations {
        bench_bitnet_3b_layer_nda(&mut bitnet_data);
        black_box(());
    }
    let duration_bitnet_nda = start_bitnet_nda.elapsed();
    let bitnet_nda_layer_avg_us =
        (duration_bitnet_nda.as_micros() as f64) / (model_iterations as f64);
    println!(
        " [OK] Mean Latency: {:.2} us (Speedup: {:.2}x)",
        bitnet_nda_layer_avg_us,
        bitnet_layer_avg_us / bitnet_nda_layer_avg_us
    );

    // 6. Model-Level Benchmark on GPU (GeForce MX250)
    println!("\nInitializing Vulkan GPU Compute Driver (V-NCE)...");
    let mut gpu_qwen_latency = 0.0;
    let mut gpu_bitnet_latency = 0.0;
    let mut gpu_bitnet_nda_latency = 0.0;
    let mut gpu_available = false;

    match VulkanDriver::init() {
        Ok(driver) => {
            let _ = driver.run_diagnostics();
            gpu_available = true;

            println!("\nPreparing GPU-Resident Data Structures...");
            let mut gpu_qwen_data =
                Qwen3BGpuLayerData::new(&driver).expect("Failed to initialize Qwen GPU data");
            let mut gpu_bitnet_data =
                BitNet3BGpuLayerData::new(&driver).expect("Failed to initialize BitNet GPU data");

            print!("Running Qwen-3B Layer (INT4) GPU Compute Execution...");
            let mut sum_qwen = 0.0;
            for _ in 0..model_iterations {
                sum_qwen += bench_qwen_3b_layer_gpu(&mut gpu_qwen_data).unwrap();
            }
            gpu_qwen_latency = sum_qwen / (model_iterations as f64);
            println!(" [OK] Mean Latency: {:.2} us", gpu_qwen_latency);

            print!("Running BitNet-3B Layer (Ternary) GPU Compute Execution...");
            let mut sum_bitnet = 0.0;
            for _ in 0..model_iterations {
                sum_bitnet += bench_bitnet_3b_layer_gpu(&mut gpu_bitnet_data).unwrap();
            }
            gpu_bitnet_latency = sum_bitnet / (model_iterations as f64);
            println!(" [OK] Mean Latency: {:.2} us", gpu_bitnet_latency);

            let mut gpu_bitnet_nda_data = BitNet3BGpuNdaLayerData::new(&driver)
                .expect("Failed to initialize BitNet NDA GPU data");
            print!("Running BitNet-3B Layer (V.E.L.O.C.I.T.Y. NDA) GPU Compute Execution...");
            let mut sum_bitnet_nda = 0.0;
            for _ in 0..model_iterations {
                sum_bitnet_nda += bench_bitnet_3b_layer_gpu_nda(&mut gpu_bitnet_nda_data).unwrap();
            }
            gpu_bitnet_nda_latency = sum_bitnet_nda / (model_iterations as f64);
            println!(
                " [OK] Mean Latency: {:.2} us (Speedup: {:.2}x)",
                gpu_bitnet_nda_latency,
                gpu_bitnet_latency / gpu_bitnet_nda_latency
            );

            println!("\nRunning Attention Memory Access Benchmark (Contiguous vs NDA-KV)...");
            match driver.run_attn_benchmarks() {
                Ok((contig_us, ndakv_us)) => {
                    println!(
                        "  [OK] Contiguous KV-Cache (Old-Fashioned): {:.2} us",
                        contig_us
                    );
                    println!(
                        "  [OK] Cryptographic NDA-KV Traversal:       {:.2} us",
                        ndakv_us
                    );
                    println!(
                        "  Attention Traversal Overhead:            {:.2}x slower",
                        ndakv_us / contig_us
                    );
                }
                Err(e) => {
                    println!("  [WARNING] Attention benchmarks failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!(
                " [WARNING] GPU Benchmarks skipped: Low-level driver initialization failed: {:?}",
                e
            );
        }
    }

    // Calculations
    let qwen_full_cpu_ms = (qwen_layer_avg_us * 36.0) / 1000.0;
    let _bitnet_full_cpu_ms = (bitnet_layer_avg_us * 30.0) / 1000.0;
    let qwen_cpu_tps = 1000.0 / qwen_full_cpu_ms;
    let bitnet_cpu_tps = 1000.0 / bitnet_full_ms_calc(bitnet_layer_avg_us);

    fn bitnet_full_ms_calc(layer_avg: f64) -> f64 {
        (layer_avg * 30.0) / 1000.0
    }

    let qwen_layer_ops = 175_767_552f64;
    let bitnet_layer_ops = 247_808_000f64;

    println!("\n============================================================");
    println!("                       Summary Results");
    println!("============================================================");
    println!("  JSON-RPC Parse (Serde):      {:.2} ns", json_avg_ns);
    println!("  Mmapped Buffer R/W:          {:.2} ns", shmem_avg_ns);
    println!("  Zero-Alloc Binary Parse:     {:.2} ns", bin_avg_ns);
    println!(
        "  Binary Ingestion Speedup:    {:.1}x over JSON-RPC",
        json_avg_ns / bin_avg_ns
    );
    println!("------------------------------------------------------------");
    println!("  INT4 Quantized GEMM (1024):  {:.2} us", int4_avg_ms);
    println!("  Ternary Popcount GEMM (1024):{:.2} us", ternary_avg_ms);
    println!(
        "  Ternary Arithmetic Speedup:  {:.2}x over INT4 Quantized",
        int4_avg_ms / ternary_avg_ms
    );
    println!("------------------------------------------------------------");
    println!(
        "  Qwen-3B (INT4) CPU Latency:  {:.2} us ({:.1} Gops)",
        qwen_layer_avg_us,
        (qwen_layer_ops / (qwen_layer_avg_us * 1e-6)) * 1e-9
    );
    println!(
        "  BitNet-3B (Ternary) CPU Lat: {:.2} us ({:.1} Gops)",
        bitnet_layer_avg_us,
        (bitnet_layer_ops / (bitnet_layer_avg_us * 1e-6)) * 1e-9
    );
    println!(
        "  CPU Layer Speedup Ratio:     {:.2}x faster",
        qwen_layer_avg_us / bitnet_layer_avg_us
    );
    println!("  CPU Extrapolated Token Gen:");
    println!(
        "    - Qwen-3B (36 Layers):      {:.2} ms ({:.1} tokens/sec)",
        qwen_full_cpu_ms, qwen_cpu_tps
    );
    println!(
        "    - BitNet-3B (30 Layers):    {:.2} ms ({:.1} tokens/sec)",
        bitnet_full_ms_calc(bitnet_layer_avg_us),
        bitnet_cpu_tps
    );

    if gpu_available {
        let qwen_full_gpu_ms = (gpu_qwen_latency * 36.0) / 1000.0;
        let bitnet_full_gpu_ms = (gpu_bitnet_latency * 30.0) / 1000.0;
        let bitnet_nda_full_gpu_ms = (gpu_bitnet_nda_latency * 30.0) / 1000.0;
        let qwen_gpu_tps = 1000.0 / qwen_full_gpu_ms;
        let bitnet_gpu_tps = 1000.0 / bitnet_full_gpu_ms;
        let bitnet_nda_gpu_tps = 1000.0 / bitnet_nda_full_gpu_ms;

        let qwen_gpu_gops = (qwen_layer_ops / (gpu_qwen_latency * 1e-6)) * 1e-9;
        let bitnet_gpu_gops = (bitnet_layer_ops / (gpu_bitnet_latency * 1e-6)) * 1e-9;
        let bitnet_nda_gpu_gops = (bitnet_layer_ops / (gpu_bitnet_nda_latency * 1e-6)) * 1e-9;

        println!("------------------------------------------------------------");
        println!(
            "  Qwen-3B (INT4) GPU Latency:  {:.2} us ({:.1} Gops)",
            gpu_qwen_latency, qwen_gpu_gops
        );
        println!(
            "  BitNet-3B (Ternary) GPU Lat: {:.2} us ({:.1} Gops)",
            gpu_bitnet_latency, bitnet_gpu_gops
        );
        println!(
            "  BitNet-3B (NDA) GPU Latency: {:.2} us ({:.1} Gops)",
            gpu_bitnet_nda_latency, bitnet_nda_gpu_gops
        );
        println!(
            "  GPU Layer Speedup Ratio:     {:.2}x (NDA vs Ternary)",
            gpu_bitnet_latency / gpu_bitnet_nda_latency
        );
        println!("  GPU Extrapolated Token Gen:");
        println!(
            "    - Qwen-3B (36 Layers):      {:.2} ms ({:.1} tokens/sec)",
            qwen_full_gpu_ms, qwen_gpu_tps
        );
        println!(
            "    - BitNet-3B (Ternary):      {:.2} ms ({:.1} tokens/sec)",
            bitnet_full_gpu_ms, bitnet_gpu_tps
        );
        println!(
            "    - BitNet-3B (NDA):          {:.2} ms ({:.1} tokens/sec)",
            bitnet_nda_full_gpu_ms, bitnet_nda_gpu_tps
        );
        println!("------------------------------------------------------------");
        println!("  GPU vs. CPU Acceleration Factor:");
        println!(
            "    - Qwen-3B Speedup (GPU/CPU):{:.2}x faster",
            qwen_layer_avg_us / gpu_qwen_latency
        );
        println!(
            "    - BitNet-3B Speedup(GPU/CPU):{:.2}x faster",
            bitnet_layer_avg_us / gpu_bitnet_latency
        );
    }
    println!("============================================================");
}
