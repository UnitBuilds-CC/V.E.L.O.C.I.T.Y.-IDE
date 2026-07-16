use std::time::Instant;
use std::hint::black_box;
use crate::protocol::nmcp_binary::NmcpBinaryFrame;
use crate::ipc::shmem::SharedMemoryBuffer;
use crate::compiler::driver::{VulkanDriver, VulkanQwenLayer, VulkanBitNetLayer, VulkanNdaBitNetLayer};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

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
    binary_buffer.extend_from_slice(b"NMCP"); // Magic
    binary_buffer.extend_from_slice(&[0u8; 32]); // Dummy Merkle root
    binary_buffer.extend_from_slice(b"read_nda C:/invoices/inv-001.nda"); // Payload
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
    
    let mut buffer = SharedMemoryBuffer::create_or_open(temp_shmem_path).expect("Failed to create temp shmem");
    
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
        black_box(bench_qwen_3b_layer_int4(&mut qwen_data));
    }
    let duration_qwen_layer = start_qwen_layer.elapsed();
    let qwen_layer_avg_us = (duration_qwen_layer.as_micros() as f64) / (model_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", qwen_layer_avg_us);

    print!("Running BitNet-3B Layer (Ternary) CPU Benchmark...");
    let start_bitnet_layer = Instant::now();
    for _ in 0..model_iterations {
        black_box(bench_bitnet_3b_layer_ternary(&mut bitnet_data));
    }
    let duration_bitnet_layer = start_bitnet_layer.elapsed();
    let bitnet_layer_avg_us = (duration_bitnet_layer.as_micros() as f64) / (model_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us", bitnet_layer_avg_us);

    print!("Running BitNet-3B Layer (V.E.L.O.C.I.T.Y. NDA) CPU Benchmark...");
    let start_bitnet_nda = Instant::now();
    for _ in 0..model_iterations {
        black_box(bench_bitnet_3b_layer_nda(&mut bitnet_data));
    }
    let duration_bitnet_nda = start_bitnet_nda.elapsed();
    let bitnet_nda_layer_avg_us = (duration_bitnet_nda.as_micros() as f64) / (model_iterations as f64);
    println!(" [OK] Mean Latency: {:.2} us (Speedup: {:.2}x)", bitnet_nda_layer_avg_us, bitnet_layer_avg_us / bitnet_nda_layer_avg_us);

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
            let mut gpu_qwen_data = Qwen3BGpuLayerData::new(&driver).expect("Failed to initialize Qwen GPU data");
            let mut gpu_bitnet_data = BitNet3BGpuLayerData::new(&driver).expect("Failed to initialize BitNet GPU data");

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

            let mut gpu_bitnet_nda_data = BitNet3BGpuNdaLayerData::new(&driver).expect("Failed to initialize BitNet NDA GPU data");
            print!("Running BitNet-3B Layer (V.E.L.O.C.I.T.Y. NDA) GPU Compute Execution...");
            let mut sum_bitnet_nda = 0.0;
            for _ in 0..model_iterations {
                sum_bitnet_nda += bench_bitnet_3b_layer_gpu_nda(&mut gpu_bitnet_nda_data).unwrap();
            }
            gpu_bitnet_nda_latency = sum_bitnet_nda / (model_iterations as f64);
            println!(" [OK] Mean Latency: {:.2} us (Speedup: {:.2}x)", gpu_bitnet_nda_latency, gpu_bitnet_latency / gpu_bitnet_nda_latency);

            println!("\nRunning Attention Memory Access Benchmark (Contiguous vs NDA-KV)...");
            match driver.run_attn_benchmarks() {
                Ok((contig_us, ndakv_us)) => {
                    println!("  [OK] Contiguous KV-Cache (Old-Fashioned): {:.2} us", contig_us);
                    println!("  [OK] Cryptographic NDA-KV Traversal:       {:.2} us", ndakv_us);
                    println!("  Attention Traversal Overhead:            {:.2}x slower", ndakv_us / contig_us);
                }
                Err(e) => {
                    println!("  [WARNING] Attention benchmarks failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!(" [WARNING] GPU Benchmarks skipped: Low-level driver initialization failed: {:?}", e);
        }
    }

    // Calculations
    let qwen_full_cpu_ms = (qwen_layer_avg_us * 36.0) / 1000.0;
    let _bitnet_full_cpu_ms = (bitnet_layer_avg_us * 30.0) / 1000.0;
    let qwen_cpu_tps = 1000.0 / qwen_full_cpu_ms;
    let bitnet_cpu_tps = 1000.0 / bitnet_full_ms_calc(bitnet_layer_avg_us);

    fn bitnet_full_ms_calc(layer_avg: f64) -> f64 { (layer_avg * 30.0) / 1000.0 }

    let qwen_layer_ops = 175_767_552f64;
    let bitnet_layer_ops = 247_808_000f64;

    println!("\n============================================================");
    println!("                       Summary Results");
    println!("============================================================");
    println!("  JSON-RPC Parse (Serde):      {:.2} ns", json_avg_ns);
    println!("  Mmapped Buffer R/W:          {:.2} ns", shmem_avg_ns);
    println!("  Zero-Alloc Binary Parse:     {:.2} ns", bin_avg_ns);
    println!("  Binary Ingestion Speedup:    {:.1}x over JSON-RPC", json_avg_ns / bin_avg_ns);
    println!("------------------------------------------------------------");
    println!("  INT4 Quantized GEMM (1024):  {:.2} us", int4_avg_ms);
    println!("  Ternary Popcount GEMM (1024):{:.2} us", ternary_avg_ms);
    println!("  Ternary Arithmetic Speedup:  {:.2}x over INT4 Quantized", int4_avg_ms / ternary_avg_ms);
    println!("------------------------------------------------------------");
    println!("  Qwen-3B (INT4) CPU Latency:  {:.2} us ({:.1} Gops)", qwen_layer_avg_us, (qwen_layer_ops / (qwen_layer_avg_us * 1e-6)) * 1e-9);
    println!("  BitNet-3B (Ternary) CPU Lat: {:.2} us ({:.1} Gops)", bitnet_layer_avg_us, (bitnet_layer_ops / (bitnet_layer_avg_us * 1e-6)) * 1e-9);
    println!("  CPU Layer Speedup Ratio:     {:.2}x faster", qwen_layer_avg_us / bitnet_layer_avg_us);
    println!("  CPU Extrapolated Token Gen:");
    println!("    - Qwen-3B (36 Layers):      {:.2} ms ({:.1} tokens/sec)", qwen_full_cpu_ms, qwen_cpu_tps);
    println!("    - BitNet-3B (30 Layers):    {:.2} ms ({:.1} tokens/sec)", bitnet_full_ms_calc(bitnet_layer_avg_us), bitnet_cpu_tps);
    
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
        println!("  Qwen-3B (INT4) GPU Latency:  {:.2} us ({:.1} Gops)", gpu_qwen_latency, qwen_gpu_gops);
        println!("  BitNet-3B (Ternary) GPU Lat: {:.2} us ({:.1} Gops)", gpu_bitnet_latency, bitnet_gpu_gops);
        println!("  BitNet-3B (NDA) GPU Latency: {:.2} us ({:.1} Gops)", gpu_bitnet_nda_latency, bitnet_nda_gpu_gops);
        println!("  GPU Layer Speedup Ratio:     {:.2}x (NDA vs Ternary)", gpu_bitnet_latency / gpu_bitnet_nda_latency);
        println!("  GPU Extrapolated Token Gen:");
        println!("    - Qwen-3B (36 Layers):      {:.2} ms ({:.1} tokens/sec)", qwen_full_gpu_ms, qwen_gpu_tps);
        println!("    - BitNet-3B (Ternary):      {:.2} ms ({:.1} tokens/sec)", bitnet_full_gpu_ms, bitnet_gpu_tps);
        println!("    - BitNet-3B (NDA):          {:.2} ms ({:.1} tokens/sec)", bitnet_nda_full_gpu_ms, bitnet_nda_gpu_tps);
        println!("------------------------------------------------------------");
        println!("  GPU vs. CPU Acceleration Factor:");
        println!("    - Qwen-3B Speedup (GPU/CPU):{:.2}x faster", qwen_layer_avg_us / gpu_qwen_latency);
        println!("    - BitNet-3B Speedup(GPU/CPU):{:.2}x faster", bitnet_layer_avg_us / gpu_bitnet_latency);
    }
    println!("============================================================");
}

#[inline(never)]
fn benchmark_int4_gemm(size: usize) -> u32 {
    let mut sum = 0u32;
    let inputs = vec![0x55u8; size / 2];
    let weights = vec![0x33u8; (size * size) / 2];
    
    for row in 0..size {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * size) / 2;
        for col_pair in 0..(size / 2) {
            let i_byte = inputs[col_pair];
            let w_byte = weights[weight_row_offset + col_pair];
            
            let i0 = (i_byte & 0x0F) as i32 - 8;
            let i1 = (i_byte >> 4) as i32 - 8;
            let w0 = (w_byte & 0x0F) as i32 - 8;
            let w1 = (w_byte >> 4) as i32 - 8;
            
            row_sum += w0 * i0 + w1 * i1;
        }
        sum = sum.wrapping_add(row_sum as u32);
    }
    sum
}

