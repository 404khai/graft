//! `[blueprint.<name>]` tables: a graft's runtime environment (ADR 0015).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;
use toml::Spanned;

use crate::template::{Origin, Template};
use crate::{BlueprintError, Locator, parse_file, parse_layers, safe_relative};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBlueprint {
    layers: Option<Vec<Spanned<String>>>,
    #[serde(default)]
    setup: Vec<Spanned<String>>,
    #[serde(default)]
    env_files: Vec<Spanned<RawEnvFile>>,
    #[serde(default)]
    env: BTreeMap<String, Spanned<String>>,
    #[serde(default)]
    port: Vec<Spanned<RawPort>>,
    #[serde(default)]
    service: Vec<Spanned<RawService>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawEnvFile {
    Path(String),
    Table(RawEnvFileTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvFileTable {
    from: String,
    to: Option<String>,
    #[serde(default)]
    render: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPort {
    name: Spanned<String>,
    container_port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawService {
    name: Spanned<String>,
    kind: Spanned<String>,
    template: Option<Spanned<String>>,
}

/// A port a graft reserves, exposed as `{{port.<name>}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortSpec {
    /// The port's name.
    pub name: String,
    /// The port the app listens on inside a container, if it runs in one
    /// (informational: Graft maps nothing).
    pub container_port: Option<u16>,
}

/// What a service provisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceKind {
    /// A Postgres database cloned from `template` for each graft.
    PostgresTemplate {
        /// The template database's name.
        template: String,
    },
    /// A Docker volume named for the graft.
    DockerVolume,
    /// A Docker network named for the graft.
    DockerNetwork,
}

impl ServiceKind {
    /// The `kind` string in `graft.toml`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PostgresTemplate { .. } => "postgres-template",
            Self::DockerVolume => "docker-volume",
            Self::DockerNetwork => "docker-network",
        }
    }

    /// The `{{service.<name>.<field>}}` fields this kind provides.
    #[must_use]
    pub fn fields(&self) -> &'static [&'static str] {
        match self {
            Self::PostgresTemplate { .. } => &["database"],
            Self::DockerVolume | Self::DockerNetwork => &["name"],
        }
    }
}

/// A service a graft gets its own instance of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// The service's name.
    pub name: String,
    /// What it is.
    pub kind: ServiceKind,
}

/// An env file to put into a new graft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvFileSpec {
    /// Source, relative to the top level: in Root's working tree when not
    /// rendered, in the graft's checkout when rendered.
    pub from: PathBuf,
    /// Destination in the graft.
    pub to: PathBuf,
    /// Whether `from` is a template to render.
    pub render: bool,
}

/// One `[blueprint.<name>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueprintSpec {
    /// Its name.
    pub name: String,
    /// The `[[layer]]` paths to apply; `None` applies every layer.
    pub layers: Option<Vec<PathBuf>>,
    /// Shell commands run in the new graft, in order, confined.
    pub setup: Vec<String>,
    /// Env files to copy or render.
    pub env_files: Vec<EnvFileSpec>,
    /// Environment variables, by name, as templates.
    pub env: Vec<(String, Template)>,
    /// Ports to reserve.
    pub ports: Vec<PortSpec>,
    /// Services to provision.
    pub services: Vec<ServiceSpec>,
}

/// Variables every blueprint template may use.
pub const GRAFT_VARIABLES: &[&str] = &[
    "graft.name",
    "graft.id",
    "graft.path",
    "graft.blueprint",
    "root.path",
];

fn simple_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !s.starts_with('-')
}

fn env_name(s: &str) -> bool {
    let mut b = s.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// A Postgres identifier Graft accepts unquoted.
pub(crate) fn pg_identifier(s: &str) -> bool {
    s.len() <= 63 && env_name(s) && s.bytes().all(|c| !c.is_ascii_uppercase())
}

impl Locator<'_> {
    /// The origin of a TOML string's contents, given its span. Exact for
    /// strings without escapes; otherwise the string's opening quote.
    fn string_origin(&self, span: std::ops::Range<usize>, value: &str) -> Origin {
        let raw = self.text.get(span.clone()).unwrap_or_default();
        let quote = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
            3
        } else {
            1
        };
        let inner = raw.get(quote..raw.len().saturating_sub(quote));
        let mut start = if inner == Some(value) {
            span.start + quote
        } else {
            span.start
        };
        // A multi-line string drops a newline right after its opening quotes.
        if quote == 3 && inner.is_some_and(|i| i.strip_prefix('\n') == Some(value)) {
            start = span.start + quote + 1;
        }
        let e = self.error(start, "");
        Origin {
            file: e.file,
            line: e.line,
            column: e.column,
        }
    }
}

