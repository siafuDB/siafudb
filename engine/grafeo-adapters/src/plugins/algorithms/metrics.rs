//! Partition quality metrics for comparing community assignments.
//!
//! These metrics evaluate how similar two partitions of the same node set are.
//! They are used by the GraphChallenge streaming benchmark to compare inferred
//! partitions against ground truth, but are general-purpose: any pair of
//! node-to-community maps can be compared.
//!
//! | Metric | Range | 1.0 means |
//! | ------ | ----- | --------- |
//! | Rand Index | \[0, 1\] | Identical partitions |
//! | Adjusted Rand Index | \[-1, 1\] | Identical (0 = random) |
//! | Normalized Mutual Information | \[0, 1\] | Identical partitions |
//! | Pairwise Precision | \[0, 1\] | Every predicted co-cluster is true |
//! | Pairwise Recall | \[0, 1\] | Every true co-cluster is predicted |

use grafeo_common::types::NodeId;
use grafeo_common::utils::hash::FxHashMap;

/// Computes the Rand Index between two partitions.
///
/// The Rand Index measures the fraction of node pairs where both partitions
/// agree: either both place the pair in the same community, or both place
/// them in different communities.
///
/// # Range
///
/// \[0, 1\] where 1.0 means identical partitions.
///
/// # Complexity
///
/// O(n^2) where n is the number of nodes.
#[must_use]
pub fn rand_index(
    partition_a: &FxHashMap<NodeId, usize>,
    partition_b: &FxHashMap<NodeId, usize>,
) -> f64 {
    let nodes: Vec<NodeId> = partition_a.keys().copied().collect();
    let n = nodes.len();
    if n < 2 {
        return 1.0;
    }

    let mut agree = 0u64;
    let mut total = 0u64;

    for i in 0..n {
        for j in (i + 1)..n {
            let a_same = partition_a.get(&nodes[i]) == partition_a.get(&nodes[j]);
            let b_same = partition_b.get(&nodes[i]) == partition_b.get(&nodes[j]);
            if a_same == b_same {
                agree += 1;
            }
            total += 1;
        }
    }

    agree as f64 / total as f64
}

/// Computes the Adjusted Rand Index between two partitions.
///
/// Corrects the Rand Index for chance agreement. A value of 0.0 indicates
/// random-level agreement, while 1.0 indicates identical partitions.
///
/// Uses the contingency table approach for efficient computation.
///
/// # Range
///
/// \[-1, 1\] where 1.0 = identical, 0.0 = random agreement.
///
/// # Complexity
///
/// O(n) using contingency table (not O(n^2) pairwise).
#[must_use]
pub fn adjusted_rand_index(
    partition_a: &FxHashMap<NodeId, usize>,
    partition_b: &FxHashMap<NodeId, usize>,
) -> f64 {
    let nodes: Vec<NodeId> = partition_a.keys().copied().collect();
    let n = nodes.len();
    if n < 2 {
        return 1.0;
    }

    // Build contingency table: n_ij = number of nodes in community i of A and community j of B.
    let mut contingency: FxHashMap<(usize, usize), u64> = FxHashMap::default();
    for &node in &nodes {
        let a = partition_a.get(&node).copied().unwrap_or(0);
        let b = partition_b.get(&node).copied().unwrap_or(0);
        *contingency.entry((a, b)).or_default() += 1;
    }

    // Row sums (a_i) and column sums (b_j).
    let mut row_sums: FxHashMap<usize, u64> = FxHashMap::default();
    let mut col_sums: FxHashMap<usize, u64> = FxHashMap::default();
    for (&(a, b), &count) in &contingency {
        *row_sums.entry(a).or_default() += count;
        *col_sums.entry(b).or_default() += count;
    }

    // Sum of C(n_ij, 2) over all cells.
    let sum_comb_nij: f64 = contingency.values().map(|&v| choose2(v)).sum();

    // Sum of C(a_i, 2) over rows.
    let sum_comb_a: f64 = row_sums.values().map(|&v| choose2(v)).sum();

    // Sum of C(b_j, 2) over columns.
    let sum_comb_b: f64 = col_sums.values().map(|&v| choose2(v)).sum();

    let total_comb = choose2(n as u64);

    if total_comb == 0.0 {
        return 1.0;
    }

    let expected = sum_comb_a * sum_comb_b / total_comb;
    let max_index = f64::midpoint(sum_comb_a, sum_comb_b);

    if (max_index - expected).abs() < 1e-15 {
        // All nodes in one cluster in both partitions.
        return 1.0;
    }

    (sum_comb_nij - expected) / (max_index - expected)
}

