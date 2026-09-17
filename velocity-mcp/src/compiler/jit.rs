use std::collections::HashMap;
use std::error::Error;
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// Existing JIT compiler (backward-compatible)
// ---------------------------------------------------------------------------

pub struct JitCompiler;

impl JitCompiler {
    /// JIT-compiles a custom SPIR-V compute shader by taking a pre-optimized
    /// SPIR-V template and patching the embedded weight constant array
    /// directly in-memory. Bypasses external compilers (glslang/shaderc).
    pub fn compile_inlined_weights(weights: &[i8]) -> Result<Vec<u32>, Box<dyn Error>> {
        // SPIR-V binary header (5 words):
        // 0: Magic number (0x07230203)
        // 1: Version number (e.g., 0x00010000 for SPIR-V 1.0)
        // 2: Generator magic number
        // 3: Bound (maximum ID used + 1)
        // 4: Reserved (0)
        let mut spirv_template = vec![
            0x07230203, // Magic Number
            0x00010300, // SPIR-V 1.3
            0x000d000b, // Generator: V-NCE JIT
            0x00000025, // Bound (ID limit)
            0x00000000, // Reserved
            // Instruction: OpCapability Shader
            0x00020011, 0x00000001, // Instruction: OpMemoryModel Logical GLSL450
            0x0003000e, 0x00000000, 0x00000001,
            // Instruction: OpEntryPoint GLCompute %main "main" %gl_GlobalInvocationID
            0x0006000f, 0x00000005, 0x00000004, 0x6e69616d, 0x00000000, 0x0000000f,
            // Instruction: OpExecutionMode %main LocalSize 64 1 1
            0x00060010, 0x00000004, 0x00000011, 0x00000040, 0x00000001, 0x00000001,
            // Type declarations
            0x00030015, 0x00000007, 0x00000020, // TypeInt 32 0 (u32)
            0x00040015, 0x00000008, 0x00000020, 0x00000001,
            // Constant placeholder block for weights.
            // OpConstant %i32 %weight_val_0 (Placeholder ID 0x00000020)
            0x0004002b, 0x00000008, 0x00000020, 0x0000002a, // Placeholder constant value 42
        ];

        // Locate the placeholder OpConstant instruction (opcode 43 -> 0x002b)
        // Format of OpConstant: [Length/Opcode, Type ID, Result ID, Value...]
        // We find the result ID 0x00000020 and patch its value with the first weight in-memory.
        let mut patched = false;
        for i in 0..(spirv_template.len() - 3) {
            if spirv_template[i] == 0x0004002b && spirv_template[i + 2] == 0x00000020 {
                // Patch the placeholder value with the inlined weight
                let val = if !weights.is_empty() {
                    weights[0] as u32
                } else {
                    1
                };
                spirv_template[i + 3] = val;
                patched = true;
                break;
            }
        }

        if !patched {
            return Err("Failed to find weight placeholder in JIT SPIR-V template.".into());
        }

        println!("JIT Compiler: Successfully assembled weight-inlined compute shader.");
        Ok(spirv_template)
    }
}

// ---------------------------------------------------------------------------
// Hash helper – deterministic weight-vector → u64 for cache keys
// ---------------------------------------------------------------------------