/// Parses every `[blueprint.<name>]` of a `graft.toml`, by name.
///
/// # Errors
///
/// A [`BlueprintError`] with file, line and column for invalid TOML,
/// unknown keys, bad names, unknown layers, duplicate ports or services,
/// unknown service kinds, unsafe env file paths, and templates that use
/// unknown variables.
pub fn parse_blueprints(
    text: &str,
    file: &str,
) -> Result<BTreeMap<String, BlueprintSpec>, BlueprintError> {
    let at = Locator { file, text };
    let raw = parse_file(&at)?;
    let layers: BTreeSet<PathBuf> = parse_layers(text, file)?
        .into_iter()
        .map(|l| l.path)
        .collect();
    let mut out = BTreeMap::new();
    for (name, bp) in raw.blueprint {
        let start = bp.span().start;
        let bp = bp.into_inner();
        if !simple_name(&name) || name.contains('_') {
            return Err(at.error(
                start,
                format!("blueprint name {name:?} must be lowercase letters, digits and hyphens"),
            ));
        }
        out.insert(name.clone(), parse_one(&at, name, bp, &layers)?);
    }
    Ok(out)
}

fn parse_one(
    at: &Locator<'_>,
    name: String,
    bp: RawBlueprint,
    known_layers: &BTreeSet<PathBuf>,
) -> Result<BlueprintSpec, BlueprintError> {
    let layers = match bp.layers {
        None => None,
        Some(list) => {
            let mut out = Vec::new();
            for l in list {
                let p = PathBuf::from(l.get_ref());
                if !known_layers.contains(&p) {
                    return Err(at.error(
                        l.span().start,
                        format!("no [[layer]] has path {:?}", l.get_ref()),
                    ));
                }
                out.push(p);
            }
            Some(out)
        }
    };

    let mut setup = Vec::new();
    for s in bp.setup {
        let cmd = s.get_ref().trim();
        if cmd.is_empty() {
            return Err(at.error(s.span().start, "setup command must not be empty"));
        }
        setup.push(cmd.to_owned());
    }

    let env_files = parse_env_files(at, bp.env_files)?;

    let mut ports: Vec<PortSpec> = Vec::new();
    for p in bp.port {
        let start = p.span().start;
        let p = p.into_inner();
        let n = p.name.get_ref();
        if !simple_name(n) {
            return Err(at.error(
                p.name.span().start,
                format!("port name {n:?} must be lowercase letters, digits, _ and -"),
            ));
        }
        if ports.iter().any(|q| &q.name == n) {
            return Err(at.error(start, format!("duplicate port {n}")));
        }
        ports.push(PortSpec {
            name: n.clone(),
            container_port: p.container_port,
        });
    }

    let services = parse_services(at, bp.service)?;

    let mut env = Vec::new();
    for (key, value) in bp.env {
        if !env_name(&key) {
            return Err(at.error(
                value.span().start,
                format!("environment variable name {key:?} is not valid"),
            ));
        }
        let origin = at.string_origin(value.span(), value.get_ref());
        let t = Template::parse(value.get_ref(), &origin)?;
        for (var, line, column) in t.variables() {
            if let Some(why) = unknown_variable(var, &ports, &services) {
                return Err(t.error_at(line, column, why));
            }
        }
        env.push((key, t));
    }

    Ok(BlueprintSpec {
        name,
        layers,
        setup,
        env_files,
        env,
        ports,
        services,
    })
}

