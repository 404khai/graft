//! What a graft's blueprint provisioned (ADR 0015).

use graft_core::GraftId;
use rusqlite::params;

use crate::{Result, Store, StoreError};

/// The kind of a [`Resource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceKind {
    /// A reserved TCP port; the value is the port number.
    Port,
    /// A Postgres database; the value is its name.
    PostgresDatabase,
    /// A Docker volume; the value is its name.
    DockerVolume,
    /// A Docker network; the value is its name.
    DockerNetwork,
}

impl ResourceKind {
    /// The `kind` column's value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Port => "port",
            Self::PostgresDatabase => "postgres-database",
            Self::DockerVolume => "docker-volume",
            Self::DockerNetwork => "docker-network",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "port" => Self::Port,
            "postgres-database" => Self::PostgresDatabase,
            "docker-volume" => Self::DockerVolume,
            "docker-network" => Self::DockerNetwork,
            _ => return None,
        })
    }
}

/// One provisioned resource of a graft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// What it is.
    pub kind: ResourceKind,
    /// The blueprint's name for it (port or service name).
    pub name: String,
    /// The port number, database name or Docker name.
    pub value: String,
}

impl Store {
    /// Records the blueprint graft `id` was created with.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn set_blueprint(&mut self, id: GraftId, blueprint: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE grafts SET blueprint = ?2 WHERE graft_id = ?1",
                params![id.to_string(), blueprint],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Records `resource` for graft `id`. `false` if another graft of this
    /// Root already holds the same value (a port taken, a name in use);
    /// `true` if it is recorded, or was already recorded for this graft.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn reserve(&mut self, id: GraftId, resource: &Resource) -> Result<bool> {
        let gid = id.to_string();
        let result = self.conn.execute(
            "INSERT INTO graft_resources (graft_id, kind, name, value) VALUES (?1, ?2, ?3, ?4)",
            params![gid, resource.kind.as_str(), resource.name, resource.value],
        );
        match result {
            Ok(_) => Ok(true),
            Err(e) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) => {
                let mine: bool = self
                    .conn
                    .query_row(
                        "SELECT count(*) > 0 FROM graft_resources
                         WHERE graft_id = ?1 AND kind = ?2 AND name = ?3 AND value = ?4",
                        params![gid, resource.kind.as_str(), resource.name, resource.value],
                        |r| r.get(0),
                    )
                    .map_err(|e| self.sql(e))?;
                Ok(mine)
            }
            Err(e) => Err(self.sql(e)),
        }
    }

    /// Graft `id`'s resources, in kind and name order.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn resources(&self, id: GraftId) -> Result<Vec<Resource>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kind, name, value FROM graft_resources WHERE graft_id = ?1
                 ORDER BY kind, name",
            )
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, name, value) = row.map_err(|e| self.sql(e))?;
            let kind = ResourceKind::parse(&kind)
                .ok_or_else(|| self.corrupt(format!("graft_resources.kind {kind:?}")))?;
            out.push(Resource { kind, name, value });
        }
        Ok(out)
    }

    /// Forgets a resource once it has been released.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn release(&mut self, id: GraftId, kind: ResourceKind, name: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM graft_resources WHERE graft_id = ?1 AND kind = ?2 AND name = ?3",
                params![id.to_string(), kind.as_str(), name],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Every port any graft of this Root holds.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn reserved_ports(&self) -> Result<Vec<u16>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM graft_resources WHERE kind = 'port'")
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            let v = row.map_err(|e| self.sql(e))?;
            out.push(v.parse().map_err(|_| StoreError::Corrupt {
                path: self.path.clone(),
                detail: format!("graft_resources port {v:?}"),
            })?);
        }
        Ok(out)
    }
}