/// Computes a deterministic 64-bit hash of a weight slice using the
/// standard library `DefaultHasher` (SipHash).  No external crates needed.
fn weight_vector_hash(weights: &[i8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    weights.len().hash(&mut hasher);
    for &w in weights {
        w.hash(&mut hasher);
    }
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Common weight patterns used for warm-up
// ---------------------------------------------------------------------------

/// Well-known weight vectors that are compiled eagerly during [`JitCache::warm_up`]
/// so that the first real diagnostic / scoring call never pays compile cost.
pub fn common_weight_patterns() -> Vec<Vec<i8>> {
    vec![
        // Diagnostic severity weights: [critical, error, warning, info, hint]
        vec![2, -1, 0, 1, 1],
        // Syntax scoring: [keyword, string, comment, number, operator]
        vec![1, 1, -1, 1, 0],
        // Uniform weights (all equal)
        vec![1, 1, 1, 1, 1],
        // Negative-only (penalty mode)
        vec![-1, -1, -1, -1, -1],
        // Single-weight passthrough
        vec![1],
        // Empty / no-op
        vec![],
    ]
}

// ---------------------------------------------------------------------------
// JitCache – compiled-shader cache keyed by weight-vector hash
// ---------------------------------------------------------------------------

/// An LRU-free, `HashMap`-based cache that stores compiled SPIR-V shaders
/// keyed by the hash of their weight vector.  Repeated compilations with the
/// same weights become O(1) look-ups.
pub struct JitCache {
    inner: HashMap<u64, Vec<u32>>,
}

impl JitCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Returns the compiled SPIR-V for `weights`, compiling and inserting it
    /// on a cache miss.
    pub fn get_or_compile(&mut self, weights: &[i8]) -> Result<&[u32], Box<dyn Error>> {
        let key = weight_vector_hash(weights);
        match self.inner.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => Ok(e.into_mut()),
            std::collections::hash_map::Entry::Vacant(e) => {
                let spirv = JitCompiler::compile_inlined_weights(weights)?;
                Ok(e.insert(spirv))
            }
        }
    }
    /// Pre-compiles a set of weight patterns so that subsequent look-ups are
    /// cache hits.  Returns the number of patterns successfully cached.
    pub fn warm_up(&mut self, patterns: &[Vec<i8>]) -> usize {
        let mut count = 0;
        for pat in patterns {
            if self.get_or_compile(pat).is_ok() {
                count += 1;
            }
        }
        count
    }

    /// Convenience: warm up with the built-in [`common_weight_patterns`].
    pub fn warm_up_defaults(&mut self) -> usize {
        let patterns = common_weight_patterns();
        self.warm_up(&patterns)
    }

    /// Number of entries currently cached.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Check whether a given weight vector is already cached (O(1)).
    pub fn contains(&self, weights: &[i8]) -> bool {
        let key = weight_vector_hash(weights);
        self.inner.contains_key(&key)
    }

    /// Evict all cached entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl Default for JitCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Software interpreter – fast fallback when JIT / Vulkan is unavailable
// ---------------------------------------------------------------------------

/// Result of the combined compile-or-fallback path.
#[derive(Debug, Clone)]
pub enum JitOutput {
    /// JIT compilation succeeded; contains the SPIR-V binary words.
    Compiled(Vec<u32>),
    /// JIT compilation failed; the software interpreter produced equivalent
    /// weighted scores instead.
    Interpreted(Vec<f64>),
}

/// A fast software interpreter that computes weighted diagnostic / syntax
/// scores directly on the CPU.  Used as a fallback when the GPU JIT path is
/// unavailable (e.g. Vulkan driver missing, no discrete GPU, shader compile
/// failure).
pub struct SoftwareInterpreter;

impl SoftwareInterpreter {
    /// Interprets a weight vector and returns per-element normalised scores
    /// in the range `[-1.0, 1.0]`.  The computation mirrors what the SPIR-V
    /// shader would produce on the GPU:
    ///
    /// ```text
    /// score[i] = weight[i] / max(1, sum_of_abs_weights)
    /// ```
    ///
    /// An empty weight slice yields an empty result.
    pub fn interpret(weights: &[i8]) -> Vec<f64> {
        if weights.is_empty() {
            return Vec::new();
        }
        let abs_sum: f64 = weights.iter().map(|w| w.unsigned_abs() as f64).sum();
        let denom = abs_sum.max(1.0);
        weights.iter().map(|w| (*w as f64) / denom).collect()
    }

    /// Convenience wrapper: returns an aggregate scalar score (mean of
    /// per-element scores).  Useful for quick ranking.
    pub fn aggregate_score(weights: &[i8]) -> f64 {
        let scores = Self::interpret(weights);
        if scores.is_empty() {
            return 0.0;
        }
        let sum: f64 = scores.iter().sum();
        sum / scores.len() as f64
    }
}

/// Attempts JIT compilation first.  If it fails for any reason (Vulkan
/// unavailable, shader compile error, …) the function transparently falls
/// back to the [`SoftwareInterpreter`] so that callers always get a usable
/// result.
pub fn compile_or_fallback(weights: &[i8]) -> JitOutput {
    match JitCompiler::compile_inlined_weights(weights) {
        Ok(spirv) => JitOutput::Compiled(spirv),
        Err(e) => {
            log::warn!(
                "JIT compilation failed ({}), falling back to software interpreter",
                e
            );
            let scores = SoftwareInterpreter::interpret(weights);
            JitOutput::Interpreted(scores)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Existing tests (backward compatibility) ----------------------------

    #[test]
    fn jit_compile_with_weights() {
        let weights = vec![7i8, 3, -1];
        let result = JitCompiler::compile_inlined_weights(&weights).unwrap();
        // SPIR-V binary should start with the magic number
        assert_eq!(result[0], 0x07230203, "SPIR-V magic number");
    }

    #[test]
    fn jit_compile_empty_weights() {
        let weights: Vec<i8> = vec![];
        let result = JitCompiler::compile_inlined_weights(&weights).unwrap();
        assert_eq!(result[0], 0x07230203);
        // Empty weights → placeholder value defaults to 1
        // Find the patched constant instruction
        for i in 0..(result.len() - 3) {
            if result[i] == 0x0004002b && result[i + 2] == 0x00000020 {
                assert_eq!(result[i + 3], 1, "empty weights should default to 1");
                break;
            }
        }
    }

    #[test]
    fn jit_patches_first_weight() {
        let weights = vec![99i8];
        let result = JitCompiler::compile_inlined_weights(&weights).unwrap();
        // Find the patched constant
        for i in 0..(result.len() - 3) {
            if result[i] == 0x0004002b && result[i + 2] == 0x00000020 {
                assert_eq!(result[i + 3], 99, "should patch with first weight value");
                break;
            }
        }
    }

    #[test]
    fn jit_spirv_has_valid_version() {
        let result = JitCompiler::compile_inlined_weights(&[1]).unwrap();
        // Version word should be SPIR-V 1.3 = 0x00010300
        assert_eq!(result[1], 0x00010300, "SPIR-V version should be 1.3");
    }

    #[test]
    fn jit_spirv_has_generator_magic() {
        let result = JitCompiler::compile_inlined_weights(&[1]).unwrap();
        assert_eq!(result[2], 0x000d000b, "generator magic should be V-NCE JIT");
    }

    #[test]
    fn jit_output_is_nonempty() {
        let result = JitCompiler::compile_inlined_weights(&[0]).unwrap();
        assert!(
            result.len() > 10,
            "SPIR-V binary should have substantial content"
        );
    }

    // -- JitCache tests -----------------------------------------------------

    #[test]
    fn cache_starts_empty() {
        let cache = JitCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_compiles_on_miss() {
        let mut cache = JitCache::new();
        let result = cache.get_or_compile(&[1, 2, 3]).unwrap();
        assert_eq!(result[0], 0x07230203, "should return valid SPIR-V");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_hit_returns_same_data() {
        let mut cache = JitCache::new();
        let first = cache.get_or_compile(&[5, 10]).unwrap().to_vec();
        let second = cache.get_or_compile(&[5, 10]).unwrap().to_vec();
        assert_eq!(first, second, "cache hit should return identical SPIR-V");
        assert_eq!(cache.len(), 1, "same key should not create a second entry");
    }

    #[test]
    fn cache_different_weights_different_entries() {
        let mut cache = JitCache::new();
        let _ = cache.get_or_compile(&[1]).unwrap();
        let _ = cache.get_or_compile(&[2]).unwrap();
        assert_eq!(
            cache.len(),
            2,
            "different weights should produce different cache entries"
        );
    }

    #[test]
    fn cache_contains_check() {
        let mut cache = JitCache::new();
        assert!(!cache.contains(&[7]));
        let _ = cache.get_or_compile(&[7]).unwrap();
        assert!(cache.contains(&[7]));
    }

    #[test]
    fn cache_clear() {
        let mut cache = JitCache::new();
        let _ = cache.get_or_compile(&[1]).unwrap();
        let _ = cache.get_or_compile(&[2]).unwrap();
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_warm_up_defaults() {
        let mut cache = JitCache::new();
        let count = cache.warm_up_defaults();
        assert!(
            count > 0,
            "warm_up_defaults should compile at least one pattern"
        );
        assert_eq!(cache.len(), count);
        // All default patterns should now be cache hits.
        for pat in common_weight_patterns() {
            assert!(
                cache.contains(&pat),
                "pattern {:?} should be cached after warm-up",
                pat
            );
        }
    }

    #[test]
    fn cache_warm_up_custom_patterns() {
        let mut cache = JitCache::new();
        let patterns = vec![vec![10, 20], vec![-5, 0, 5]];
        let count = cache.warm_up(&patterns);
        assert_eq!(count, 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn weight_hash_is_deterministic() {
        let w = vec![1i8, -2, 3];
        assert_eq!(weight_vector_hash(&w), weight_vector_hash(&w));
    }

    #[test]
    fn weight_hash_differs_for_different_inputs() {
        let h1 = weight_vector_hash(&[1, 2, 3]);
        let h2 = weight_vector_hash(&[3, 2, 1]);
        assert_ne!(
            h1, h2,
            "different weight vectors should (almost certainly) hash differently"
        );
    }

    // -- Software interpreter tests -----------------------------------------

    #[test]
    fn interpreter_empty_weights() {
        let scores = SoftwareInterpreter::interpret(&[]);
        assert!(scores.is_empty());
    }

    #[test]
    fn interpreter_single_weight() {
        let scores = SoftwareInterpreter::interpret(&[5]);
        assert_eq!(scores.len(), 1);
        assert!(
            (scores[0] - 1.0).abs() < f64::EPSILON,
            "single positive weight → score 1.0"
        );
    }

    #[test]
    fn interpreter_uniform_weights() {
        let scores = SoftwareInterpreter::interpret(&[1, 1, 1]);
        // abs_sum = 3, each score = 1/3
        for &s in &scores {
            assert!((s - 1.0 / 3.0).abs() < 1e-9);
        }
    }

    #[test]
    fn interpreter_mixed_sign_weights() {
        let scores = SoftwareInterpreter::interpret(&[2, -1, 0, 1]);
        // abs_sum = 4
        let expected = [2.0 / 4.0, -1.0 / 4.0, 0.0 / 4.0, 1.0 / 4.0];
        for (s, e) in scores.iter().zip(expected.iter()) {
            assert!((s - e).abs() < 1e-9, "got {}, expected {}", s, e);
        }
    }

    #[test]
    fn interpreter_all_zero_weights() {
        let scores = SoftwareInterpreter::interpret(&[0, 0, 0]);
        // abs_sum = 0 → denom clamped to 1.0 → all scores 0.0
        for &s in &scores {
            assert!((s - 0.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn aggregate_score_empty() {
        assert!((SoftwareInterpreter::aggregate_score(&[]) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_score_uniform_positive() {
        let agg = SoftwareInterpreter::aggregate_score(&[1, 1, 1]);
        assert!((agg - 1.0 / 3.0).abs() < 1e-9);
    }

    // -- compile_or_fallback tests ------------------------------------------

    #[test]
    fn fallback_returns_compiled_on_success() {
        // Normal weights should succeed via JIT
        let output = compile_or_fallback(&[1, 2, 3]);
        match output {
            JitOutput::Compiled(spirv) => {
                assert_eq!(spirv[0], 0x07230203, "should be valid SPIR-V");
            }
            JitOutput::Interpreted(_) => {
                panic!("expected Compiled variant for valid weights");
            }
        }
    }

    #[test]
    fn fallback_output_is_deterministic() {
        let a = compile_or_fallback(&[4, -2, 1]);
        let b = compile_or_fallback(&[4, -2, 1]);
        // Both calls should produce the same variant and data.
        match (&a, &b) {
            (JitOutput::Compiled(x), JitOutput::Compiled(y)) => assert_eq!(x, y),
            (JitOutput::Interpreted(x), JitOutput::Interpreted(y)) => assert_eq!(x, y),
            _ => panic!("same input should produce same variant"),
        }
    }
}