#[inline(never)]
fn benchmark_ternary_gemm(size: usize) -> u32 {
    let mut sum = 0u32;
    let inputs = vec![0x55555555u32; size / 16];
    let weights = vec![0x33333333u32; (size * size) / 16];
    
    for row in 0..size {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * size) / 16;
        for col_group in 0..(size / 16) {
            let i_val = inputs[col_group];
            let w_val = weights[weight_row_offset + col_group];
            
            let matches = !(i_val ^ w_val);
            let count = matches.count_ones() as i32;
            row_sum += count - 8;
        }
        sum = sum.wrapping_add(row_sum as u32);
    }
    sum
}

#[inline(always)]
fn gemv_int4(k: usize, n: usize, inputs: &[u8], weights: &[u8], outputs: &mut [i32]) {
    for row in 0..n {
        let mut sum = 0i32;
        let weight_row_offset = (row * k) / 2;
        for col_pair in 0..(k / 2) {
            let i_byte = inputs[col_pair];
            let w_byte = weights[weight_row_offset + col_pair];
            
            let i0 = (i_byte & 0x0F) as i32 - 8;
            let i1 = (i_byte >> 4) as i32 - 8;
            let w0 = (w_byte & 0x0F) as i32 - 8;
            let w1 = (w_byte >> 4) as i32 - 8;
            
            sum += w0 * i0 + w1 * i1;
        }
        outputs[row] = sum;
    }
}

