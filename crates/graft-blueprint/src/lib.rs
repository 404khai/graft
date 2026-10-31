//! Blueprints: the declarative recipe for a graft's environment.
//!
//! A Blueprint lives in `graft.toml` at the top of the repository. This
//! crate parses it. For now that means the `[[layer]]` entries used by the
//! layer cache; ports, services, env files and templates arrive with the full
//! Blueprint phase.
//!
//! ```toml
//! [[layer]]
//! path = "node_modules"                         # directory the build produces
//! inputs = ["package.json", "package-lock.json"] # files whose contents key it
//! build = "npm ci"                              # run in the graft's top level
//! toolchain = "node --version"                  # optional: output is part of the key
//! ```
//!
//! Every error carries the file, line and column it refers to.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

/// The Blueprint's file name, relative to the repository's top level.
pub const BLUEPRINT_FILE: &str = "graft.toml";

/// A `graft.toml` problem, located in the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct BlueprintError {
    /// The file, as given to the parser.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.column, self.message
        )
    }
}

/// One `[[layer]]`: a directory tree built once per input hash and shared
/// by every graft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSpec {
    /// Where the layer lives in a graft, relative to its top level.
    pub path: PathBuf,
    /// Files whose contents determine the layer, relative to the top level.
    pub inputs: Vec<PathBuf>,
    /// Shell command that produces `path`, run in the graft's top level.
    pub build: String,
    /// Optional shell command whose output is part of the key, such as
    /// `node --version`.
    pub toolchain: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    layer: Vec<RawLayer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayer {
    path: Spanned<String>,
    inputs: Spanned<Vec<Spanned<String>>>,
    build: Spanned<String>,
    toolchain: Option<Spanned<String>>,
}

struct Locator<'a> {
    file: &'a str,
    text: &'a str,
}

impl Locator<'_> {
    fn error(&self, offset: usize, message: impl Into<String>) -> BlueprintError {
        let before = &self.text[..offset.min(self.text.len())];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        BlueprintError {
            file: self.file.to_owned(),
            line,
            column,
            message: message.into(),
        }
    }
}

/// A relative path made only of normal components, none of them `.git`.
fn safe_relative(raw: &str) -> Result<PathBuf, &'static str> {
    if raw.is_empty() {
        return Err("must not be empty");
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err("must be relative to the repository's top level");
    }
    // `Path::components` silently drops inner `.` and empty segments, so
    // check the text itself first.
    for segment in raw.trim_end_matches('/').split('/') {
        match segment {
            "." | ".." => return Err("must not contain . or .. components"),
            "" => return Err("must not contain empty components"),
            _ => {}
        }
    }
    for component in path.components() {
        match component {
            Component::Normal(c) if c.eq_ignore_ascii_case(".git") => {
                return Err("must not be inside .git");
            }
            Component::Normal(_) => {}
            Component::CurDir | Component::ParentDir => {
                return Err("must not contain . or .. components");
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("must be relative to the repository's top level");
            }
        }
    }
    Ok(path.components().collect())
}

