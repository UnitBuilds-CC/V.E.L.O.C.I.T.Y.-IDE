use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

// ---------------------------------------------------------------------------
// GPU Compute Shader Pipeline – text-processing acceleration layer
// ---------------------------------------------------------------------------

/// Statistics about GPU pipeline usage.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Total number of task dispatches (GPU path only).
    pub total_dispatches: u64,
    /// Shader cache hits during [`GpuPipeline::compile_shader`].
    pub cache_hits: u64,
    /// Shader cache misses during [`GpuPipeline::compile_shader`].
    pub cache_misses: u64,
    /// Running average GPU dispatch time in microseconds.
    pub avg_dispatch_time_us: u64,
    /// GPU utilization ratio in range `[0.0, 1.0]`.
    pub gpu_utilization: f64,
}

/// A compiled compute shader stored in the pipeline cache.
#[derive(Debug, Clone)]
pub struct CompiledShader {
    pub name: String,
    pub hash: u64,
    pub word_count: usize,
    pub compile_time_us: u64,
}

/// Text processing tasks that can be accelerated by the GPU pipeline.
#[derive(Debug, Clone)]
pub enum TextProcessingTask {
    SyntaxHighlight { text_hash: u64, line_count: usize },
    TokenCount { text_hash: u64, char_count: usize },
    DiffCompute { old_hash: u64, new_hash: u64 },
    IndentAnalysis { text_hash: u64 },
}

/// Result of executing a text processing task.
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Task ran on the GPU successfully.
    GpuSuccess { dispatch_time_us: u64 },
    /// GPU was unavailable; task was handled by the CPU fallback.
    CpuFallback { compute_time_us: u64 },
    /// Task failed for the given reason.
    Failed { reason: String },
}

// ---------------------------------------------------------------------------
// GpuPipeline
// ---------------------------------------------------------------------------

/// GPU-accelerated text processing pipeline.
///
/// Manages shader compilation, caching, task dispatch, and transparent CPU
/// fallback when no suitable GPU is present.
pub struct GpuPipeline {
    pub device_name: String,
    pub available: bool,
    pub compute_units: u32,
    pub max_workgroup_size: u32,
    pub shader_cache: HashMap<u64, CompiledShader>,
    pub pipeline_stats: PipelineStats,
}

impl GpuPipeline {
    /// Creates a new pipeline, auto-detecting GPU availability.
    ///
    /// Detection honours the `VELOCITY_GPU` environment variable:
    /// - `VELOCITY_GPU=1` → simulated GPU available (useful for tests / CI).
    /// - anything else   → GPU unavailable (CPU fallback path).
    ///
    /// In a production build this would enumerate Vulkan / wgpu adapters.
    pub fn new() -> Self {
        let (available, device_name, compute_units, max_workgroup_size) = Self::detect_gpu();
        Self {
            device_name,
            available,
            compute_units,
            max_workgroup_size,
            shader_cache: HashMap::new(),
            pipeline_stats: PipelineStats::default(),
        }
    }

    /// Creates a pipeline with explicit GPU availability (for testing).
    #[cfg(test)]
    fn with_gpu(available: bool) -> Self {
        let (device_name, compute_units, max_workgroup_size) = if available {
            ("Velocity GPU (Simulated)".to_string(), 32, 256)
        } else {
            ("None".to_string(), 0, 0)
        };
        Self {
            device_name,
            available,
            compute_units,
            max_workgroup_size,
            shader_cache: HashMap::new(),
            pipeline_stats: PipelineStats::default(),
        }
    }

    // -- GPU detection ------------------------------------------------------

    fn detect_gpu() -> (bool, String, u32, u32) {
        match std::env::var("VELOCITY_GPU") {
            Ok(val) if val == "1" => (true, "Velocity GPU (Simulated)".to_string(), 32, 256),
            _ => (false, "None".to_string(), 0, 0),
        }
    }

    // -- Query --------------------------------------------------------------

    /// Returns `true` when a GPU is available for dispatch.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Returns a reference to the current pipeline statistics.
    pub fn stats(&self) -> &PipelineStats {
        &self.pipeline_stats
    }