#[inline(always)]
fn gemv_ternary(k: usize, n: usize, inputs: &[u32], weights: &[u32], outputs: &mut [i32]) {
    for row in 0..n {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * k) / 16;
        for col_group in 0..(k / 16) {
            let i_val = inputs[col_group];
            let w_val = weights[weight_row_offset + col_group];
            
            let matches = !(i_val ^ w_val);
            row_sum += matches.count_ones() as i32 - 8;
        }
        outputs[row] = row_sum;
    }
}

struct Qwen3BLayerData {
    inputs_2304: Vec<u8>,
    inputs_11008: Vec<u8>,
    weight_q: Vec<u8>,
    weight_o: Vec<u8>,
    weight_k: Vec<u8>,
    weight_v: Vec<u8>,
    weight_gate: Vec<u8>,
    weight_up: Vec<u8>,
    weight_down: Vec<u8>,
    out_2304_a: Vec<i32>,
    out_2304_b: Vec<i32>,
    out_256_k: Vec<i32>,
    out_256_v: Vec<i32>,
    out_11008_gate: Vec<i32>,
    out_11008_up: Vec<i32>,
}

impl Qwen3BLayerData {
    fn new() -> Self {
        Self {
            inputs_2304: vec![0x55u8; 2304 / 2],
            inputs_11008: vec![0x66u8; 11008 / 2],
            weight_q: vec![0x33u8; (2304 * 2304) / 2],
            weight_o: vec![0x44u8; (2304 * 2304) / 2],
            weight_k: vec![0x11u8; (2304 * 256) / 2],
            weight_v: vec![0x22u8; (2304 * 256) / 2],
            weight_gate: vec![0x77u8; (2304 * 11008) / 2],
            weight_up: vec![0x88u8; (2304 * 11008) / 2],
            weight_down: vec![0x99u8; (11008 * 2304) / 2],
            out_2304_a: vec![0; 2304],
            out_2304_b: vec![0; 2304],
            out_256_k: vec![0; 256],
            out_256_v: vec![0; 256],
            out_11008_gate: vec![0; 11008],
            out_11008_up: vec![0; 11008],
        }
    }
}

