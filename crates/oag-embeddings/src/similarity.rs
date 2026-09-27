//! Pure, DB-free vector ranking. Spec section 64 explicitly rules out a
//! mandatory external vector database or SQLite vector extension for v1 —
//! brute-force cosine similarity over embeddings already loaded into memory
//! is correct v1 scope (see OPEN_QUESTIONS.md for the "swap in a real ANN
//! index later" deferral).

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Ranks `candidates` by cosine similarity to `query`, highest first,
/// truncated to `limit`. Ties break by input order (stable sort).
pub fn rank<T: Clone>(query: &[f32], candidates: &[(T, Vec<f32>)], limit: usize) -> Vec<(T, f32)> {
    let mut scored: Vec<(T, f32)> = candidates
        .iter()
        .map(|(key, vector)| (key.clone(), cosine_similarity(query, vector)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_have_similarity_one() {
        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_have_similarity_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn opposite_vectors_have_similarity_negative_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn mismatched_lengths_are_zero_not_a_panic() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn zero_vector_is_zero_not_nan() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 2.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn rank_orders_by_similarity_descending_and_truncates() {
        let query = vec![1.0, 0.0];
        let candidates = vec![
            ("orthogonal", vec![0.0, 1.0]),
            ("identical", vec![1.0, 0.0]),
            ("opposite", vec![-1.0, 0.0]),
        ];
        let ranked = rank(&query, &candidates, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, "identical");
        assert_eq!(ranked[1].0, "orthogonal");
    }
}
