//! Mapping file changes to the symbols they touch, with tree-sitter.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tree_sitter::{Node, Parser};

/// A language Radar can name symbols in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// `.rs`
    Rust,
    /// `.ts`
    TypeScript,
    /// `.tsx`
    Tsx,
    /// `.py`
    Python,
    /// `.go`
    Go,
}

impl Lang {
    /// The language of `path`, by extension.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        Some(match path.extension()?.to_str()? {
            "rs" => Self::Rust,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "py" => Self::Python,
            "go" => Self::Go,
            _ => return None,
        })
    }

    /// The tree-sitter grammar.
    #[must_use]
    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    fn separator(self) -> &'static str {
        if self == Self::Rust { "::" } else { "." }
    }
}

/// What a node is to Radar.
enum Def {
    /// A symbol, named by its `name` field.
    Symbol,
    /// A symbol that also qualifies the symbols inside it.
    Container,
    /// Not a symbol, but qualifies the symbols inside it with this name.
    Qualifier(String),
    /// A declaration whose declarators are symbols.
    Declarators(&'static str),
    /// Anything else.
    Other,
}

fn text<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or_default()
}

fn field_text(node: Node<'_>, field: &str, src: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| text(n, src).to_owned())
}

/// The type a Go method's receiver names (`(s *Server)` → `Server`).
fn go_receiver(node: Node<'_>, src: &[u8]) -> Option<String> {
    let recv = node.child_by_field_name("receiver")?;
    let mut cursor = recv.walk();
    let param = recv.named_children(&mut cursor).next()?;
    let ty = text(param.child_by_field_name("type")?, src);
    let ty = ty.trim_start_matches('*');
    Some(ty.split('[').next().unwrap_or(ty).to_owned())
}

fn classify(lang: Lang, node: Node<'_>, src: &[u8]) -> Def {
    let kind = node.kind();
    match lang {
        Lang::Rust => match kind {
            "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "union_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "macro_definition" => Def::Symbol,
            "trait_item" | "mod_item" => Def::Container,
            "impl_item" => match field_text(node, "type", src) {
                Some(t) => Def::Qualifier(t),
                None => Def::Other,
            },
            _ => Def::Other,
        },
        Lang::TypeScript | Lang::Tsx => match kind {
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "method_signature"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration" => Def::Symbol,
            "class_declaration" | "abstract_class_declaration" | "internal_module" => {
                Def::Container
            }
            "lexical_declaration" | "variable_declaration"
                if node
                    .parent()
                    .is_some_and(|p| matches!(p.kind(), "program" | "export_statement")) =>
            {
                Def::Declarators("variable_declarator")
            }
            _ => Def::Other,
        },
        Lang::Python => match kind {
            "function_definition" => Def::Symbol,
            "class_definition" => Def::Container,
            _ => Def::Other,
        },
        Lang::Go => match kind {
            "function_declaration" => Def::Symbol,
            "method_declaration" => match go_receiver(node, src) {
                Some(r) => Def::Qualifier(r),
                None => Def::Symbol,
            },
            "type_declaration" => Def::Declarators("type_spec"),
            "const_declaration" => Def::Declarators("const_spec"),
            "var_declaration" if node.parent().is_some_and(|p| p.kind() == "source_file") => {
                Def::Declarators("var_spec")
            }
            _ => Def::Other,
        },
    }
}

/// Every definition in `src`, by qualified name, with its text. Repeated
/// names (overloads, `#[cfg]` twins) get `#2`, `#3`... suffixes.
///
/// `None` if `src` does not parse.
#[must_use]
pub fn definitions(lang: Lang, src: &[u8]) -> Option<BTreeMap<String, String>> {
    let mut parser = Parser::new();
    parser.set_language(&lang.grammar()).ok()?;
    let tree = parser.parse(src, None)?;
    if tree.root_node().has_error() {
        return None;
    }
    let mut out = BTreeMap::new();
    let mut add = |name: String, body: &str| {
        let mut key = name.clone();
        let mut n = 1;
        while out.contains_key(&key) {
            n += 1;
            key = format!("{name}#{n}");
        }
        out.insert(key, body.to_owned());
    };
    let mut stack: Vec<(Node<'_>, String)> = vec![(tree.root_node(), String::new())];
    let sep = lang.separator();
    let qualify = |prefix: &str, name: &str| {
        if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}{sep}{name}")
        }
    };
    while let Some((node, prefix)) = stack.pop() {
        let mut inner = prefix.clone();
        match classify(lang, node, src) {
            Def::Symbol => {
                if let Some(n) = field_text(node, "name", src) {
                    add(qualify(&prefix, &n), text(node, src));
                }
            }
            Def::Container => {
                if let Some(n) = field_text(node, "name", src) {
                    inner = qualify(&prefix, &n);
                    add(inner.clone(), text(node, src));
                }
            }
            Def::Qualifier(q) => {
                inner = qualify(&prefix, &q);
                if node.kind() == "method_declaration"
                    && let Some(n) = field_text(node, "name", src)
                {
                    add(qualify(&inner, &n), text(node, src));
                }
            }
            Def::Declarators(kind) => {
                let mut cursor = node.walk();
                for d in node
                    .named_children(&mut cursor)
                    .filter(|c| c.kind() == kind)
                {
                    if let Some(n) = field_text(d, "name", src) {
                        add(qualify(&prefix, &n), text(d, src));
                    }
                }
                continue;
            }
            Def::Other => {}
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        for c in children.into_iter().rev() {
            stack.push((c, inner.clone()));
        }
    }
    Some(out)
}

