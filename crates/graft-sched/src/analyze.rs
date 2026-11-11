//! What one source file imports and which names it uses, with tree-sitter.

use std::collections::BTreeMap;

use graft_radar::symbols::Lang;
use tree_sitter::{Node, Parser};

/// An import, as written; [`crate::graph`] resolves it to files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Import {
    /// Rust `mod name;` (a child module in its own file).
    RustMod(String),
    /// Rust `use` path starting with `crate`, `super` or `self`.
    RustPath(Vec<String>),
    /// Python import: leading dots (0 for absolute), the module path, and
    /// the names imported from it (`from m import a, b`).
    Python {
        /// Number of leading dots.
        level: usize,
        /// The dotted module path, split.
        module: Vec<String>,
        /// Names imported from it.
        names: Vec<String>,
    },
    /// TypeScript module specifier (`"./x"`).
    Ts(String),
    /// Go import path.
    Go(String),
}

/// What [`analyze`] found in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFacts {
    /// Imports, in source order.
    pub imports: Vec<Import>,
    /// Identifiers used, with how often.
    pub names: BTreeMap<String, usize>,
}

fn text<'a>(n: Node<'_>, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or_default()
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_owned()
}

/// The path of a Rust `use` argument up to any `{`, `*` or `as`.
fn rust_use_path(arg: &str) -> Vec<String> {
    let before = arg.split('{').next().unwrap_or(arg);
    let before = before.split(" as ").next().unwrap_or(before);
    before
        .split("::")
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "*")
        .map(str::to_owned)
        .collect()
}

fn python_import_from(n: Node<'_>, src: &[u8]) -> Option<Import> {
    let module = n.child_by_field_name("module_name")?;
    let raw = text(module, src);
    let level = raw.chars().take_while(|c| *c == '.').count();
    let path: Vec<String> = raw[level..]
        .split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let mut names = Vec::new();
    let mut cursor = n.walk();
    for c in n.children_by_field_name("name", &mut cursor) {
        let name = match c.kind() {
            "aliased_import" => c.child_by_field_name("name").map(|x| text(x, src)),
            _ => Some(text(c, src)),
        };
        if let Some(name) = name {
            names.push(name.to_owned());
        }
    }
    Some(Import::Python {
        level,
        module: path,
        names,
    })
}

fn visit(lang: Lang, n: Node<'_>, src: &[u8], out: &mut FileFacts) {
    let kind = n.kind();
    match (lang, kind) {
        (Lang::Rust, "mod_item") if n.child_by_field_name("body").is_none() => {
            if let Some(name) = n.child_by_field_name("name") {
                out.imports
                    .push(Import::RustMod(text(name, src).to_owned()));
            }
        }
        (Lang::Rust, "use_declaration") => {
            if let Some(arg) = n.child_by_field_name("argument") {
                let path = rust_use_path(text(arg, src));
                if path
                    .first()
                    .is_some_and(|s| matches!(s.as_str(), "crate" | "super" | "self"))
                {
                    out.imports.push(Import::RustPath(path));
                }
            }
        }
        (Lang::Python, "import_statement") => {
            let mut cursor = n.walk();
            for c in n.children_by_field_name("name", &mut cursor) {
                let dotted = match c.kind() {
                    "aliased_import" => c.child_by_field_name("name").map(|x| text(x, src)),
                    _ => Some(text(c, src)),
                };
                if let Some(d) = dotted {
                    out.imports.push(Import::Python {
                        level: 0,
                        module: d.split('.').map(str::to_owned).collect(),
                        names: Vec::new(),
                    });
                }
            }
        }
        (Lang::Python, "import_from_statement") => {
            if let Some(i) = python_import_from(n, src) {
                out.imports.push(i);
            }
        }
        (Lang::TypeScript | Lang::Tsx, "import_statement" | "export_statement") => {
            if let Some(s) = n.child_by_field_name("source") {
                out.imports.push(Import::Ts(unquote(text(s, src))));
            }
        }
        (Lang::Go, "import_spec") => {
            if let Some(p) = n.child_by_field_name("path") {
                out.imports.push(Import::Go(unquote(text(p, src))));
            }
        }
        (
            _,
            "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_property_identifier",
        ) => {
            *out.names.entry(text(n, src).to_owned()).or_default() += 1;
        }
        _ => {}
    }
    let mut cursor = n.walk();
    for c in n.named_children(&mut cursor) {
        visit(lang, c, src, out);
    }
}

/// The imports and identifiers of `src`. `None` if it does not parse.
#[must_use]
pub fn analyze(lang: Lang, src: &[u8]) -> Option<FileFacts> {
    let mut parser = Parser::new();
    parser.set_language(&lang.grammar()).ok()?;
    let tree = parser.parse(src, None)?;
    let mut out = FileFacts::default();
    visit(lang, tree.root_node(), src, &mut out);
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn finds_imports_per_language() {
        let rs = analyze(
            Lang::Rust,
            b"mod a;\nmod b { }\nuse crate::x::{y, z};\nuse super::w as v;\nuse std::io;\nfn f() { y(); }\n",
        )
        .unwrap();
        assert_eq!(
            rs.imports,
            [
                Import::RustMod("a".into()),
                Import::RustPath(vec!["crate".into(), "x".into()]),
                Import::RustPath(vec!["super".into(), "w".into()]),
            ]
        );
        assert_eq!(rs.names.get("y"), Some(&2));
        let py = analyze(
            Lang::Python,
            b"import a.b\nfrom ..c import d, e as f\nfrom . import g\n",
        )
        .unwrap();
        assert_eq!(
            py.imports,
            [
                Import::Python {
                    level: 0,
                    module: vec!["a".into(), "b".into()],
                    names: vec![]
                },
                Import::Python {
                    level: 2,
                    module: vec!["c".into()],
                    names: vec!["d".into(), "e".into()]
                },
                Import::Python {
                    level: 1,
                    module: vec![],
                    names: vec!["g".into()]
                },
            ]
        );
        let ts = analyze(
            Lang::TypeScript,
            b"import { a } from './x';\nexport * from \"../y\";\n",
        )
        .unwrap();
        assert_eq!(
            ts.imports,
            [Import::Ts("./x".into()), Import::Ts("../y".into())]
        );
        let go = analyze(
            Lang::Go,
            b"package p\nimport (\n  \"fmt\"\n  \"ex.com/m/q\"\n)\n",
        )
        .unwrap();
        assert_eq!(
            go.imports,
            [Import::Go("fmt".into()), Import::Go("ex.com/m/q".into())]
        );
    }
}