#[inline(never)]
fn bench_qwen_3b_layer_int4(data: &mut Qwen3BLayerData) {
    gemv_int4(2304, 2304, &data.inputs_2304, &data.weight_q, &mut data.out_2304_a);
    gemv_int4(2304, 256, &data.inputs_2304, &data.weight_k, &mut data.out_256_k);
    gemv_int4(2304, 256, &data.inputs_2304, &data.weight_v, &mut data.out_256_v);
    gemv_int4(2304, 2304, &data.inputs_2304, &data.weight_o, &mut data.out_2304_b);
    gemv_int4(2304, 11008, &data.inputs_2304, &data.weight_gate, &mut data.out_11008_gate);
    gemv_int4(2304, 11008, &data.inputs_2304, &data.weight_up, &mut data.out_11008_up);
    
    for i in 0..11008 {
        data.inputs_11008[i / 2] = (data.out_11008_gate[i] ^ data.out_11008_up[i]) as u8;
    }
    
    gemv_int4(11008, 2304, &data.inputs_11008, &data.weight_down, &mut data.out_2304_a);
}

struct Qwen3BGpuLayerData {
    inputs_2304: Vec<f32>,
    out_2304_a: Vec<f32>,
    layer: VulkanQwenLayer,
}

impl Qwen3BGpuLayerData {
    fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_2304 = vec![1.0f32; 2304];
        let out_2304_a = vec![0.0; 2304];

        let weight_q = vec![0x33u8; (2304 * 2304) / 2];
        let weight_o = vec![0x44u8; (2304 * 2304) / 2];
        let weight_k = vec![0x11u8; (2304 * 256) / 2];
        let weight_v = vec![0x22u8; (2304 * 256) / 2];
        let weight_gate = vec![0x77u8; (2304 * 11008) / 2];
        let weight_up = vec![0x88u8; (2304 * 11008) / 2];
        let weight_down = vec![0x99u8; (11008 * 2304) / 2];

        let layer = VulkanQwenLayer::new(
            driver,
            &weight_q,
            &weight_k,
            &weight_v,
            &weight_o,
            &weight_gate,
            &weight_up,
            &weight_down,
        )?;

