//! The file dependency graph of a tree (ADR 0018).
//!
//! Nodes are files Radar can parse (Rust, TypeScript, Python, Go). An
//! import of another file in the tree adds weight 1 to the edge from the
//! importer; each use of a name the imported file defines adds 1 more.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use graft_radar::symbols::{Lang, definitions};

use crate::analyze::{FileFacts, Import, analyze};

/// A directed, weighted file graph. Files are sorted by path, and edges
/// are keyed by `(importer, imported)` indices into [`Graph::files`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    /// Every file, sorted.
    pub files: Vec<PathBuf>,
    /// Edge weights.
    pub edges: BTreeMap<(usize, usize), f64>,
    /// Each file's definitions (qualified names).
    pub symbols: Vec<BTreeSet<String>>,
}

impl Graph {
    /// The index of `path`, if it is in the graph.
    #[must_use]
    pub fn index(&self, path: &Path) -> Option<usize> {
        self.files.binary_search_by(|p| p.as_path().cmp(path)).ok()
    }

    /// How many distinct files import file `i`.
    #[must_use]
    pub fn importers(&self, i: usize) -> usize {
        self.edges.keys().filter(|(_, to)| *to == i).count()
    }

    /// Files with at least one edge.
    #[must_use]
    pub fn connected_files(&self) -> usize {
        let mut seen = BTreeSet::new();
        for (a, b) in self.edges.keys() {
            seen.insert(*a);
            seen.insert(*b);
        }
        seen.len()
    }
}

/// `path` with `.` and `..` folded away (never above the tree's top).
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

fn parent(p: &Path) -> PathBuf {
    p.parent().map(Path::to_owned).unwrap_or_default()
}

struct Resolver<'a> {
    files: &'a BTreeSet<PathBuf>,
    go_module: Option<String>,
}

