// sandbox/scope_validator.rs — Semantic alignment check via cosine similarity
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ScopeValidation {
    pub similarity: f32, // cosine_sim(output_vec, conditioning_vec)
    pub passed: bool,    // similarity >= threshold
    pub threshold: f32,  // current θ
    /// Euclidean distance between output and conditioning vectors.
    pub euclidean_distance: f32,
    /// Manhattan distance between output and conditioning vectors.
    pub manhattan_distance: f32,
    /// Dimension of the compared vectors.
    pub vector_dim: usize,
}

pub struct ScopeValidator;

impl ScopeValidator {
    pub fn validate(
        output_vec: &[f32],
        conditioning_vec: &[f32],
        threshold: f32,
    ) -> ScopeValidation {
        if output_vec.len() != conditioning_vec.len() || output_vec.is_empty() {
            return ScopeValidation {
                similarity: 0.0,
                passed: false,
                threshold,
                euclidean_distance: f32::MAX,
                manhattan_distance: f32::MAX,
                vector_dim: 0,
            };
        }

        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;
        let mut eucl_sq = 0.0f32;
        let mut manh = 0.0f32;

        for (&a, &b) in output_vec.iter().zip(conditioning_vec.iter()) {
            dot += a * b;
            norm_a += a * a;
            norm_b += b * b;
            let diff = a - b;
            eucl_sq += diff * diff;
            manh += diff.abs();
        }

        let similarity = if norm_a > 1e-8 && norm_b > 1e-8 {
            dot / (norm_a.sqrt() * norm_b.sqrt())
        } else {
            0.0
        };

        let euclidean_distance = eucl_sq.sqrt();
        let manhattan_distance = manh;

        // Cosine similarity ranges from -1.0 to 1.0.
        // Similarity must be at least the threshold.
        let passed = similarity >= threshold;

        ScopeValidation {
            similarity,
            passed,
            threshold,
            euclidean_distance,
            manhattan_distance,
            vector_dim: output_vec.len(),
        }
    }
}
