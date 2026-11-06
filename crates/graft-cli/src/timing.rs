//! Step timings for profiling `graft new` (Phase 8, "measure first").
//!
//! Each step is logged at debug level on the `graft::timing` target, so
//! `GRAFT_LOG=graft::timing=debug graft new ...` prints one line per step
//! with its duration and nothing else. `scripts/profile-graft-new.sh`
//! collects them.

use std::path::Path;
use std::time::Instant;

/// Runs `f`, logging how long `step` took.
pub(crate) fn time<T>(step: &'static str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let us = start.elapsed().as_micros();
    tracing::debug!(target: "graft::timing", step, us, "timing");
    out
}

/// [`time`] for a step of one layer.
pub(crate) fn time_layer<T>(step: &'static str, layer: &Path, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let us = start.elapsed().as_micros();
    tracing::debug!(target: "graft::timing", step, layer = %layer.display(), us, "timing");
    out
}
