//! AGENTS.md Phase 14 acceptance, on the three-module fixture
//! (`tests/fixtures/tasks/`): coupled files share a node, hub changes come
//! first, and plans are deterministic.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use graft_sched::graph::build;
use graft_sched::plan::{plan, seeds};

fn fixture() -> BTreeMap<PathBuf, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tasks/three-modules.sh");
    let ok = std::process::Command::new("sh")
        .arg(script)
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(ok.success());
    let mut tree = BTreeMap::new();
    let mut stack = vec![dir.path().to_owned()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                tree.insert(
                    p.strip_prefix(dir.path()).unwrap().to_owned(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    tree
}

fn task() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tasks/three-modules.md"),
    )
    .unwrap()
}

#[test]
fn coupled_files_share_a_node_and_the_hub_goes_first() {
    let graph = build(&fixture());
    let seeds = seeds(&task(), &graph);
    let p = plan(&graph, &seeds);
    let shown: Vec<(String, Vec<String>, Vec<usize>)> = p
        .nodes
        .iter()
        .map(|n| {
            (
                n.name.clone(),
                n.claims.iter().map(ToString::to_string).collect(),
                n.deps.clone(),
            )
        })
        .collect();
    let s = |x: &[&str]| x.iter().map(|y| (*y).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        shown,
        [
            ("contract".to_owned(), s(&["app/types.py"]), vec![]),
            (
                "billing".to_owned(),
                s(&["billing/invoice.py", "billing/tax.py"]),
                vec![0]
            ),
            (
                "search".to_owned(),
                s(&["search/index.py", "search/query.py"]),
                vec![0]
            ),
            (
                "users".to_owned(),
                s(&["users/auth.py", "users/models.py"]),
                vec![0]
            ),
        ],
        "{}",
        p.render()
    );
    assert!(p.nodes[0].contract);
    assert_eq!(p.width, 3);
}

#[test]
fn plans_are_deterministic() {
    let tree = fixture();
    let first = plan(&build(&tree), &seeds(&task(), &build(&tree)));
    for _ in 0..5 {
        let graph = build(&tree);
        assert_eq!(plan(&graph, &seeds(&task(), &graph)), first);
    }
    // Whatever the order of the task's sentences.
    let text = task();
    let reversed = text.lines().rev().collect::<Vec<_>>().join("\n");
    let graph = build(&tree);
    assert_eq!(plan(&graph, &seeds(&reversed, &graph)), first);
}