impl Resolver<'_> {
    fn first(&self, candidates: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        candidates
            .into_iter()
            .filter_map(|c| normalize(&c))
            .find(|c| self.files.contains(c))
            .into_iter()
            .collect()
    }

    /// The directory a Rust file's child modules live in.
    fn rust_module_dir(file: &Path) -> PathBuf {
        let dir = parent(file);
        match file.file_name().and_then(|n| n.to_str()) {
            Some("mod.rs" | "lib.rs" | "main.rs") => dir,
            _ => dir.join(file.file_stem().unwrap_or_default()),
        }
    }

    /// The crate's `src/` (or the file's own directory).
    fn rust_crate_root(file: &Path) -> PathBuf {
        let mut dir = parent(file);
        loop {
            if dir.file_name().is_some_and(|n| n == "src") {
                return dir;
            }
            if !dir.pop() {
                return parent(file);
            }
        }
    }

    /// Files for module path `segs` under `dir`, longest prefix first.
    fn rust_module(&self, dir: &Path, segs: &[String]) -> Vec<PathBuf> {
        for n in (1..=segs.len()).rev() {
            let mut p = dir.to_owned();
            for s in &segs[..n] {
                p.push(s);
            }
            let found = self.first([p.with_extension("rs"), p.join("mod.rs")]);
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    fn resolve(&self, file: &Path, import: &Import) -> Vec<PathBuf> {
        match import {
            Import::RustMod(name) => {
                let dir = Self::rust_module_dir(file);
                self.first([
                    dir.join(format!("{name}.rs")),
                    dir.join(name).join("mod.rs"),
                ])
            }
            Import::RustPath(segs) => {
                let (base, rest) = match segs.first().map(String::as_str) {
                    Some("crate") => (Self::rust_crate_root(file), &segs[1..]),
                    Some("self") => (Self::rust_module_dir(file), &segs[1..]),
                    Some("super") => {
                        let mut base = parent(&Self::rust_module_dir(file));
                        let mut rest = &segs[1..];
                        while rest.first().is_some_and(|s| s == "super") {
                            base = parent(&base);
                            rest = &rest[1..];
                        }
                        (base, rest)
                    }
                    _ => return Vec::new(),
                };
                self.rust_module(&base, rest)
            }
            Import::Python {
                level,
                module,
                names,
            } => {
                let base = if *level == 0 {
                    PathBuf::new()
                } else {
                    let mut b = parent(file);
                    for _ in 1..*level {
                        b = parent(&b);
                    }
                    b
                };
                let mut m = base.clone();
                for s in module {
                    m.push(s);
                }
                let mut out = Vec::new();
                for n in names {
                    out.extend(
                        self.first([m.join(format!("{n}.py")), m.join(n).join("__init__.py")]),
                    );
                }
                if module.is_empty() && *level > 0 {
                    return out;
                }
                out.extend(self.first([m.with_extension("py"), m.join("__init__.py")]));
                if out.is_empty() && *level == 0 {
                    let src = Path::new("src").join(&m);
                    out.extend(self.first([src.with_extension("py"), src.join("__init__.py")]));
                }
                out
            }
            Import::Ts(spec) => {
                if !spec.starts_with('.') {
                    return Vec::new();
                }
                let p = parent(file).join(spec);
                let stem = spec
                    .strip_suffix(".js")
                    .map_or_else(|| p.clone(), |s| parent(file).join(s));
                self.first([
                    p.clone(),
                    stem.with_extension("ts"),
                    stem.with_extension("tsx"),
                    stem.with_extension("d.ts"),
                    p.join("index.ts"),
                    p.join("index.tsx"),
                ])
            }
            Import::Go(path) => {
                let Some(module) = &self.go_module else {
                    return Vec::new();
                };
                let Some(rest) = path.strip_prefix(module.as_str()) else {
                    return Vec::new();
                };
                let dir = PathBuf::from(rest.trim_start_matches('/'));
                self.files
                    .iter()
                    .filter(|f| {
                        parent(f) == dir
                            && f.extension().is_some_and(|e| e == "go")
                            && !f.to_string_lossy().ends_with("_test.go")
                    })
                    .cloned()
                    .collect()
            }
        }
    }
}

/// The module path declared in a `go.mod`.
fn go_module(go_mod: &[u8]) -> Option<String> {
    String::from_utf8_lossy(go_mod)
        .lines()
        .find_map(|l| l.trim().strip_prefix("module "))
        .map(|m| m.trim().trim_matches('"').to_owned())
}

/// Builds the graph of `tree` (path → contents). Other files may be in
/// `tree`; only `go.mod` at the top is read among them.
#[must_use]
pub fn build(tree: &BTreeMap<PathBuf, Vec<u8>>) -> Graph {
    let files: BTreeSet<PathBuf> = tree
        .keys()
        .filter(|p| Lang::of(p).is_some())
        .cloned()
        .collect();
    let resolver = Resolver {
        files: &files,
        go_module: tree.get(Path::new("go.mod")).and_then(|m| go_module(m)),
    };
    let list: Vec<PathBuf> = files.iter().cloned().collect();
    let index: BTreeMap<&PathBuf, usize> = list.iter().enumerate().map(|(i, p)| (p, i)).collect();
    let facts: Vec<Option<FileFacts>> = list
        .iter()
        .map(|p| Lang::of(p).and_then(|l| analyze(l, &tree[p])))
        .collect();
    let symbols: Vec<BTreeSet<String>> = list
        .iter()
        .map(|p| {
            Lang::of(p)
                .and_then(|l| definitions(l, &tree[p]))
                .map(|d| d.into_keys().collect())
                .unwrap_or_default()
        })
        .collect();
    let mut edges: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for (i, f) in facts.iter().enumerate() {
        let Some(f) = f else { continue };
        let mut targets = BTreeSet::new();
        for imp in &f.imports {
            for t in resolver.resolve(&list[i], imp) {
                if let Some(&j) = index.get(&t)
                    && j != i
                {
                    targets.insert(j);
                }
            }
        }
        for j in targets {
            // Uses of the imported file's top-level names, by their last
            // path segment.
            let uses: usize = symbols[j]
                .iter()
                .filter_map(|s| s.rsplit([':', '.']).next())
                .collect::<BTreeSet<_>>()
                .iter()
                .map(|n| f.names.get(*n).copied().unwrap_or(0))
                .sum();
            #[allow(clippy::cast_precision_loss)] // counts are small
            let w = 1.0 + uses as f64;
            edges.insert((i, j), w);
        }
    }
    Graph {
        files: list,
        edges,
        symbols,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn tree(files: &[(&str, &str)]) -> BTreeMap<PathBuf, Vec<u8>> {
        files
            .iter()
            .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
            .collect()
    }

    fn edges(g: &Graph) -> Vec<(String, String)> {
        g.edges
            .keys()
            .map(|(a, b)| {
                (
                    g.files[*a].display().to_string(),
                    g.files[*b].display().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn resolves_imports_in_every_language() {
        let g = build(&tree(&[
            ("src/lib.rs", "mod a;\nmod b;\n"),
            ("src/a.rs", "use crate::b::Thing;\nfn f(_: Thing) {}\n"),
            ("src/b/mod.rs", "pub struct Thing;\nuse super::a;\n"),
            ("py/pkg/__init__.py", ""),
            ("py/pkg/m.py", "from . import n\nfrom .n import g\n"),
            ("py/pkg/n.py", "def g():\n    pass\n"),
            (
                "web/x.ts",
                "import { y } from './y';\nexport const x = y;\n",
            ),
            ("web/y.ts", "export const y = 1;\n"),
            ("go.mod", "module ex.com/m\n"),
            (
                "cmd/main.go",
                "package main\nimport \"ex.com/m/lib\"\nfunc main() { lib.F() }\n",
            ),
            ("lib/f.go", "package lib\nfunc F() {}\n"),
            ("lib/f_test.go", "package lib\n"),
            ("README.md", "not code"),
        ]));
        assert_eq!(
            edges(&g),
            [
                ("cmd/main.go", "lib/f.go"),
                ("py/pkg/m.py", "py/pkg/n.py"),
                ("src/a.rs", "src/b/mod.rs"),
                ("src/b/mod.rs", "src/a.rs"),
                ("src/lib.rs", "src/a.rs"),
                ("src/lib.rs", "src/b/mod.rs"),
                ("web/x.ts", "web/y.ts"),
            ]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
        );
        // a.rs uses Thing twice (import and parameter): 1 + 2.
        let a = g.index(Path::new("src/a.rs")).unwrap();
        let b = g.index(Path::new("src/b/mod.rs")).unwrap();
        assert!((g.edges[&(a, b)] - 3.0).abs() < f64::EPSILON);
    }
}