    // -- Task dispatch ------------------------------------------------------

    /// Submits a text processing task.
    ///
    /// If the GPU is available the task is dispatched to the device; otherwise
    /// it is transparently rerouted through [`fallback_to_cpu`].
    pub fn submit_task(&mut self, task: TextProcessingTask) -> TaskResult {
        if !self.available {
            return self.fallback_to_cpu(task);
        }

        let start = Instant::now();
        // Simulate GPU dispatch (real impl would enqueue a compute pass here).
        let _ = task;
        let dispatch_time = start.elapsed().as_micros() as u64;

        self.pipeline_stats.total_dispatches += 1;
        // Update running average dispatch time.
        let n = self.pipeline_stats.total_dispatches;
        let old_avg = self.pipeline_stats.avg_dispatch_time_us;
        self.pipeline_stats.avg_dispatch_time_us =
            old_avg + dispatch_time.saturating_sub(old_avg) / n;
        // Bump utilization slightly per dispatch, capped at 1.0.
        self.pipeline_stats.gpu_utilization = (self.pipeline_stats.gpu_utilization + 0.01).min(1.0);

        TaskResult::GpuSuccess {
            dispatch_time_us: dispatch_time,
        }
    }

    // -- Shader compilation & cache -----------------------------------------

    /// Compiles a compute shader and stores it in the pipeline cache.
    ///
    /// On a cache hit the existing [`CompiledShader`] is cloned and returned
    /// without recompiling; `cache_hits` is incremented.  On a miss the shader
    /// is compiled, inserted, and `cache_misses` is incremented.
    pub fn compile_shader(
        &mut self,
        name: &str,
        weights: &[f64],
    ) -> Result<CompiledShader, String> {
        let hash = Self::shader_hash(name, weights);

        // Cache hit – return early.
        if let Some(existing) = self.shader_cache.get(&hash) {
            self.pipeline_stats.cache_hits += 1;
            return Ok(existing.clone());
        }

        self.pipeline_stats.cache_misses += 1;

        let start = Instant::now();
        // Simulate compilation: word count scales with weight vector length.
        let word_count = weights.len() * 4 + 16;
        let compile_time = start.elapsed().as_micros() as u64;

        let shader = CompiledShader {
            name: name.to_string(),
            hash,
            word_count,
            compile_time_us: compile_time,
        };

        self.shader_cache.insert(hash, shader.clone());
        Ok(shader)
    }

    /// Deterministic hash of a shader's identity (name + weight signature).
    fn shader_hash(name: &str, weights: &[f64]) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        weights.len().hash(&mut hasher);
        for &w in weights {
            w.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Looks up a previously compiled shader by its hash.
    pub fn get_cached_shader(&self, hash: u64) -> Option<&CompiledShader> {
        self.shader_cache.get(&hash)
    }

    /// Evicts all entries from the shader cache.
    pub fn flush_cache(&mut self) {
        self.shader_cache.clear();
    }

    // -- CPU fallback -------------------------------------------------------

    /// Executes a task on the CPU when the GPU is unavailable.
    pub fn fallback_to_cpu(&mut self, _task: TextProcessingTask) -> TaskResult {
        let start = Instant::now();
        // Simulate CPU-side computation.
        let compute_time = start.elapsed().as_micros() as u64;
        TaskResult::CpuFallback {
            compute_time_us: compute_time,
        }
    }
}

impl Default for GpuPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Pipeline creation --------------------------------------------------

    #[test]
    fn pipeline_creation_default() {
        let p = GpuPipeline::new();
        // Without VELOCITY_GPU=1 the pipeline should report unavailable.
        // (If the env var is set the test still passes – just the other branch.)
        assert!(!p.device_name.is_empty());
    }

    #[test]
    fn pipeline_creation_with_gpu() {
        let p = GpuPipeline::with_gpu(true);
        assert!(p.is_available());
        assert_eq!(p.compute_units, 32);
        assert_eq!(p.max_workgroup_size, 256);
        assert!(p.device_name.contains("Simulated"));
    }