/// Computes the Normalized Mutual Information between two partitions.
///
/// NMI measures the amount of information shared between two partitions,
/// normalized to \[0, 1\]. Uses the arithmetic mean normalization.
///
/// # Range
///
/// \[0, 1\] where 1.0 means identical partitions.
///
/// # Complexity
///
/// O(n) using contingency table.
#[must_use]
pub fn normalized_mutual_information(
    partition_a: &FxHashMap<NodeId, usize>,
    partition_b: &FxHashMap<NodeId, usize>,
) -> f64 {
    let nodes: Vec<NodeId> = partition_a.keys().copied().collect();
    let n = nodes.len();
    if n == 0 {
        return 1.0;
    }
    let n_f = n as f64;

    // Build contingency table.
    let mut contingency: FxHashMap<(usize, usize), u64> = FxHashMap::default();
    for &node in &nodes {
        let a = partition_a.get(&node).copied().unwrap_or(0);
        let b = partition_b.get(&node).copied().unwrap_or(0);
        *contingency.entry((a, b)).or_default() += 1;
    }

    let mut row_sums: FxHashMap<usize, u64> = FxHashMap::default();
    let mut col_sums: FxHashMap<usize, u64> = FxHashMap::default();
    for (&(a, b), &count) in &contingency {
        *row_sums.entry(a).or_default() += count;
        *col_sums.entry(b).or_default() += count;
    }

    // Mutual Information: MI = sum_{ij} (n_ij / n) * log(n * n_ij / (a_i * b_j))
    let mut mi = 0.0f64;
    for (&(a, b), &n_ij) in &contingency {
        if n_ij == 0 {
            continue;
        }
        let a_i = row_sums[&a] as f64;
        let b_j = col_sums[&b] as f64;
        let n_ij_f = n_ij as f64;
        mi += (n_ij_f / n_f) * (n_f * n_ij_f / (a_i * b_j)).ln();
    }

    // Entropy of A: H(A) = -sum_i (a_i / n) * log(a_i / n)
    let h_a: f64 = row_sums
        .values()
        .map(|&a_i| {
            let p = a_i as f64 / n_f;
            if p > 0.0 { -p * p.ln() } else { 0.0 }
        })
        .sum();

    // Entropy of B: H(B) = -sum_j (b_j / n) * log(b_j / n)
    let h_b: f64 = col_sums
        .values()
        .map(|&b_j| {
            let p = b_j as f64 / n_f;
            if p > 0.0 { -p * p.ln() } else { 0.0 }
        })
        .sum();

    let denom = f64::midpoint(h_a, h_b);
    if denom < 1e-15 {
        // Both partitions have zero entropy (all in one cluster).
        return 1.0;
    }

    mi / denom
}

