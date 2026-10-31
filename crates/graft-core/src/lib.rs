//! Shared vocabulary for every Graft crate.
//!
//! `graft-core` sits at the bottom of the crate graph. It holds the types
//! that are persisted or passed between crates, so that nothing above it
//! has to agree on bare strings:
//!
//! - [`GraftId`], [`RootId`] and [`BlobHash`]: fixed-size identifiers with a
//!   canonical lowercase-hex text form, plus [`LayerKey`] and
//!   [`CommandHash`]; [`CommitId`] for git commits.
//! - [`GraftName`] and [`GraftState`]: a graft's validated name and its
//!   lifecycle state.
//! - [`Timestamp`]: persisted UTC instants; [`path_to_bytes`] and
//!   [`path_from_bytes`] for persisting paths losslessly.
//! - [`GraftHome`]: resolution of `$GRAFT_HOME`, the only directory Graft
//!   owns outside a repository.
//! - [`Config`]: user configuration loaded from `$GRAFT_HOME/config.toml`.
//! - [`GraftError`]: the error type for everything above.

#![forbid(unsafe_code)]

mod commit;
mod config;
mod error;
mod graft;
mod home;
mod id;
mod paths;
mod time;

pub use commit::CommitId;
pub use config::{CONFIG_FILE_NAME, Config, LogConfig};
pub use error::{GraftError, Result};
pub use graft::{GraftName, GraftState, InvalidGraftName, MAX_GRAFT_NAME_LEN, UnknownGraftState};
pub use home::{GRAFT_HOME_ENV, GraftHome, HomeSource};
pub use id::{BlobHash, CommandHash, GraftId, LayerKey, ParseIdError, RootId};
pub use paths::{path_from_bytes, path_to_bytes};
pub use time::Timestamp;