    #[test]
    fn pipeline_creation_without_gpu() {
        let p = GpuPipeline::with_gpu(false);
        assert!(!p.is_available());
        assert_eq!(p.compute_units, 0);
        assert_eq!(p.max_workgroup_size, 0);
        assert_eq!(p.device_name, "None");
    }

    // -- is_available -------------------------------------------------------

    #[test]
    fn is_available_true_when_gpu_present() {
        let p = GpuPipeline::with_gpu(true);
        assert!(p.is_available());
    }

    #[test]
    fn is_available_false_when_no_gpu() {
        let p = GpuPipeline::with_gpu(false);
        assert!(!p.is_available());
    }

    // -- Shader compilation & caching ---------------------------------------

    #[test]
    fn shader_compilation_success() {
        let mut p = GpuPipeline::with_gpu(true);
        let result = p.compile_shader("syntax_highlight", &[0.1, 0.5, 1.0]);
        assert!(result.is_ok());
        let shader = result.unwrap();
        assert_eq!(shader.name, "syntax_highlight");
        assert!(shader.word_count > 0);
    }

    #[test]
    fn shader_caching_second_call_is_hit() {
        let mut p = GpuPipeline::with_gpu(true);
        let s1 = p.compile_shader("tok_count", &[1.0, 2.0]).unwrap();
        let s2 = p.compile_shader("tok_count", &[1.0, 2.0]).unwrap();
        assert_eq!(s1.hash, s2.hash);
        assert_eq!(p.stats().cache_hits, 1, "second call should be a cache hit");
        assert_eq!(p.stats().cache_misses, 1, "only the first call should miss");
    }

    #[test]
    fn shader_cache_different_weights_different_hash() {
        let mut p = GpuPipeline::with_gpu(true);
        let s1 = p.compile_shader("shader_a", &[1.0]).unwrap();
        let s2 = p.compile_shader("shader_a", &[2.0]).unwrap();
        assert_ne!(s1.hash, s2.hash);
        assert_eq!(p.shader_cache.len(), 2);
    }

    #[test]
    fn shader_cache_different_names_different_hash() {
        let mut p = GpuPipeline::with_gpu(true);
        let s1 = p.compile_shader("alpha", &[1.0]).unwrap();
        let s2 = p.compile_shader("beta", &[1.0]).unwrap();
        assert_ne!(s1.hash, s2.hash);
    }

    // -- get_cached_shader --------------------------------------------------

