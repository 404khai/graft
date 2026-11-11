//! Community detection: Louvain modularity optimisation in a fixed node
//! order, then Leiden's connectivity refinement (ADR 0018).
//!
//! Deterministic: nodes are visited in index order, ties go to the lowest
//! community, and communities are renumbered by their first node.

use std::collections::BTreeMap;

/// An undirected weighted graph as adjacency lists (`adj[i]`: `(j, w)`).
type Adj = Vec<Vec<(usize, f64)>>;

fn adjacency(n: usize, edges: &[(usize, usize, f64)]) -> Adj {
    let mut m: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for &(a, b, w) in edges {
        if a == b {
            *m[a].entry(a).or_default() += 2.0 * w;
        } else {
            *m[a].entry(b).or_default() += w;
            *m[b].entry(a).or_default() += w;
        }
    }
    m.into_iter().map(|row| row.into_iter().collect()).collect()
}

/// One level of Louvain local moving. Returns each node's community.
fn local_moving(adj: &Adj) -> Vec<usize> {
    let n = adj.len();
    let degree: Vec<f64> = adj.iter().map(|r| r.iter().map(|(_, w)| w).sum()).collect();
    let m2: f64 = degree.iter().sum();
    let mut comm: Vec<usize> = (0..n).collect();
    if m2 == 0.0 {
        return comm;
    }
    let mut tot: Vec<f64> = degree.clone();
    loop {
        let mut moved = false;
        for i in 0..n {
            let ci = comm[i];
            let mut links: BTreeMap<usize, f64> = BTreeMap::new();
            for &(j, w) in &adj[i] {
                if j != i {
                    *links.entry(comm[j]).or_default() += w;
                }
            }
            tot[ci] -= degree[i];
            let gain = |c: usize, k_in: f64| k_in - tot[c] * degree[i] / m2;
            let mut best = ci;
            let mut best_gain = gain(ci, links.get(&ci).copied().unwrap_or(0.0));
            for (&c, &k_in) in &links {
                let g = gain(c, k_in);
                if g > best_gain + 1e-12 || ((g - best_gain).abs() <= 1e-12 && c < best) {
                    best = c;
                    best_gain = g;
                }
            }
            tot[best] += degree[i];
            if best != ci {
                comm[i] = best;
                moved = true;
            }
        }
        if !moved {
            return comm;
        }
    }
}

/// Renumbers communities 0.. by first appearance.
fn renumber(comm: &[usize]) -> Vec<usize> {
    let mut map = BTreeMap::new();
    comm.iter()
        .map(|c| {
            let next = map.len();
            *map.entry(*c).or_insert(next)
        })
        .collect()
}

/// Louvain over `n` nodes and undirected `edges`: each node's community.
#[must_use]
pub fn louvain(n: usize, edges: &[(usize, usize, f64)]) -> Vec<usize> {
    let mut assignment: Vec<usize> = (0..n).collect();
    let mut adj = adjacency(n, edges);
    loop {
        let level = renumber(&local_moving(&adj));
        let count = level.iter().max().map_or(0, |m| m + 1);
        for a in &mut assignment {
            *a = level[*a];
        }
        if count == adj.len() {
            return assignment;
        }
        // Aggregate: one node per community.
        let mut agg = Vec::new();
        for (i, row) in adj.iter().enumerate() {
            for &(j, w) in row {
                if i <= j {
                    let w = if i == j { w / 2.0 } else { w };
                    agg.push((level[i], level[j], w));
                }
            }
        }
        adj = adjacency(count, &agg);
    }
}

/// Splits every community whose members are not connected through edges
/// among themselves into its connected parts (Leiden's guarantee).
#[must_use]
pub fn refine(n: usize, edges: &[(usize, usize, f64)], comm: &[usize]) -> Vec<usize> {
    let adj = adjacency(n, edges);
    let mut out = vec![usize::MAX; n];
    let mut next = 0;
    for start in 0..n {
        if out[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        out[start] = next;
        while let Some(i) = stack.pop() {
            for &(j, _) in &adj[i] {
                if out[j] == usize::MAX && comm[j] == comm[start] {
                    out[j] = next;
                    stack.push(j);
                }
            }
        }
        next += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_two_cliques_joined_by_a_bridge() {
        // 0-1-2 tightly coupled, 3-4-5 tightly coupled, one light bridge.
        let mut e = Vec::new();
        for (a, b) in [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5)] {
            e.push((a, b, 5.0));
        }
        e.push((2, 3, 1.0));
        let c = refine(6, &e, &louvain(6, &e));
        assert_eq!(c[0], c[1]);
        assert_eq!(c[1], c[2]);
        assert_eq!(c[3], c[4]);
        assert_eq!(c[4], c[5]);
        assert_ne!(c[0], c[3]);
        assert_eq!(c, refine(6, &e, &louvain(6, &e)), "deterministic");
    }

    #[test]
    fn isolated_nodes_and_disconnected_communities_stay_apart() {
        let c = refine(3, &[], &[0, 0, 0]);
        assert_eq!(c, [0, 1, 2]);
        assert_eq!(louvain(2, &[]), [0, 1]);
    }
}