fn parse_env_files(
    at: &Locator<'_>,
    raw: Vec<Spanned<RawEnvFile>>,
) -> Result<Vec<EnvFileSpec>, BlueprintError> {
    let mut env_files = Vec::new();
    for f in raw {
        let span = f.span().start;
        let spec = match f.into_inner() {
            RawEnvFile::Path(p) => {
                let stripped = p
                    .strip_suffix(".example")
                    .or_else(|| p.strip_suffix(".template"));
                match stripped {
                    Some(to) if !to.is_empty() && !to.ends_with('/') => {
                        (p.clone(), to.to_owned(), true)
                    }
                    _ => (p.clone(), p, false),
                }
            }
            RawEnvFile::Table(t) => {
                let to = t.to.unwrap_or_else(|| t.from.clone());
                (t.from, to, t.render)
            }
        };
        let (from, to, render) = spec;
        let from =
            safe_relative(&from).map_err(|why| at.error(span, format!("env file path {why}")))?;
        let to =
            safe_relative(&to).map_err(|why| at.error(span, format!("env file path {why}")))?;
        if env_files.iter().any(|e: &EnvFileSpec| e.to == to) {
            return Err(at.error(span, format!("env file {} is written twice", to.display())));
        }
        env_files.push(EnvFileSpec { from, to, render });
    }
    Ok(env_files)
}

fn parse_services(
    at: &Locator<'_>,
    raw: Vec<Spanned<RawService>>,
) -> Result<Vec<ServiceSpec>, BlueprintError> {
    let mut services: Vec<ServiceSpec> = Vec::new();
    for s in raw {
        let start = s.span().start;
        let s = s.into_inner();
        let n = s.name.get_ref();
        if !simple_name(n) {
            return Err(at.error(
                s.name.span().start,
                format!("service name {n:?} must be lowercase letters, digits, _ and -"),
            ));
        }
        if services.iter().any(|q| &q.name == n) {
            return Err(at.error(start, format!("duplicate service {n}")));
        }
        let kind = match s.kind.get_ref().as_str() {
            "postgres-template" => {
                let Some(t) = &s.template else {
                    return Err(at.error(start, "a postgres-template service needs `template`"));
                };
                if !pg_identifier(t.get_ref()) {
                    return Err(at.error(
                        t.span().start,
                        "template must be a lowercase Postgres identifier (letters, digits, _)",
                    ));
                }
                ServiceKind::PostgresTemplate {
                    template: t.get_ref().clone(),
                }
            }
            k @ ("docker-volume" | "docker-network") => {
                if let Some(t) = &s.template {
                    return Err(
                        at.error(t.span().start, format!("a {k} service takes no template"))
                    );
                }
                if k == "docker-volume" {
                    ServiceKind::DockerVolume
                } else {
                    ServiceKind::DockerNetwork
                }
            }
            other => {
                return Err(at.error(
                    s.kind.span().start,
                    format!(
                        "unknown service kind {other:?} (expected postgres-template, docker-volume or docker-network)"
                    ),
                ));
            }
        };
        services.push(ServiceSpec {
            name: n.clone(),
            kind,
        });
    }
    Ok(services)
}