    #[test]
    fn get_cached_shader_found() {
        let mut p = GpuPipeline::with_gpu(true);
        let shader = p.compile_shader("diff", &[0.5]).unwrap();
        let cached = p.get_cached_shader(shader.hash);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().name, "diff");
    }

    #[test]
    fn get_cached_shader_not_found() {
        let p = GpuPipeline::with_gpu(true);
        assert!(p.get_cached_shader(0xDEAD_BEEF).is_none());
    }

    // -- Task submission ----------------------------------------------------

    #[test]
    fn task_submission_gpu_available() {
        let mut p = GpuPipeline::with_gpu(true);
        let task = TextProcessingTask::SyntaxHighlight {
            text_hash: 0x1234,
            line_count: 100,
        };
        match p.submit_task(task) {
            TaskResult::GpuSuccess { .. } => {}
            other => panic!("expected GpuSuccess, got {:?}", other),
        }
        assert_eq!(p.stats().total_dispatches, 1);
    }

    #[test]
    fn task_submission_gpu_unavailable_falls_back() {
        let mut p = GpuPipeline::with_gpu(false);
        let task = TextProcessingTask::TokenCount {
            text_hash: 0xABCD,
            char_count: 500,
        };
        match p.submit_task(task) {
            TaskResult::CpuFallback { .. } => {}
            other => panic!("expected CpuFallback, got {:?}", other),
        }
        // GPU dispatch counter should NOT increment on fallback.
        assert_eq!(p.stats().total_dispatches, 0);
    }

    #[test]
    fn task_submission_all_variants() {
        let mut p = GpuPipeline::with_gpu(true);
        let tasks = vec![
            TextProcessingTask::SyntaxHighlight {
                text_hash: 1,
                line_count: 10,
            },
            TextProcessingTask::TokenCount {
                text_hash: 2,
                char_count: 20,
            },
            TextProcessingTask::DiffCompute {
                old_hash: 3,
                new_hash: 4,
            },
            TextProcessingTask::IndentAnalysis { text_hash: 5 },
        ];
        for t in tasks {
            let _ = p.submit_task(t);
        }
        assert_eq!(p.stats().total_dispatches, 4);
    }

    // -- CPU fallback -------------------------------------------------------

    #[test]
    fn cpu_fallback_returns_cpu_result() {
        let mut p = GpuPipeline::with_gpu(false);
        let task = TextProcessingTask::IndentAnalysis { text_hash: 42 };
        match p.fallback_to_cpu(task) {
            TaskResult::CpuFallback { compute_time_us: _ } => {}
            other => panic!("expected CpuFallback, got {:?}", other),
        }
    }

    // -- Stats accuracy -----------------------------------------------------

    #[test]
    fn stats_accuracy_dispatches() {
        let mut p = GpuPipeline::with_gpu(true);
        for _ in 0..5 {
            let _ = p.submit_task(TextProcessingTask::IndentAnalysis { text_hash: 0 });
        }
        assert_eq!(p.stats().total_dispatches, 5);
        assert!(p.stats().gpu_utilization > 0.0);
        assert!(p.stats().gpu_utilization <= 1.0);
    }

    #[test]
    fn stats_accuracy_cache_counters() {
        let mut p = GpuPipeline::with_gpu(true);
        // 3 misses
        let _ = p.compile_shader("s1", &[1.0]);
        let _ = p.compile_shader("s2", &[2.0]);
        let _ = p.compile_shader("s3", &[3.0]);
        // 2 hits
        let _ = p.compile_shader("s1", &[1.0]);
        let _ = p.compile_shader("s2", &[2.0]);

        assert_eq!(p.stats().cache_misses, 3);
        assert_eq!(p.stats().cache_hits, 2);
    }

    #[test]
    fn stats_start_at_zero() {
        let p = GpuPipeline::with_gpu(true);
        assert_eq!(p.stats().total_dispatches, 0);
        assert_eq!(p.stats().cache_hits, 0);
        assert_eq!(p.stats().cache_misses, 0);
        assert_eq!(p.stats().avg_dispatch_time_us, 0);
        assert!((p.stats().gpu_utilization - 0.0).abs() < f64::EPSILON);
    }

    // -- Cache flush --------------------------------------------------------

    #[test]
    fn cache_flush_clears_entries() {
        let mut p = GpuPipeline::with_gpu(true);
        let _ = p.compile_shader("a", &[1.0]);
        let _ = p.compile_shader("b", &[2.0]);
        assert_eq!(p.shader_cache.len(), 2);
        p.flush_cache();
        assert!(p.shader_cache.is_empty());
    }

    #[test]
    fn cache_flush_empty_is_noop() {
        let mut p = GpuPipeline::with_gpu(true);
        assert!(p.shader_cache.is_empty());
        p.flush_cache();
        assert!(p.shader_cache.is_empty());
    }

    #[test]
    fn cache_flush_allows_recompile() {
        let mut p = GpuPipeline::with_gpu(true);
        let s1 = p.compile_shader("x", &[9.0]).unwrap();
        p.flush_cache();
        // After flush, compiling the same shader should be a miss, not a hit.
        let s2 = p.compile_shader("x", &[9.0]).unwrap();
        assert_eq!(s1.hash, s2.hash);
        // 1 miss (initial) + 1 miss (post-flush) = 2 misses, 0 hits.
        assert_eq!(p.stats().cache_misses, 2);
        assert_eq!(p.stats().cache_hits, 0);
    }

    // -- Device name --------------------------------------------------------

    #[test]
    fn device_name_set_correctly() {
        let p = GpuPipeline::with_gpu(true);
        assert!(!p.device_name.is_empty());
        let p2 = GpuPipeline::with_gpu(false);
        assert_eq!(p2.device_name, "None");
    }
}