        Ok(Self {
            inputs_2304,
            out_2304_a,
            layer,
        })
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn gemv_nda_sse2(k: usize, n: usize, inputs_active: &[u32], inputs_pos: &[u32], weights_active: &[u32], weights_pos: &[u32], outputs: &mut [i32]) {
    let num_col_groups_128 = k / 128;
    for row in 0..n {
        let mut row_sum = 0i32;
        let all_ones = _mm_set1_epi32(-1);
        for cg in 0..num_col_groups_128 {
            // Load 4 words (128 bits) of input active & pos
            let in_act = _mm_loadu_si128(inputs_active.as_ptr().add(cg * 4) as *const __m128i);
            let in_pos = _mm_loadu_si128(inputs_pos.as_ptr().add(cg * 4) as *const __m128i);
            
            // Load 4 words (128 bits) of weight active & pos
            let w_act = _mm_loadu_si128(weights_active.as_ptr().add(cg * n * 4 + row * 4) as *const __m128i);
            let w_pos = _mm_loadu_si128(weights_pos.as_ptr().add(cg * n * 4 + row * 4) as *const __m128i);
            
            // Perform logical operations in SSE registers
            let act_both = _mm_and_si128(in_act, w_act);
            let same_sign = _mm_xor_si128(in_pos, w_pos);
            let same_sign = _mm_xor_si128(same_sign, all_ones); // same_sign = !same_sign
            
            let pos_contrib = _mm_and_si128(act_both, same_sign);
            let neg_contrib = _mm_andnot_si128(same_sign, act_both); // act_both & ~same_sign
            
            // Extract elements to call fast scalar popcnt
            let pos_arr: [u32; 4] = std::mem::transmute(pos_contrib);
            let neg_arr: [u32; 4] = std::mem::transmute(neg_contrib);
            
            row_sum += (pos_arr[0].count_ones() as i32 - neg_arr[0].count_ones() as i32) +
                       (pos_arr[1].count_ones() as i32 - neg_arr[1].count_ones() as i32) +
                       (pos_arr[2].count_ones() as i32 - neg_arr[2].count_ones() as i32) +
                       (pos_arr[3].count_ones() as i32 - neg_arr[3].count_ones() as i32);
        }
        outputs[row] = row_sum;
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
unsafe fn gemv_nda_sse2(k: usize, n: usize, inputs_active: &[u32], inputs_pos: &[u32], weights_active: &[u32], weights_pos: &[u32], outputs: &mut [i32]) {
    // stub for completeness; on x86_64 only the version above is used
}

#[cfg(not(target_arch = "x86_64"))]
fn gemv_nda_scalar(k: usize, n: usize, inputs_active: &[u32], inputs_pos: &[u32], weights_active: &[u32], weights_pos: &[u32], outputs: &mut [i32]) {
    let num_col_groups_128 = k / 128;
    for row in 0..n {
        let mut row_sum = 0i32;
        for cg in 0..num_col_groups_128 {
            for i in 0..4 {
                let in_a = inputs_active[cg * 4 + i];
                let in_p = inputs_pos[cg * 4 + i];
                
                let w_a = weights_active[cg * n * 4 + row * 4 + i];
                let w_p = weights_pos[cg * n * 4 + row * 4 + i];
                
                let act_both = in_a & w_a;
                let same_sign = !(in_p ^ w_p);
                let pos_contrib = act_both & same_sign;
                let neg_contrib = act_both & !same_sign;
                
                row_sum += pos_contrib.count_ones() as i32 - neg_contrib.count_ones() as i32;
            }
        }
        outputs[row] = row_sum;
    }
}

#[inline(always)]
fn gemv_nda(k: usize, n: usize, inputs_active: &[u32], inputs_pos: &[u32], weights_active: &[u32], weights_pos: &[u32], outputs: &mut [i32]) {
    #[cfg(target_arch = "x86_64")]
    {
        unsafe {
            gemv_nda_sse2(k, n, inputs_active, inputs_pos, weights_active, weights_pos, outputs);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        gemv_nda_scalar(k, n, inputs_active, inputs_pos, weights_active, weights_pos, outputs);
    }
}

struct BitNet3BLayerData {
    inputs_3200: Vec<u32>,
    inputs_8640: Vec<u32>,
    weight_q: Vec<u32>,
    weight_k: Vec<u32>,
    weight_v: Vec<u32>,
    weight_o: Vec<u32>,
    weight_gate: Vec<u32>,
    weight_up: Vec<u32>,
    weight_down: Vec<u32>,
    
    // NDA Decomposed versions for CPU benchmark
    nda_inputs_3200_active: Vec<u32>,
    nda_inputs_3200_pos: Vec<u32>,
    nda_inputs_8640_active: Vec<u32>,
    nda_inputs_8640_pos: Vec<u32>,
    
    nda_weight_q_active: Vec<u32>,
    nda_weight_q_pos: Vec<u32>,
    nda_weight_k_active: Vec<u32>,
    nda_weight_k_pos: Vec<u32>,
    nda_weight_v_active: Vec<u32>,
    nda_weight_v_pos: Vec<u32>,
    nda_weight_o_active: Vec<u32>,
    nda_weight_o_pos: Vec<u32>,
    nda_weight_gate_active: Vec<u32>,
    nda_weight_gate_pos: Vec<u32>,
    nda_weight_up_active: Vec<u32>,
    nda_weight_up_pos: Vec<u32>,
    nda_weight_down_active: Vec<u32>,
    nda_weight_down_pos: Vec<u32>,
    
    out_3200_q: Vec<i32>,
    out_3200_k: Vec<i32>,
    out_3200_v: Vec<i32>,
    out_3200_o: Vec<i32>,
    out_8640_gate: Vec<i32>,
    out_8640_up: Vec<i32>,
    out_3200_down: Vec<i32>,
    
    nda_out_3200_q: Vec<i32>,
    nda_out_3200_k: Vec<i32>,
    nda_out_3200_v: Vec<i32>,
    nda_out_3200_o: Vec<i32>,
    nda_out_8640_gate: Vec<i32>,
    nda_out_8640_up: Vec<i32>,
    nda_out_3200_down: Vec<i32>,
}

impl BitNet3BLayerData {
    fn new() -> Self {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let inputs_8640 = vec![0x66666666u32; 8640 / 16];
        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
        };
        let to_u32_vec = |bytes: Vec<u8>| -> Vec<u32> {
            unsafe {
                let u32_ptr = bytes.as_ptr() as *const u32;
                std::slice::from_raw_parts(u32_ptr, bytes.len() / 4).to_vec()
            }
        };

        let (nda_inputs_3200_active, nda_inputs_3200_pos) = crate::compiler::driver::pack_inputs_nda(&inputs_3200);
        let (nda_inputs_8640_active, nda_inputs_8640_pos) = crate::compiler::driver::pack_inputs_nda(&inputs_8640);

        let (wq_a, wq_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_q), 3200, 3200);
        let (wk_a, wk_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_k), 3200, 3200);
        let (wv_a, wv_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_v), 3200, 3200);
        let (wo_a, wo_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_o), 3200, 3200);
        let (wgate_a, wgate_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_gate), 3200, 8640);
        let (wup_a, wup_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_up), 3200, 8640);
        let (wdown_a, wdown_p) = crate::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_down), 8640, 3200);

        Self {
            inputs_3200,
            inputs_8640,
            weight_q,
            weight_k,
            weight_v,
            weight_o,
            weight_gate,
            weight_up,
            weight_down,
            nda_inputs_3200_active,
            nda_inputs_3200_pos,
            nda_inputs_8640_active,
            nda_inputs_8640_pos,
            nda_weight_q_active: to_u32_vec(wq_a),
            nda_weight_q_pos: to_u32_vec(wq_p),
            nda_weight_k_active: to_u32_vec(wk_a),
            nda_weight_k_pos: to_u32_vec(wk_p),
            nda_weight_v_active: to_u32_vec(wv_a),
            nda_weight_v_pos: to_u32_vec(wv_p),
            nda_weight_o_active: to_u32_vec(wo_a),
            nda_weight_o_pos: to_u32_vec(wo_p),
            nda_weight_gate_active: to_u32_vec(wgate_a),
            nda_weight_gate_pos: to_u32_vec(wgate_p),
            nda_weight_up_active: to_u32_vec(wup_a),
            nda_weight_up_pos: to_u32_vec(wup_p),
            nda_weight_down_active: to_u32_vec(wdown_a),
            nda_weight_down_pos: to_u32_vec(wdown_p),
            out_3200_q: vec![0; 3200],
            out_3200_k: vec![0; 3200],
            out_3200_v: vec![0; 3200],
            out_3200_o: vec![0; 3200],
            out_8640_gate: vec![0; 8640],
            out_8640_up: vec![0; 8640],
            out_3200_down: vec![0; 3200],
            nda_out_3200_q: vec![0; 3200],
            nda_out_3200_k: vec![0; 3200],
            nda_out_3200_v: vec![0; 3200],
            nda_out_3200_o: vec![0; 3200],
            nda_out_8640_gate: vec![0; 8640],
            nda_out_8640_up: vec![0; 8640],
            nda_out_3200_down: vec![0; 3200],
        }
    }
}