/// Why `var` is not a variable this blueprint provides, if it is not.
#[must_use]
pub fn unknown_variable(var: &str, ports: &[PortSpec], services: &[ServiceSpec]) -> Option<String> {
    if GRAFT_VARIABLES.contains(&var) {
        return None;
    }
    let segments: Vec<&str> = var.split('.').collect();
    match segments.as_slice() {
        ["env", name] if env_name(name) => None,
        ["port", p] if ports.iter().any(|q| q.name == *p) => None,
        ["port", p] => Some(format!(
            "unknown template variable {var}: no port named {p}"
        )),
        ["service", s, field] => match services.iter().find(|q| q.name == *s) {
            Some(q) if q.kind.fields().contains(field) => None,
            Some(q) => Some(format!(
                "unknown template variable {var}: a {} service has {}",
                q.kind.as_str(),
                q.kind.fields().join(", ")
            )),
            None => Some(format!(
                "unknown template variable {var}: no service named {s}"
            )),
        },
        _ => Some(format!(
            "unknown template variable {var} (known: graft.name, graft.id, graft.path, graft.blueprint, root.path, port.<name>, service.<name>.<field>, env.<NAME>)"
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const WEB: &str = r#"[[layer]]
path = "node_modules"
inputs = ["package.json"]
build = "npm ci"

[blueprint.web]
layers = ["node_modules"]
setup = ["npm run db:migrate"]
env_files = [".env.example", ".secrets", { from = "conf/app.tpl", to = "conf/app.ini", render = true }]

[blueprint.web.env]
DATABASE_URL = "postgres://localhost:{{port.db}}/{{service.db.database}}"
NAME = "{{graft.name}}"

[[blueprint.web.port]]
name = "app"
container_port = 3000

[[blueprint.web.port]]
name = "db"

[[blueprint.web.service]]
name = "db"
kind = "postgres-template"
template = "app_dev"

[[blueprint.web.service]]
name = "cache"
kind = "docker-volume"
"#;

    #[test]
    fn parses_the_agents_md_example() {
        let bps = parse_blueprints(WEB, "graft.toml").unwrap();
        let web = &bps["web"];
        assert_eq!(
            web.layers.as_deref(),
            Some(&[PathBuf::from("node_modules")][..])
        );
        assert_eq!(web.setup, ["npm run db:migrate"]);
        assert_eq!(
            web.env_files,
            [
                EnvFileSpec {
                    from: ".env.example".into(),
                    to: ".env".into(),
                    render: true
                },
                EnvFileSpec {
                    from: ".secrets".into(),
                    to: ".secrets".into(),
                    render: false
                },
                EnvFileSpec {
                    from: "conf/app.tpl".into(),
                    to: "conf/app.ini".into(),
                    render: true
                },
            ]
        );
        assert_eq!(web.ports.len(), 2);
        assert_eq!(web.ports[0].container_port, Some(3000));
        assert_eq!(
            web.services[0].kind,
            ServiceKind::PostgresTemplate {
                template: "app_dev".into()
            }
        );
        assert_eq!(web.env[0].0, "DATABASE_URL");
        let vars: BTreeMap<String, String> =
            [("port.db", "5500"), ("service.db.database", "app_dev_a")]
                .into_iter()
                .map(|(a, b)| (a.into(), b.into()))
                .collect();
        assert_eq!(
            web.env[0].1.render(&vars).unwrap(),
            "postgres://localhost:5500/app_dev_a"
        );
    }

    fn err(text: &str) -> BlueprintError {
        parse_blueprints(text, "graft.toml").unwrap_err()
    }

    #[test]
    fn template_errors_point_at_the_variable() {
        let e = err("[blueprint.web.env]\nA = \"x {{port.nope}}\"\n");
        assert_eq!((e.file.as_str(), e.line, e.column), ("graft.toml", 2, 8));
        assert!(e.message.contains("no port named nope"), "{e}");
        let e = err("[blueprint.web.env]\nA = \"\"\"\nline one\n  {{ graft.nmae }}\"\"\"\n");
        assert_eq!((e.line, e.column), (4, 3), "{e}");
        let e = err("[blueprint.web.env]\nA = \"{{ port.x\"\n");
        assert_eq!((e.line, e.column), (2, 6));
        assert!(e.message.contains("unclosed"));
    }

    #[test]
    fn rejects_invalid_blueprints() {
        for (text, needle) in [
            ("[blueprint.Web]\n", "blueprint name"),
            ("[blueprint.web]\nlayers = [\"nope\"]\n", "no [[layer]]"),
            ("[blueprint.web]\nsetup = [\" \"]\n", "setup command"),
            ("[blueprint.web]\nenv_files = [\"../x\"]\n", "env file path"),
            ("[blueprint.web]\nbogus = 1\n", "unknown field"),
            (
                "[[blueprint.web.port]]\nname = \"a\"\n[[blueprint.web.port]]\nname = \"a\"\n",
                "duplicate port",
            ),
            (
                "[[blueprint.web.service]]\nname = \"d\"\nkind = \"mysql\"\n",
                "unknown service kind",
            ),
            (
                "[[blueprint.web.service]]\nname = \"d\"\nkind = \"postgres-template\"\n",
                "needs `template`",
            ),
            (
                "[[blueprint.web.service]]\nname = \"d\"\nkind = \"postgres-template\"\ntemplate = \"a;drop\"\n",
                "Postgres identifier",
            ),
            ("[blueprint.web.env]\n\"A-B\" = \"x\"\n", "not valid"),
            (
                "[blueprint.web.env]\nA = \"{{service.d.name}}\"\n",
                "no service named d",
            ),
        ] {
            let e = err(text);
            assert!(e.message.contains(needle), "{text:?}: {e}");
        }
    }
}
