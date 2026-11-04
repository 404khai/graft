//! The JSON-RPC 2.0 messages `graftd` speaks, one per line (ADR 0007).

use serde::{Deserialize, Serialize};

/// The `jsonrpc` member of every message.
pub const JSONRPC: &str = "2.0";

/// Error code: the line was not valid JSON.
pub const PARSE_ERROR: i64 = -32700;
/// Error code: valid JSON but not a request.
pub const INVALID_REQUEST: i64 = -32600;
/// Error code: no such method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// Error code: params do not match the method.
pub const INVALID_PARAMS: i64 = -32602;
/// Error code: a Graft error (the message says what).
pub const GRAFT_ERROR: i64 = 1;

/// A request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Always [`JSONRPC`].
    pub jsonrpc: String,
    /// Echoed in the response.
    pub id: u64,
    /// The method name.
    pub method: String,
    /// Method parameters; `null` when there are none.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A response: exactly one of `result` and `error` is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Always [`JSONRPC`].
    pub jsonrpc: String,
    /// The request's ID (`null` if it could not be read).
    pub id: Option<u64>,
    /// The result on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// A successful response.
    #[must_use]
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC.to_owned(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    /// An error response.
    #[must_use]
    pub fn err(id: Option<u64>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC.to_owned(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    /// One of the codes in this module.
    pub code: i64,
    /// What went wrong, for people.
    pub message: String,
}

/// Result of `ping`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    /// The daemon's version.
    pub version: String,
    /// The daemon's process ID.
    pub pid: u32,
}

/// Params of `mount` and `unmount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraftRef {
    /// The Root's ID.
    pub root: String,
    /// The graft's name.
    pub graft: String,
}

/// Result of `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mounted {
    /// The mount point.
    pub path: std::path::PathBuf,
}

/// Result of `unmount`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unmounted {
    /// Whether the graft was mounted at all.
    pub was_mounted: bool,
    /// Whether it was busy and was detached lazily.
    pub lazy: bool,
}

/// One entry of the result of `mounts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountInfo {
    /// The Root's ID.
    pub root: String,
    /// The graft's name.
    pub graft: String,
    /// The mount point.
    pub path: std::path::PathBuf,
}

/// Params of `set_actor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorParams {
    /// The Root's ID.
    pub root: String,
    /// The graft's name.
    pub graft: String,
    /// The actor to record on following ops, or `None` to clear it.
    pub actor: Option<String>,
}

/// Params of `gc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRef {
    /// The Root's ID.
    pub root: String,
}

/// Result of `gc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swept {
    /// Blobs removed.
    pub blobs: u64,
    /// Bytes freed.
    pub bytes: u64,
}
