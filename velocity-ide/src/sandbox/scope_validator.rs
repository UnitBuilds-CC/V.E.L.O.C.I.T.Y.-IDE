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

    /// Validate multiple output/conditioning pairs and return a summary.
    pub fn validate_batch(
        pairs: &[(&[f32], &[f32])],
        threshold: f32,
    ) -> ScopeValidationBatch {
        let results: Vec<ScopeValidation> = pairs
            .iter()
            .map(|(o, c)| Self::validate(o, c, threshold))
            .collect();
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let avg_similarity = if total > 0 {
            results.iter().map(|r| r.similarity).sum::<f32>() / total as f32
        } else {
            0.0
        };
        let min_similarity = results.iter().map(|r| r.similarity).fold(f32::INFINITY, f32::min);
        let max_similarity = results.iter().map(|r| r.similarity).fold(f32::NEG_INFINITY, f32::max);

        ScopeValidationBatch {
            total,
            passed,
            failed: total - passed,
            pass_rate: if total > 0 { passed as f64 / total as f64 } else { 0.0 },
            avg_similarity,
            min_similarity: if min_similarity.is_infinite() { 0.0 } else { min_similarity },
            max_similarity: if max_similarity.is_infinite() { 0.0 } else { max_similarity },
            threshold,
            results,
        }
    }
}

/// Batch scope validation summary.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeValidationBatch {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f64,
    pub avg_similarity: f32,
    pub min_similarity: f32,
    pub max_similarity: f32,
    pub threshold: f32,
    pub results: Vec<ScopeValidation>,
}

/// Validate that the threshold is in the valid range [-1.0, 1.0].
pub fn validate_scope_threshold(threshold: f32) -> Vec<String> {
    let mut issues = Vec::new();
    if threshold < -1.0 || threshold > 1.0 {
        issues.push(format!(
            "threshold {} is outside valid cosine similarity range [-1.0, 1.0]",
            threshold
        ));
    }
    if threshold.is_nan() {
        issues.push("threshold is NaN".to_string());
    }
    issues
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let result = ScopeValidator::validate(&v, &v, 0.99);
        assert!(result.passed);
        assert!((result.similarity - 1.0).abs() < 1e-6);
        assert_eq!(result.vector_dim, 3);
        assert!((result.euclidean_distance - 0.0).abs() < 1e-6);
        assert!((result.manhattan_distance - 0.0).abs() < 1e-6);
    }

    #[test]
    fn validate_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let result = ScopeValidator::validate(&a, &b, 0.5);
        assert!(!result.passed);
        assert!(result.similarity.abs() < 1e-6);
    }

    #[test]
    fn validate_opposite_vectors() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let result = ScopeValidator::validate(&a, &b, -0.5);
        assert!(!result.passed); // similarity should be -1.0
        assert!((result.similarity - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn validate_mismatched_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let result = ScopeValidator::validate(&a, &b, 0.5);
        assert!(!result.passed);
        assert_eq!(result.vector_dim, 0);
        assert_eq!(result.euclidean_distance, f32::MAX);
    }

    #[test]
    fn validate_empty_vectors() {
        let result = ScopeValidator::validate(&[], &[], 0.5);
        assert!(!result.passed);
        assert_eq!(result.vector_dim, 0);
    }

    #[test]
    fn validate_zero_vectors() {
        let v = vec![0.0, 0.0, 0.0];
        // Zero vectors have similarity 0.0; threshold 0.5 means it fails
        let result = ScopeValidator::validate(&v, &v, 0.5);
        assert!(!result.passed);
        assert_eq!(result.similarity, 0.0);
    }

    #[test]
    fn validate_batch_mixed() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0]; // identical
        let c = vec![-1.0, -2.0, -3.0]; // opposite
        let pairs = vec![
            (a.as_slice(), b.as_slice()),
            (a.as_slice(), c.as_slice()),
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.9);
        assert_eq!(batch.total, 2);
        assert_eq!(batch.passed, 1); // only identical passes
        assert_eq!(batch.failed, 1);
        assert!((batch.pass_rate - 0.5).abs() < 1e-6);
    }

    #[test]
    fn validate_batch_empty() {
        let batch = ScopeValidator::validate_batch(&[], 0.5);
        assert_eq!(batch.total, 0);
        assert_eq!(batch.passed, 0);
        assert!((batch.pass_rate - 0.0).abs() < 1e-6);
    }

    #[test]
    fn validate_scope_threshold_valid() {
        assert!(validate_scope_threshold(0.5).is_empty());
        assert!(validate_scope_threshold(-1.0).is_empty());
        assert!(validate_scope_threshold(1.0).is_empty());
    }

    #[test]
    fn validate_scope_threshold_invalid() {
        let issues = validate_scope_threshold(1.5);
        assert!(!issues.is_empty());
        let issues = validate_scope_threshold(-2.0);
        assert!(!issues.is_empty());
    }

    #[test]
    fn validate_scope_threshold_nan() {
        let issues = validate_scope_threshold(f32::NAN);
        assert!(!issues.is_empty());
    }

    #[test]
    fn scope_validation_serializable() {
        let result = ScopeValidator::validate(&[1.0, 2.0], &[1.0, 2.0], 0.5);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("similarity"));
        assert!(json.contains("passed"));
    }

    #[test]
    fn scope_validation_batch_serializable() {
        let a = vec![1.0, 0.0];
        let pairs = vec![(a.as_slice(), a.as_slice())];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        let json = serde_json::to_string(&batch).unwrap();
        assert!(json.contains("pass_rate"));
        assert!(json.contains("total"));
    }
}