/// Computes pairwise precision: the fraction of node pairs placed in the same
/// predicted community that are also in the same true community.
///
/// # Range
///
/// \[0, 1\] where 1.0 means every predicted co-cluster pair is correct.
///
/// # Complexity
///
/// O(n^2) where n is the number of nodes.
#[must_use]
pub fn pairwise_precision(
    predicted: &FxHashMap<NodeId, usize>,
    truth: &FxHashMap<NodeId, usize>,
) -> f64 {
    let nodes: Vec<NodeId> = predicted.keys().copied().collect();
    let n = nodes.len();
    if n < 2 {
        return 1.0;
    }

    let mut true_positive = 0u64;
    let mut predicted_positive = 0u64;

    for i in 0..n {
        for j in (i + 1)..n {
            let pred_same = predicted.get(&nodes[i]) == predicted.get(&nodes[j]);
            if pred_same {
                predicted_positive += 1;
                let true_same = truth.get(&nodes[i]) == truth.get(&nodes[j]);
                if true_same {
                    true_positive += 1;
                }
            }
        }
    }

    if predicted_positive == 0 {
        return 1.0;
    }

    true_positive as f64 / predicted_positive as f64
}

/// Computes pairwise recall: the fraction of node pairs in the same true
/// community that are also placed in the same predicted community.
///
/// # Range
///
/// \[0, 1\] where 1.0 means every true co-cluster pair is predicted.
///
/// # Complexity
///
/// O(n^2) where n is the number of nodes.
#[must_use]
pub fn pairwise_recall(
    predicted: &FxHashMap<NodeId, usize>,
    truth: &FxHashMap<NodeId, usize>,
) -> f64 {
    let nodes: Vec<NodeId> = predicted.keys().copied().collect();
    let n = nodes.len();
    if n < 2 {
        return 1.0;
    }

    let mut true_positive = 0u64;
    let mut condition_positive = 0u64;

    for i in 0..n {
        for j in (i + 1)..n {
            let true_same = truth.get(&nodes[i]) == truth.get(&nodes[j]);
            if true_same {
                condition_positive += 1;
                let pred_same = predicted.get(&nodes[i]) == predicted.get(&nodes[j]);
                if pred_same {
                    true_positive += 1;
                }
            }
        }
    }

    if condition_positive == 0 {
        return 1.0;
    }

    true_positive as f64 / condition_positive as f64
}

