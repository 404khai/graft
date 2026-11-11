//! Plans: a task's seeds, partitioned into an ordered DAG of nodes
//! (ADR 0018).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::community::{louvain, refine};
use crate::graph::Graph;

/// Something a task names: a file, or one symbol in it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct Seed {
    /// The file, relative to the top level.
    pub path: PathBuf,
    /// The symbol (qualified as Radar names it), or `None` for the whole
    /// file.
    pub symbol: Option<String>,
}

impl std::fmt::Display for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.symbol {
            Some(s) => write!(f, "{}::{s}", self.path.display()),
            None => write!(f, "{}", self.path.display()),
        }
    }
}

/// The seeds a task text names: code spans that are files, `file::symbol`
/// or a symbol defined in exactly one file, and bare words that are files.
#[must_use]
pub fn seeds(task: &str, graph: &Graph) -> Vec<Seed> {
    let mut by_name: BTreeMap<&str, BTreeSet<(usize, &str)>> = BTreeMap::new();
    for (i, syms) in graph.symbols.iter().enumerate() {
        for s in syms {
            by_name
                .entry(s.as_str())
                .or_default()
                .insert((i, s.as_str()));
            if let Some(last) = s.rsplit([':', '.']).next()
                && last != s
            {
                by_name.entry(last).or_default().insert((i, s.as_str()));
            }
        }
    }
    let file = |w: &str| {
        let w = w.trim_start_matches("./");
        graph.index(Path::new(w)).map(|i| graph.files[i].clone())
    };
    let mut out = BTreeSet::new();
    let spans: Vec<&str> = task.split('`').skip(1).step_by(2).collect();
    for span in spans {
        let span = span.trim();
        if let Some(f) = file(span) {
            out.insert(Seed {
                path: f,
                symbol: None,
            });
        } else if let Some((p, sym)) = span.split_once("::")
            && let Some(f) = file(p)
            && let Some(i) = graph.index(&f)
            && graph.symbols[i].contains(sym)
        {
            out.insert(Seed {
                path: f,
                symbol: Some(sym.to_owned()),
            });
        } else if let Some(defs) = by_name.get(span)
            && defs.len() == 1
            && let Some((i, full)) = defs.iter().next()
        {
            out.insert(Seed {
                path: graph.files[*i].clone(),
                symbol: Some((*full).to_owned()),
            });
        }
    }
    for word in task.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '`'))
    {
        let word = word.trim_end_matches(['.', ':']);
        if word.contains('/')
            && let Some(f) = file(word)
        {
            out.insert(Seed {
                path: f,
                symbol: None,
            });
        }
    }
    // A whole-file seed covers its symbol seeds.
    let whole: BTreeSet<PathBuf> = out
        .iter()
        .filter(|s| s.symbol.is_none())
        .map(|s| s.path.clone())
        .collect();
    out.into_iter()
        .filter(|s| s.symbol.is_none() || !whole.contains(&s.path))
        .collect()
}

/// Files most of the graph depends on: imported by at least 3 files and by
/// at least a fifth of the files with any edge.
#[must_use]
pub fn hubs(graph: &Graph) -> BTreeSet<usize> {
    let connected = graph.connected_files();
    (0..graph.files.len())
        .filter(|&i| {
            let n = graph.importers(i);
            n >= 3 && n * 5 >= connected
        })
        .collect()
}

/// One node of a plan: work for one graft.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanNode {
    /// Position in the plan (topological order).
    pub index: usize,
    /// Suggested graft name.
    pub name: String,
    /// Whether this is the contract node (hub changes).
    pub contract: bool,
    /// What the node's graft claims.
    pub claims: Vec<Seed>,
    /// Indices of the nodes that must land first.
    pub deps: Vec<usize>,
    /// Depth in the DAG (0: no dependencies).
    pub level: usize,
}

/// A task's plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Plan {
    /// Nodes in order.
    pub nodes: Vec<PlanNode>,
    /// The size of the largest level: how many grafts can work at once.
    pub width: usize,
}