#[inline(never)]
fn bench_bitnet_3b_layer_ternary(data: &mut BitNet3BLayerData) {
    gemv_ternary(3200, 3200, &data.inputs_3200, &data.weight_q, &mut data.out_3200_q);
    gemv_ternary(3200, 3200, &data.inputs_3200, &data.weight_k, &mut data.out_3200_k);
    gemv_ternary(3200, 3200, &data.inputs_3200, &data.weight_v, &mut data.out_3200_v);
    gemv_ternary(3200, 3200, &data.inputs_3200, &data.weight_o, &mut data.out_3200_o);
    gemv_ternary(3200, 8640, &data.inputs_3200, &data.weight_gate, &mut data.out_8640_gate);
    gemv_ternary(3200, 8640, &data.inputs_3200, &data.weight_up, &mut data.out_8640_up);
    
    for i in 0..(8640 / 16) {
        data.inputs_8640[i] = (data.out_8640_gate[i * 16] ^ data.out_8640_up[i * 16]) as u32;
    }
    
    gemv_ternary(8640, 3200, &data.inputs_8640, &data.weight_down, &mut data.out_3200_down);
}

#[inline(never)]
fn bench_bitnet_3b_layer_nda(data: &mut BitNet3BLayerData) {
    gemv_nda(3200, 3200, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_q_active, &data.nda_weight_q_pos, &mut data.nda_out_3200_q);
    gemv_nda(3200, 3200, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_k_active, &data.nda_weight_k_pos, &mut data.nda_out_3200_k);
    gemv_nda(3200, 3200, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_v_active, &data.nda_weight_v_pos, &mut data.nda_out_3200_v);
    gemv_nda(3200, 3200, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_o_active, &data.nda_weight_o_pos, &mut data.nda_out_3200_o);
    gemv_nda(3200, 8640, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_gate_active, &data.nda_weight_gate_pos, &mut data.nda_out_8640_gate);
    gemv_nda(3200, 8640, &data.nda_inputs_3200_active, &data.nda_inputs_3200_pos, &data.nda_weight_up_active, &data.nda_weight_up_pos, &mut data.nda_out_8640_up);
    
    for i in 0..(8640 / 32) {
        let mut active_word = 0u32;
        let mut pos_word = 0u32;
        for bit in 0..32 {
            let idx = i * 32 + bit;
            let val = (data.nda_out_8640_gate[idx] * data.nda_out_8640_up[idx]) as f32 * 0.0001;
            let q_val = if val > 0.0 { 1 } else if val < 0.0 { -1 } else { 0 };
            if q_val != 0 {
                active_word |= 1 << bit;
                if q_val == 1 {
                    pos_word |= 1 << bit;
                }
            }
        }
        data.nda_inputs_8640_active[i] = active_word;
        data.nda_inputs_8640_pos[i] = pos_word;
    }
    
    gemv_nda(8640, 3200, &data.nda_inputs_8640_active, &data.nda_inputs_8640_pos, &data.nda_weight_down_active, &data.nda_weight_down_pos, &mut data.nda_out_3200_down);
}