/// Binomial coefficient C(n, 2) = n * (n-1) / 2.
fn choose2(n: u64) -> f64 {
    if n < 2 {
        0.0
    } else {
        (n * (n - 1)) as f64 / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a partition from pairs of (node_id_u64, community).
    fn partition(pairs: &[(u64, usize)]) -> FxHashMap<NodeId, usize> {
        pairs
            .iter()
            .map(|&(id, comm)| (NodeId::new(id), comm))
            .collect()
    }

    #[test]
    fn test_rand_index_identical() {
        let a = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let b = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let ri = rand_index(&a, &b);
        assert!(
            (ri - 1.0).abs() < 1e-10,
            "Identical partitions should give RI=1.0, got {ri}"
        );
    }

    #[test]
    fn test_rand_index_different() {
        // All same vs all different
        let a = partition(&[(0, 0), (1, 0), (2, 0), (3, 0)]);
        let b = partition(&[(0, 0), (1, 1), (2, 2), (3, 3)]);
        let ri = rand_index(&a, &b);
        // 6 pairs: a says all same, b says all different => 0 agreements
        assert!(
            (ri).abs() < 1e-10,
            "Maximally different should give RI=0.0, got {ri}"
        );
    }

    #[test]
    fn test_adjusted_rand_index_identical() {
        let a = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let b = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let ari = adjusted_rand_index(&a, &b);
        assert!(
            (ari - 1.0).abs() < 1e-10,
            "Identical partitions should give ARI=1.0, got {ari}"
        );
    }

    #[test]
    fn test_adjusted_rand_index_random() {
        // Two partitions that are offset: should be near 0.
        let a = partition(&[(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)]);
        let b = partition(&[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 0)]);
        let ari = adjusted_rand_index(&a, &b);
        // Not identical, not perfectly anti-correlated: should be low
        assert!(
            ari < 0.5,
            "Shifted partitions should have low ARI, got {ari}"
        );
    }

    #[test]
    fn test_nmi_identical() {
        let a = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let b = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let nmi = normalized_mutual_information(&a, &b);
        assert!(
            (nmi - 1.0).abs() < 1e-10,
            "Identical partitions should give NMI=1.0, got {nmi}"
        );
    }

    #[test]
    fn test_nmi_all_one_cluster() {
        let a = partition(&[(0, 0), (1, 0), (2, 0)]);
        let b = partition(&[(0, 0), (1, 0), (2, 0)]);
        let nmi = normalized_mutual_information(&a, &b);
        assert!(
            (nmi - 1.0).abs() < 1e-10,
            "Identical single-cluster partitions should give NMI=1.0, got {nmi}"
        );
    }

    #[test]
    fn test_pairwise_precision_perfect() {
        let predicted = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let truth = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let prec = pairwise_precision(&predicted, &truth);
        assert!(
            (prec - 1.0).abs() < 1e-10,
            "Perfect prediction should give precision=1.0, got {prec}"
        );
    }

    #[test]
    fn test_pairwise_recall_perfect() {
        let predicted = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let truth = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let rec = pairwise_recall(&predicted, &truth);
        assert!(
            (rec - 1.0).abs() < 1e-10,
            "Perfect prediction should give recall=1.0, got {rec}"
        );
    }

    #[test]
    fn test_pairwise_precision_refinement() {
        // Predicted is a refinement of truth (splits clusters further).
        // All predicted co-cluster pairs are true co-cluster pairs, so precision = 1.0.
        let predicted = partition(&[(0, 0), (1, 1), (2, 2), (3, 3)]);
        let truth = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let prec = pairwise_precision(&predicted, &truth);
        // No predicted co-clusters at all (each node alone), so precision = 1.0 (vacuous).
        assert!(
            (prec - 1.0).abs() < 1e-10,
            "Singleton prediction should give precision=1.0, got {prec}"
        );
    }

    #[test]
    fn test_pairwise_recall_refinement() {
        // Predicted is a refinement: recall should be 0 (no true pairs recovered).
        let predicted = partition(&[(0, 0), (1, 1), (2, 2), (3, 3)]);
        let truth = partition(&[(0, 0), (1, 0), (2, 1), (3, 1)]);
        let rec = pairwise_recall(&predicted, &truth);
        assert!(
            rec.abs() < 1e-10,
            "Singleton prediction should give recall=0.0, got {rec}"
        );
    }

    #[test]
    fn test_metrics_different_block_counts() {
        // A has 2 blocks, B has 3 blocks.
        let a = partition(&[(0, 0), (1, 0), (2, 0), (3, 1), (4, 1), (5, 1)]);
        let b = partition(&[(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)]);

        let ri = rand_index(&a, &b);
        let ari = adjusted_rand_index(&a, &b);
        let nmi = normalized_mutual_information(&a, &b);

        // Should all be valid (between bounds).
        assert!((0.0..=1.0).contains(&ri), "RI out of bounds: {ri}");
        assert!((-1.0..=1.0).contains(&ari), "ARI out of bounds: {ari}");
        assert!((0.0..=1.0).contains(&nmi), "NMI out of bounds: {nmi}");
    }

    #[test]
    fn test_empty_partition() {
        let a: FxHashMap<NodeId, usize> = FxHashMap::default();
        let b: FxHashMap<NodeId, usize> = FxHashMap::default();

        assert!((rand_index(&a, &b) - 1.0).abs() < 1e-10);
        assert!((adjusted_rand_index(&a, &b) - 1.0).abs() < 1e-10);
        assert!((normalized_mutual_information(&a, &b) - 1.0).abs() < 1e-10);
        assert!((pairwise_precision(&a, &b) - 1.0).abs() < 1e-10);
        assert!((pairwise_recall(&a, &b) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_single_node() {
        let a = partition(&[(0, 0)]);
        let b = partition(&[(0, 0)]);

        assert!((rand_index(&a, &b) - 1.0).abs() < 1e-10);
        assert!((adjusted_rand_index(&a, &b) - 1.0).abs() < 1e-10);
    }
}