/// A graft name from a node's files: the last component of their common
/// directory, or the first file's stem.
fn node_name(paths: &[&Path]) -> String {
    let mut common: Option<PathBuf> = None;
    for p in paths {
        let dir = p.parent().map(Path::to_owned).unwrap_or_default();
        common = Some(match common {
            None => dir,
            Some(c) => c
                .components()
                .zip(dir.components())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    let raw = common
        .and_then(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| !n.is_empty() && n != "src")
        .or_else(|| {
            paths
                .first()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "work".to_owned());
    let name: String = raw
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut name = name.trim_matches('-').to_owned();
    name.truncate(36);
    if name.is_empty() {
        "work".to_owned()
    } else {
        name
    }
}

/// Plans `seeds` over `graph`.
#[must_use]
pub fn plan(graph: &Graph, seeds: &[Seed]) -> Plan {
    let undirected: Vec<(usize, usize, f64)> =
        graph.edges.iter().map(|(&(a, b), &w)| (a, b, w)).collect();
    let n = graph.files.len();
    let comm = refine(n, &undirected, &louvain(n, &undirected));
    let hub = hubs(graph);

    // Group seeds: contract first, then by community (in order of the
    // community's first seed path).
    let mut groups: BTreeMap<(bool, usize), Vec<Seed>> = BTreeMap::new();
    for s in seeds {
        let Some(i) = graph.index(&s.path) else {
            continue;
        };
        let key = if hub.contains(&i) {
            (false, 0)
        } else {
            (true, comm[i])
        };
        groups.entry(key).or_default().push(s.clone());
    }
    let mut raw: Vec<(bool, Vec<Seed>)> = groups
        .into_iter()
        .map(|((other, _), v)| (!other, v))
        .collect();
    raw.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1[0].cmp(&b.1[0])));

    // Dependencies between groups: an import edge from one's files to
    // another's, and every group on the contract.
    let files_of =
        |g: &[Seed]| -> BTreeSet<usize> { g.iter().filter_map(|s| graph.index(&s.path)).collect() };
    let sets: Vec<BTreeSet<usize>> = raw.iter().map(|(_, g)| files_of(g)).collect();
    let mut deps: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); raw.len()];
    for a in 0..raw.len() {
        for b in 0..raw.len() {
            if a == b {
                continue;
            }
            let imports = sets[a]
                .iter()
                .any(|&i| sets[b].iter().any(|&j| graph.edges.contains_key(&(i, j))));
            if imports || (raw[b].0 && !raw[a].0) {
                deps[a].insert(b);
            }
        }
    }
    // Topological order; a cycle is broken at its lowest group.
    let mut order = Vec::new();
    let mut placed = vec![false; raw.len()];
    while order.len() < raw.len() {
        let ready = (0..raw.len()).find(|&g| !placed[g] && deps[g].iter().all(|d| placed[*d]));
        let g = ready.unwrap_or_else(|| (0..raw.len()).find(|&g| !placed[g]).unwrap_or(0));
        if ready.is_none() {
            let unplaced: Vec<usize> = deps[g].iter().copied().filter(|d| !placed[*d]).collect();
            for d in unplaced {
                deps[g].remove(&d);
            }
        }
        placed[g] = true;
        order.push(g);
    }
    let position: BTreeMap<usize, usize> = order.iter().enumerate().map(|(p, g)| (*g, p)).collect();
    let mut nodes: Vec<PlanNode> = Vec::new();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for (p, &g) in order.iter().enumerate() {
        let (contract, claims) = &raw[g];
        let base = if *contract {
            "contract".to_owned()
        } else {
            node_name(&claims.iter().map(|s| s.path.as_path()).collect::<Vec<_>>())
        };
        let count = used.entry(base.clone()).or_default();
        *count += 1;
        let name = if *count == 1 {
            base
        } else {
            format!("{base}-{count}")
        };
        let mut d: Vec<usize> = deps[g].iter().map(|x| position[x]).collect();
        d.sort_unstable();
        let level = d.iter().map(|x| nodes[*x].level + 1).max().unwrap_or(0);
        nodes.push(PlanNode {
            index: p,
            name,
            contract: *contract,
            claims: claims.clone(),
            deps: d,
            level,
        });
    }
    let mut per_level: BTreeMap<usize, usize> = BTreeMap::new();
    for n in &nodes {
        *per_level.entry(n.level).or_default() += 1;
    }
    Plan {
        width: per_level.values().copied().max().unwrap_or(0),
        nodes,
    }
}

impl Plan {
    /// The plan as text for people.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = String::new();
        if self.nodes.is_empty() {
            s.push_str("The task names no files or symbols of this repository.\n");
            return s;
        }
        let _ = writeln!(s, "{} node(s), width {}:", self.nodes.len(), self.width);
        for n in &self.nodes {
            let after = if n.deps.is_empty() {
                String::new()
            } else {
                let names: Vec<&str> = n
                    .deps
                    .iter()
                    .map(|d| self.nodes[*d].name.as_str())
                    .collect();
                format!(" after {}", names.join(", "))
            };
            let tag = if n.contract { " (contract)" } else { "" };
            let _ = writeln!(s, "{}. {}{tag}{after}", n.index + 1, n.name);
            for c in &n.claims {
                let _ = writeln!(s, "     {c}");
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::graph::build;

    fn tree(files: &[(&str, &str)]) -> BTreeMap<PathBuf, Vec<u8>> {
        files
            .iter()
            .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn names_are_derived_from_common_directories() {
        assert_eq!(
            node_name(&[Path::new("app/billing/a.py"), Path::new("app/billing/b.py")]),
            "billing"
        );
        assert_eq!(node_name(&[Path::new("src/lib.rs")]), "lib");
        assert_eq!(node_name(&[Path::new("My Dir/x.py")]), "my-dir");
    }

    #[test]
    fn seeds_come_from_code_spans_and_paths() {
        let g = build(&tree(&[
            (
                "a/x.py",
                "def total():\n    pass\n\n\ndef other():\n    pass\n",
            ),
            ("b/y.py", "def other():\n    pass\n"),
        ]));
        let found = seeds(
            "Change `total`, `a/x.py::other`, `other` (ambiguous), see b/y.py. And `nope`.",
            &g,
        );
        let shown: Vec<String> = found.iter().map(ToString::to_string).collect();
        assert_eq!(shown, ["a/x.py::other", "a/x.py::total", "b/y.py"]);
    }
}
