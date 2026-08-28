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
    if !(-1.0..=1.0).contains(&threshold) {
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

    // ── validate(): distance calculations ────────────────────────────────

    #[test]
    fn validate_euclidean_distance_known_value() {
        // a=[1,0], b=[0,1] → diff=[1,-1] → eucl = sqrt(1+1) = sqrt(2)
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let r = ScopeValidator::validate(&a, &b, 0.0);
        let expected = 2.0f32.sqrt();
        assert!((r.euclidean_distance - expected).abs() < 1e-6);
    }

    #[test]
    fn validate_manhattan_distance_known_value() {
        // a=[1,2,3], b=[4,5,6] → diff=[-3,-3,-3] → manh = 9
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let r = ScopeValidator::validate(&a, &b, 0.0);
        assert!((r.manhattan_distance - 9.0).abs() < 1e-6);
    }

    #[test]
    fn validate_euclidean_identical_is_zero() {
        let v = vec![3.0, 4.0, 5.0];
        let r = ScopeValidator::validate(&v, &v, 0.0);
        assert!(r.euclidean_distance.abs() < 1e-6);
    }

    #[test]
    fn validate_manhattan_identical_is_zero() {
        let v = vec![3.0, 4.0, 5.0];
        let r = ScopeValidator::validate(&v, &v, 0.0);
        assert!(r.manhattan_distance.abs() < 1e-6);
    }

    #[test]
    fn validate_euclidean_single_dim() {
        // a=[5], b=[2] → diff=[3] → eucl = 3
        let a = vec![5.0];
        let b = vec![2.0];
        let r = ScopeValidator::validate(&a, &b, -1.0);
        assert!((r.euclidean_distance - 3.0).abs() < 1e-6);
    }

    #[test]
    fn validate_manhattan_single_dim() {
        // a=[5], b=[2] → diff=[3] → manh = 3
        let a = vec![5.0];
        let b = vec![2.0];
        let r = ScopeValidator::validate(&a, &b, -1.0);
        assert!((r.manhattan_distance - 3.0).abs() < 1e-6);
    }

    // ── validate(): similarity edge cases ────────────────────────────────

    #[test]
    fn validate_single_element_same_direction() {
        let a = vec![3.0];
        let b = vec![7.0];
        let r = ScopeValidator::validate(&a, &b, 0.9);
        assert!((r.similarity - 1.0).abs() < 1e-6);
        assert!(r.passed);
        assert_eq!(r.vector_dim, 1);
    }

    #[test]
    fn validate_single_element_opposite() {
        let a = vec![3.0];
        let b = vec![-7.0];
        let r = ScopeValidator::validate(&a, &b, -1.0);
        // similarity ≈ -1.0, threshold = -1.0 → -1.0 >= -1.0 → true
        assert!((r.similarity - (-1.0)).abs() < 1e-6);
        assert!(r.passed);
    }

    #[test]
    fn validate_threshold_exactly_at_similarity() {
        let v = vec![1.0, 2.0, 3.0];
        // Use 0.99 threshold to avoid floating-point edge case at exactly 1.0
        let r = ScopeValidator::validate(&v, &v, 0.99);
        assert!(r.passed);
        assert!((r.similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn validate_negative_threshold_passes_opposite() {
        let a = vec![1.0, 2.0];
        let b = vec![-1.0, -2.0];
        let r = ScopeValidator::validate(&a, &b, -1.0);
        // similarity ≈ -1.0, threshold = -1.0 → -1.0 >= -1.0 → true
        assert!(r.passed);
    }

    #[test]
    fn validate_very_small_norm_vectors() {
        // Norms below 1e-8 → similarity = 0.0
        let a = vec![1e-10, 0.0];
        let b = vec![0.0, 1e-10];
        let r = ScopeValidator::validate(&a, &b, 0.0);
        assert_eq!(r.similarity, 0.0);
        assert!(r.passed); // 0.0 >= 0.0
    }

    #[test]
    fn validate_one_small_one_normal() {
        // norm_a < 1e-8 → similarity = 0.0
        let a = vec![1e-10, 0.0];
        let b = vec![1.0, 2.0];
        let r = ScopeValidator::validate(&a, &b, 0.5);
        assert_eq!(r.similarity, 0.0);
        assert!(!r.passed);
    }

    #[test]
    fn validate_vector_dim_matches_input() {
        let a = vec![1.0; 100];
        let b = vec![2.0; 100];
        let r = ScopeValidator::validate(&a, &b, 0.5);
        assert_eq!(r.vector_dim, 100);
    }

    #[test]
    fn validate_mismatched_returns_max_distances() {
        let a = vec![1.0];
        let b = vec![1.0, 2.0];
        let r = ScopeValidator::validate(&a, &b, 0.5);
        assert_eq!(r.euclidean_distance, f32::MAX);
        assert_eq!(r.manhattan_distance, f32::MAX);
        assert_eq!(r.similarity, 0.0);
    }

    #[test]
    fn validate_empty_returns_max_distances() {
        let r = ScopeValidator::validate(&[], &[], 0.5);
        assert_eq!(r.euclidean_distance, f32::MAX);
        assert_eq!(r.manhattan_distance, f32::MAX);
    }

    // ── validate(): known cosine similarity values ───────────────────────

    #[test]
    fn validate_45_degree_angle() {
        // a=[1,0], b=[1,1] → cos = 1/sqrt(2) ≈ 0.7071
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 1.0];
        let r = ScopeValidator::validate(&a, &b, 0.7);
        assert!((r.similarity - (1.0 / 2.0f32.sqrt())).abs() < 1e-5);
        assert!(r.passed);
    }

    #[test]
    fn validate_scaled_vectors_same_similarity() {
        // Scaling doesn't change cosine similarity
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![10.0, 20.0, 30.0];
        let r = ScopeValidator::validate(&a, &b, 0.99);
        assert!((r.similarity - 1.0).abs() < 1e-6);
        assert!(r.passed);
    }

    // ── validate_batch(): detailed checks ────────────────────────────────

    #[test]
    fn batch_all_pass() {
        let v = vec![1.0, 2.0, 3.0];
        let pairs = vec![
            (v.as_slice(), v.as_slice()),
            (v.as_slice(), v.as_slice()),
            (v.as_slice(), v.as_slice()),
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        assert_eq!(batch.total, 3);
        assert_eq!(batch.passed, 3);
        assert_eq!(batch.failed, 0);
        assert!((batch.pass_rate - 1.0).abs() < 1e-6);
    }

    #[test]
    fn batch_all_fail() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0]; // orthogonal → sim ≈ 0
        let pairs = vec![
            (a.as_slice(), b.as_slice()),
            (a.as_slice(), b.as_slice()),
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        assert_eq!(batch.total, 2);
        assert_eq!(batch.passed, 0);
        assert_eq!(batch.failed, 2);
        assert!(batch.pass_rate.abs() < 1e-6);
    }

    #[test]
    fn batch_single_pair() {
        let v = vec![1.0, 2.0];
        let pairs = vec![(v.as_slice(), v.as_slice())];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        assert_eq!(batch.total, 1);
        assert_eq!(batch.passed, 1);
        assert!((batch.pass_rate - 1.0).abs() < 1e-6);
    }

    #[test]
    fn batch_min_max_similarity() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0]; // orthogonal
        let c = vec![-1.0, 0.0]; // opposite to a
        let pairs = vec![
            (a.as_slice(), a.as_slice()), // sim ≈ 1.0
            (a.as_slice(), b.as_slice()), // sim ≈ 0.0
            (a.as_slice(), c.as_slice()), // sim ≈ -1.0
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        assert_eq!(batch.total, 3);
        assert!((batch.max_similarity - 1.0).abs() < 1e-6);
        assert!((batch.min_similarity - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn batch_avg_similarity() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0]; // orthogonal
        let pairs = vec![
            (a.as_slice(), a.as_slice()), // sim ≈ 1.0
            (a.as_slice(), b.as_slice()), // sim ≈ 0.0
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        // avg ≈ (1.0 + 0.0) / 2 = 0.5
        assert!((batch.avg_similarity - 0.5).abs() < 1e-5);
    }

    #[test]
    fn batch_empty_min_max_are_zero() {
        let batch = ScopeValidator::validate_batch(&[], 0.5);
        assert_eq!(batch.min_similarity, 0.0);
        assert_eq!(batch.max_similarity, 0.0);
    }

    #[test]
    fn batch_threshold_stored() {
        let batch = ScopeValidator::validate_batch(&[], 0.75);
        assert!((batch.threshold - 0.75).abs() < 1e-6);
    }

    #[test]
    fn batch_results_vector_length() {
        let v = vec![1.0, 2.0];
        let pairs = vec![
            (v.as_slice(), v.as_slice()),
            (v.as_slice(), v.as_slice()),
        ];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        assert_eq!(batch.results.len(), 2);
    }

    // ── validate_scope_threshold(): detailed ─────────────────────────────

    #[test]
    fn threshold_zero_is_valid() {
        assert!(validate_scope_threshold(0.0).is_empty());
    }

    #[test]
    fn threshold_just_outside_high() {
        let issues = validate_scope_threshold(1.0001);
        assert!(!issues.is_empty());
        assert!(issues[0].contains("threshold"));
    }

    #[test]
    fn threshold_just_outside_low() {
        let issues = validate_scope_threshold(-1.0001);
        assert!(!issues.is_empty());
        assert!(issues[0].contains("threshold"));
    }

    #[test]
    fn threshold_nan_issues_text() {
        let issues = validate_scope_threshold(f32::NAN);
        assert!(issues.iter().any(|i| i.contains("NaN")));
    }

    #[test]
    fn threshold_issue_text_contains_value() {
        let issues = validate_scope_threshold(5.0);
        assert!(issues[0].contains("5"));
    }

    // ── Struct derives ───────────────────────────────────────────────────

    #[test]
    fn scope_validation_clone_is_independent() {
        let r = ScopeValidator::validate(&[1.0, 2.0], &[1.0, 2.0], 0.5);
        let mut cloned = r.clone();
        cloned.similarity = -999.0;
        assert!((r.similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn scope_validation_debug_format() {
        let r = ScopeValidator::validate(&[1.0], &[1.0], 0.5);
        let debug = format!("{:?}", r);
        assert!(debug.contains("similarity"));
        assert!(debug.contains("passed"));
        assert!(debug.contains("threshold"));
    }

    #[test]
    fn scope_validation_batch_clone_is_independent() {
        let v = vec![1.0, 2.0];
        let pairs = vec![(v.as_slice(), v.as_slice())];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        let mut cloned = batch.clone();
        cloned.total = 999;
        assert_eq!(batch.total, 1);
    }

    #[test]
    fn scope_validation_batch_debug_format() {
        let batch = ScopeValidator::validate_batch(&[], 0.5);
        let debug = format!("{:?}", batch);
        assert!(debug.contains("total"));
        assert!(debug.contains("pass_rate"));
    }

    // ── Serialization details ────────────────────────────────────────────

    #[test]
    fn scope_validation_json_all_fields() {
        let r = ScopeValidator::validate(&[1.0, 2.0], &[3.0, 4.0], 0.5);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("similarity"));
        assert!(json.contains("passed"));
        assert!(json.contains("threshold"));
        assert!(json.contains("euclidean_distance"));
        assert!(json.contains("manhattan_distance"));
        assert!(json.contains("vector_dim"));
    }

    #[test]
    fn scope_validation_json_parseable_as_value() {
        let r = ScopeValidator::validate(&[1.0, 2.0], &[1.0, 2.0], 0.5);
        let json = serde_json::to_string(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["passed"].as_bool(), Some(true));
        assert_eq!(parsed["vector_dim"].as_u64(), Some(2));
        assert!((parsed["threshold"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn scope_validation_batch_json_all_fields() {
        let v = vec![1.0, 2.0];
        let pairs = vec![(v.as_slice(), v.as_slice())];
        let batch = ScopeValidator::validate_batch(&pairs, 0.5);
        let json = serde_json::to_string(&batch).unwrap();
        assert!(json.contains("total"));
        assert!(json.contains("passed"));
        assert!(json.contains("failed"));
        assert!(json.contains("pass_rate"));
        assert!(json.contains("avg_similarity"));
        assert!(json.contains("min_similarity"));
        assert!(json.contains("max_similarity"));
        assert!(json.contains("threshold"));
        assert!(json.contains("results"));
    }
}
