//! `{{ variable }}` templates (ADR 0015).
//!
//! A template is text with `{{ name.path }}` placeholders and nothing
//! else: no expressions, filters or loops. `{{{{` is a literal `{{`. Each
//! placeholder remembers its file, line and column, so a variable that is
//! unknown or missing when rendering is reported where it was written.

use std::collections::BTreeMap;

use crate::BlueprintError;

/// Where a template's text starts in its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The file, for error messages.
    pub file: String,
    /// 1-based line of the template's first character.
    pub line: usize,
    /// 1-based column of the template's first character.
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Var {
        name: String,
        line: usize,
        column: usize,
    },
}

/// A parsed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    file: String,
    pieces: Vec<Piece>,
}

/// A variable name: dot-separated segments of letters, digits, `_` and `-`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

impl Template {
    /// Parses `text`, which starts at `origin` in its file.
    ///
    /// # Errors
    ///
    /// An unclosed `{{`, an empty placeholder, or an invalid name.
    pub fn parse(text: &str, origin: &Origin) -> Result<Self, BlueprintError> {
        let (mut line, mut column) = (origin.line, origin.column);
        let err = |line, column, message: String| BlueprintError {
            file: origin.file.clone(),
            line,
            column,
            message,
        };
        let mut pieces = Vec::new();
        let mut text_buf = String::new();
        let mut rest = text;
        let advance = |s: &str, line: &mut usize, column: &mut usize| {
            for c in s.chars() {
                if c == '\n' {
                    *line += 1;
                    *column = 1;
                } else {
                    *column += 1;
                }
            }
        };
        while let Some(i) = rest.find("{{") {
            let (before, after) = rest.split_at(i);
            text_buf.push_str(before);
            advance(before, &mut line, &mut column);
            if let Some(escaped) = after.strip_prefix("{{{{") {
                text_buf.push_str("{{");
                column += 4;
                rest = escaped;
                continue;
            }
            let (vline, vcol) = (line, column);
            let inner_and_rest = &after[2..];
            let Some(end) = inner_and_rest.find("}}") else {
                return Err(err(vline, vcol, "unclosed {{ in template".to_owned()));
            };
            let raw = &inner_and_rest[..end];
            let name = raw.trim();
            if name.is_empty() {
                return Err(err(vline, vcol, "empty {{ }} in template".to_owned()));
            }
            if !valid_name(name) {
                return Err(err(
                    vline,
                    vcol,
                    format!("invalid template variable {name:?}"),
                ));
            }
            if !text_buf.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut text_buf)));
            }
            pieces.push(Piece::Var {
                name: name.to_owned(),
                line: vline,
                column: vcol,
            });
            advance(&after[..end + 4], &mut line, &mut column);
            rest = &inner_and_rest[end + 2..];
        }
        text_buf.push_str(rest);
        if !text_buf.is_empty() {
            pieces.push(Piece::Text(text_buf));
        }
        Ok(Self {
            file: origin.file.clone(),
            pieces,
        })
    }

    /// The variables the template uses, with their positions.
    pub fn variables(&self) -> impl Iterator<Item = (&str, usize, usize)> {
        self.pieces.iter().filter_map(|p| match p {
            Piece::Var { name, line, column } => Some((name.as_str(), *line, *column)),
            Piece::Text(_) => None,
        })
    }

    /// An error located at variable `name`'s position.
    #[must_use]
    pub fn error_at(&self, line: usize, column: usize, message: String) -> BlueprintError {
        BlueprintError {
            file: self.file.clone(),
            line,
            column,
            message,
        }
    }

    /// Renders the template with `vars`.
    ///
    /// # Errors
    ///
    /// A variable `vars` lacks, located where the template uses it.
    pub fn render(&self, vars: &BTreeMap<String, String>) -> Result<String, BlueprintError> {
        let mut out = String::new();
        for p in &self.pieces {
            match p {
                Piece::Text(t) => out.push_str(t),
                Piece::Var { name, line, column } => match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        return Err(self.error_at(
                            *line,
                            *column,
                            format!("unknown template variable {name}"),
                        ));
                    }
                },
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn origin(line: usize, column: usize) -> Origin {
        Origin {
            file: "f".to_owned(),
            line,
            column,
        }
    }

    fn vars() -> BTreeMap<String, String> {
        [("port.db", "5433"), ("graft.name", "auth")]
            .into_iter()
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect()
    }

    #[test]
    fn renders_variables_and_escapes() {
        let t = Template::parse(
            "postgres://localhost:{{port.db}}/{{ graft.name }} {{{{x}}",
            &origin(1, 1),
        )
        .unwrap();
        assert_eq!(
            t.render(&vars()).unwrap(),
            "postgres://localhost:5433/auth {{x}}"
        );
        assert_eq!(
            t.variables().map(|v| v.0).collect::<Vec<_>>(),
            ["port.db", "graft.name"]
        );
    }

    #[test]
    fn errors_name_the_line_and_column() {
        let t = Template::parse("a\nb {{ nope.x }}", &origin(3, 5)).unwrap();
        let e = t.render(&vars()).unwrap_err();
        assert_eq!((e.file.as_str(), e.line, e.column), ("f", 4, 3));
        assert!(e.message.contains("unknown template variable nope.x"));
        let e = Template::parse("x {{ port.db", &origin(1, 1)).unwrap_err();
        assert_eq!((e.line, e.column), (1, 3));
        assert!(e.message.contains("unclosed"));
        for bad in ["{{}}", "{{ a..b }}", "{{ a b }}", "{{ $x }}"] {
            assert!(Template::parse(bad, &origin(1, 1)).is_err(), "{bad}");
        }
    }
}