struct BitNet3BGpuLayerData {
    inputs_3200: Vec<u32>,
    out_3200_down: Vec<f32>,
    layer: VulkanBitNetLayer,
}

impl BitNet3BGpuLayerData {
    fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let out_3200_down = vec![0.0; 3200];

        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
        };

        let layer = VulkanBitNetLayer::new(
            driver,
            to_bytes_u32(&weight_q),
            to_bytes_u32(&weight_k),
            to_bytes_u32(&weight_v),
            to_bytes_u32(&weight_o),
            to_bytes_u32(&weight_gate),
            to_bytes_u32(&weight_up),
            to_bytes_u32(&weight_down),
        )?;

        Ok(Self {
            inputs_3200,
            out_3200_down,
            layer,
        })
    }
}

struct BitNet3BGpuNdaLayerData {
    inputs_active: Vec<u8>,
    inputs_pos: Vec<u8>,
    out_3200_down: Vec<f32>,
    layer: VulkanNdaBitNetLayer,
}

impl BitNet3BGpuNdaLayerData {
    fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let out_3200_down = vec![0.0; 3200];

        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
        };

        let (in_act, in_pos) = crate::compiler::driver::pack_inputs_nda(&inputs_3200);
        let inputs_active = unsafe {
            std::slice::from_raw_parts(in_act.as_ptr() as *const u8, in_act.len() * 4).to_vec()
        };
        let inputs_pos = unsafe {
            std::slice::from_raw_parts(in_pos.as_ptr() as *const u8, in_pos.len() * 4).to_vec()
        };

        let layer = VulkanNdaBitNetLayer::new(
            driver,
            to_bytes_u32(&weight_q),
            to_bytes_u32(&weight_k),
            to_bytes_u32(&weight_v),
            to_bytes_u32(&weight_o),
            to_bytes_u32(&weight_gate),
            to_bytes_u32(&weight_up),
            to_bytes_u32(&weight_down),
        )?;

        Ok(Self {
            inputs_active,
            inputs_pos,
            out_3200_down,
            layer,
        })
    }
}

fn bench_qwen_3b_layer_gpu(data: &mut Qwen3BGpuLayerData) -> Result<f64, Box<dyn std::error::Error>> {
    let to_bytes_f32 = |slice: &[f32]| -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
    };
    data.layer.run(to_bytes_f32(&data.inputs_2304), &mut data.out_2304_a)
}

fn bench_bitnet_3b_layer_gpu(data: &mut BitNet3BGpuLayerData) -> Result<f64, Box<dyn std::error::Error>> {
    let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
    };
    data.layer.run(to_bytes_u32(&data.inputs_3200), &mut data.out_3200_down)
}

fn bench_bitnet_3b_layer_gpu_nda(data: &mut BitNet3BGpuNdaLayerData) -> Result<f64, Box<dyn std::error::Error>> {
    data.layer.run(&data.inputs_active, &data.inputs_pos, &mut data.out_3200_down)
}
