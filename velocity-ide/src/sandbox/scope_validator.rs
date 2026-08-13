// sandbox/scope_validator.rs — Semantic alignment check via cosine similarity
pub struct ScopeValidation {
    pub similarity: f32, // cosine_sim(output_vec, conditioning_vec)
    pub passed: bool,    // similarity >= threshold
    pub threshold: f32,  // current θ
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
            };
        }

        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;

        for (&a, &b) in output_vec.iter().zip(conditioning_vec.iter()) {
            dot += a * b;
            norm_a += a * a;
            norm_b += b * b;
        }

        let similarity = if norm_a > 1e-8 && norm_b > 1e-8 {
            dot / (norm_a.sqrt() * norm_b.sqrt())
        } else {
            0.0
        };

        // Cosine similarity ranges from -1.0 to 1.0.
        // Similarity must be at least the threshold.
        let passed = similarity >= threshold;

        ScopeValidation {
            similarity,
            passed,
            threshold,
        }
    }
}
