//! Shared vocabulary for every Graft crate.
//!
//! `graft-core` sits at the bottom of the crate graph. It holds the types
//! that are persisted or passed between crates, so that nothing above it
//! has to agree on bare strings:
//!
//! - [`GraftId`], [`RootId`] and [`BlobHash`]: fixed-size identifiers with a
//!   canonical lowercase-hex text form.
//! - [`GraftHome`]: resolution of `$GRAFT_HOME`, the only directory Graft
//!   owns outside a repository.
//! - [`Config`]: user configuration loaded from `$GRAFT_HOME/config.toml`.
//! - [`GraftError`]: the error type for everything above.

#![forbid(unsafe_code)]

mod config;
mod error;
mod home;
mod id;

pub use config::{CONFIG_FILE_NAME, Config, LogConfig};
pub use error::{GraftError, Result};
pub use home::{GRAFT_HOME_ENV, GraftHome, HomeSource};
pub use id::{BlobHash, GraftId, ParseIdError, RootId};