/// The symbols a change to `path` touched: definitions whose text differs
/// between `old` and `new`, or that exist in only one (`None` means the file
/// did not exist). A container counts as changed when anything inside it
/// changed. `None` if Radar cannot name symbols in this file (unknown
/// language, or a version that does not parse).
#[must_use]
pub fn changed_symbols(
    path: &Path,
    old: Option<&[u8]>,
    new: Option<&[u8]>,
) -> Option<BTreeSet<String>> {
    let lang = Lang::of(path)?;
    let old = definitions(lang, old.unwrap_or_default())?;
    let new = definitions(lang, new.unwrap_or_default())?;
    let mut changed = BTreeSet::new();
    for (name, body) in &old {
        if new.get(name) != Some(body) {
            changed.insert(name.clone());
        }
    }
    for name in new.keys() {
        if !old.contains_key(name) {
            changed.insert(name.clone());
        }
    }
    // Containers change whenever a member does; keep only the innermost,
    // so two grafts editing different methods of one class do not share
    // the class.
    let sep = lang.separator();
    let leaves: BTreeSet<String> = changed
        .iter()
        .filter(|n| {
            let prefix = format!("{n}{sep}");
            !changed.iter().any(|m| m.starts_with(&prefix))
        })
        .cloned()
        .collect();
    Some(leaves)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn changed(path: &str, old: &str, new: &str) -> Vec<String> {
        changed_symbols(Path::new(path), Some(old.as_bytes()), Some(new.as_bytes()))
            .unwrap()
            .into_iter()
            .collect()
    }

    #[test]
    fn rust_functions_and_methods() {
        let old = "use a::b;\nfn one() { 1 }\nfn two() { 2 }\nstruct S;\nimpl S {\n    fn m(&self) {}\n    fn n(&self) {}\n}\n";
        let new = old
            .replace("{ 2 }", "{ 3 }")
            .replace("fn n(&self) {}", "fn n(&self) { x() }");
        assert_eq!(changed("a.rs", old, &new), ["S::n", "two"]);
        assert_eq!(
            changed("a.rs", old, &old.replace("use a::b;", "use a::c;")),
            Vec::<String>::new()
        );
        let added = format!("{old}fn three() {{}}\n");
        assert_eq!(changed("a.rs", old, &added), ["three"]);
    }

    #[test]
    fn typescript_python_and_go() {
        let ts = "export function a() { return 1 }\nexport const b = () => 2\nclass C {\n  m() { return 3 }\n  n() { return 4 }\n}\n";
        assert_eq!(
            changed("x.ts", ts, &ts.replace("return 4", "return 5")),
            ["C.n"]
        );
        assert_eq!(changed("x.ts", ts, &ts.replace("=> 2", "=> 9")), ["b"]);
        let py =
            "import os\n\ndef f():\n    return 1\n\nclass K:\n    def m(self):\n        return 2\n";
        assert_eq!(
            changed("x.py", py, &py.replace("return 2", "return 3")),
            ["K.m"]
        );
        let go = "package p\n\nfunc F() int { return 1 }\n\ntype S struct{}\n\nfunc (s *S) M() int { return 2 }\n";
        assert_eq!(
            changed("x.go", go, &go.replace("return 2", "return 3")),
            ["S.M"]
        );
        assert_eq!(
            changed("x.go", go, &go.replace("return 1", "return 0")),
            ["F"]
        );
    }

    #[test]
    fn unknown_or_broken_files_have_no_symbols() {
        assert_eq!(changed_symbols(Path::new("a.md"), None, Some(b"x")), None);
        assert_eq!(
            changed_symbols(Path::new("a.rs"), Some(b"fn a() {}"), Some(b"fn a( {")),
            None
        );
        let added =
            changed_symbols(Path::new("a.py"), None, Some(b"def f():\n    pass\n")).unwrap();
        assert_eq!(added.into_iter().collect::<Vec<_>>(), ["f"]);
    }
}