/// Parses the `[[layer]]` entries of a `graft.toml`.
///
/// `file` names the file in error messages.
///
/// # Errors
///
/// A [`BlueprintError`] for invalid TOML, unknown keys, unsafe paths, empty
/// commands, duplicate inputs, or layers that overlap.
pub fn parse_layers(text: &str, file: &str) -> Result<Vec<LayerSpec>, BlueprintError> {
    let at = Locator { file, text };
    let raw: RawFile = toml::from_str(text).map_err(|e| {
        let offset = e.span().map_or(0, |s| s.start);
        at.error(offset, e.message().trim().to_owned())
    })?;

    let mut layers: Vec<(LayerSpec, usize)> = Vec::new();
    for layer in raw.layer {
        let path = safe_relative(layer.path.get_ref())
            .map_err(|why| at.error(layer.path.span().start, format!("layer path {why}")))?;

        if layer.inputs.get_ref().is_empty() {
            return Err(at.error(
                layer.inputs.span().start,
                "layer inputs must list at least one file",
            ));
        }
        let mut inputs: Vec<PathBuf> = Vec::new();
        for input in layer.inputs.get_ref() {
            let p = safe_relative(input.get_ref())
                .map_err(|why| at.error(input.span().start, format!("layer input {why}")))?;
            if p.starts_with(&path) {
                return Err(at.error(
                    input.span().start,
                    "a layer input must not be inside the layer it builds",
                ));
            }
            if inputs.contains(&p) {
                return Err(at.error(input.span().start, "duplicate layer input"));
            }
            inputs.push(p);
        }

        let command = |s: &Spanned<String>, what: &str| {
            let trimmed = s.get_ref().trim();
            if trimmed.is_empty() {
                Err(at.error(
                    s.span().start,
                    format!("layer {what} command must not be empty"),
                ))
            } else {
                Ok(trimmed.to_owned())
            }
        };
        let build = command(&layer.build, "build")?;
        let toolchain = layer
            .toolchain
            .as_ref()
            .map(|t| command(t, "toolchain"))
            .transpose()?;

        for (other, _) in &layers {
            if other.path.starts_with(&path) || path.starts_with(&other.path) {
                return Err(at.error(
                    layer.path.span().start,
                    format!(
                        "layer {} overlaps layer {}",
                        path.display(),
                        other.path.display()
                    ),
                ));
            }
        }
        layers.push((
            LayerSpec {
                path,
                inputs,
                build,
                toolchain,
            },
            layer.path.span().start,
        ));
    }
    Ok(layers.into_iter().map(|(l, _)| l).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> BlueprintError {
        parse_layers(text, "graft.toml").unwrap_err()
    }

    #[test]
    fn parses_layers() {
        let layers = parse_layers(
            r#"
[[layer]]
path = "node_modules"
inputs = ["package.json", "package-lock.json"]
build = "npm ci"
toolchain = "node --version"

[[layer]]
path = "target"
inputs = ["Cargo.lock"]
build = "  cargo build  "
"#,
            "graft.toml",
        )
        .unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].path, Path::new("node_modules"));
        assert_eq!(
            layers[0].inputs,
            [Path::new("package.json"), Path::new("package-lock.json")]
        );
        assert_eq!(layers[0].toolchain.as_deref(), Some("node --version"));
        assert_eq!(layers[1].build, "cargo build");
        assert_eq!(layers[1].toolchain, None);
    }

    #[test]
    fn an_empty_file_has_no_layers() {
        assert_eq!(parse_layers("", "graft.toml").unwrap(), Vec::new());
    }

    #[test]
    fn syntax_errors_have_file_line_and_column() {
        let e = err("[[layer]]\npath = \"x\"\ninputs = [\"a\"\n");
        assert_eq!((e.file.as_str(), e.line), ("graft.toml", 3), "{e}");
        assert!(e.to_string().starts_with("graft.toml:3:"), "{e}");
    }

    #[test]
    fn unknown_keys_are_rejected_where_they_are() {
        let e = err("[[layer]]\npath = \"x\"\ninputs = [\"a\"]\nbuild = \"b\"\nbiuld = \"c\"\n");
        assert_eq!(e.line, 5, "{e}");
        assert!(e.message.contains("biuld"), "{e}");
        let e = err("[blueprint.web]\n");
        assert!(e.message.contains("blueprint"), "{e}");
    }

    #[test]
    fn missing_fields_are_reported() {
        let e = err("[[layer]]\npath = \"x\"\nbuild = \"b\"\n");
        assert!(e.message.contains("inputs"), "{e}");
    }

    #[test]
    fn unsafe_paths_are_rejected_at_their_position() {
        for (bad, why) in [
            ("/abs", "relative"),
            ("../up", ". or .."),
            ("a/./b", ". or .."),
            ("a//b", "empty components"),
            (".git/x", ".git"),
            (".GIT", ".git"),
            ("", "empty"),
        ] {
            let e = err(&format!(
                "[[layer]]\npath = \"{bad}\"\ninputs = [\"a\"]\nbuild = \"b\"\n"
            ));
            assert_eq!((e.line, e.column), (2, 8), "{bad}: {e}");
            assert!(e.message.contains(why), "{bad}: {e}");
            let e = err(&format!(
                "[[layer]]\npath = \"x\"\ninputs = [\"ok\", \"{bad}\"]\nbuild = \"b\"\n"
            ));
            assert_eq!(e.line, 3, "{bad}: {e}");
            assert!(e.message.starts_with("layer input"), "{bad}: {e}");
        }
    }

    #[test]
    fn semantic_errors() {
        let base = |inputs: &str, build: &str| {
            format!("[[layer]]\npath = \"node_modules\"\ninputs = {inputs}\nbuild = \"{build}\"\n")
        };
        assert!(err(&base("[]", "b")).message.contains("at least one"));
        assert!(
            err(&base("[\"a\", \"a\"]", "b"))
                .message
                .contains("duplicate")
        );
        assert!(
            err(&base("[\"node_modules/x\"]", "b"))
                .message
                .contains("inside the layer")
        );
        let e = err(&base("[\"a\"]", "   "));
        assert_eq!(e.line, 4, "{e}");
        assert!(e.message.contains("build command must not be empty"), "{e}");
    }

    #[test]
    fn overlapping_layers_are_rejected() {
        let e = err(
            "[[layer]]\npath = \"a\"\ninputs = [\"x\"]\nbuild = \"b\"\n\n[[layer]]\npath = \"a/b\"\ninputs = [\"x\"]\nbuild = \"b\"\n",
        );
        assert_eq!(e.line, 7, "{e}");
        assert!(e.message.contains("overlaps"), "{e}");
    }
}
