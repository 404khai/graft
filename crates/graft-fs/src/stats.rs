//! Counters for the virtual filesystem, for `graft stats` (ADR 0033).
//!
//! Every FUSE and NFS request records its operation, whether it failed,
//! and how long it took, in process-wide atomics: two clock reads and a
//! handful of relaxed atomic adds, with no lock and no allocation. Latency
//! goes into power-of-two microsecond buckets, from which [`snapshot`]
//! estimates percentiles. The content cache counts hits and misses.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A filesystem operation, as counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)] // the names are the operations
pub enum FsOp {
    Lookup,
    Getattr,
    Setattr,
    Readdir,
    Open,
    Read,
    Write,
    Create,
    Mkdir,
    Unlink,
    Rmdir,
    Rename,
    Symlink,
    Readlink,
    Flush,
    Fsync,
    Release,
}

impl FsOp {
    /// Every operation, in display order.
    pub const ALL: [Self; 17] = [
        Self::Lookup,
        Self::Getattr,
        Self::Setattr,
        Self::Readdir,
        Self::Open,
        Self::Read,
        Self::Write,
        Self::Create,
        Self::Mkdir,
        Self::Unlink,
        Self::Rmdir,
        Self::Rename,
        Self::Symlink,
        Self::Readlink,
        Self::Flush,
        Self::Fsync,
        Self::Release,
    ];

    /// Its name in output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lookup => "lookup",
            Self::Getattr => "getattr",
            Self::Setattr => "setattr",
            Self::Readdir => "readdir",
            Self::Open => "open",
            Self::Read => "read",
            Self::Write => "write",
            Self::Create => "create",
            Self::Mkdir => "mkdir",
            Self::Unlink => "unlink",
            Self::Rmdir => "rmdir",
            Self::Rename => "rename",
            Self::Symlink => "symlink",
            Self::Readlink => "readlink",
            Self::Flush => "flush",
            Self::Fsync => "fsync",
            Self::Release => "release",
        }
    }
}

/// Latency buckets: bucket `i` counts requests under `2^i` µs; the last
/// one also holds everything slower.
const BUCKETS: usize = 24;

struct OpCounters {
    count: AtomicU64,
    errors: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
    buckets: [AtomicU64; BUCKETS],
}

impl OpCounters {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            buckets: [const { AtomicU64::new(0) }; BUCKETS],
        }
    }
}

static OPS: [OpCounters; FsOp::ALL.len()] = [const { OpCounters::new() }; FsOp::ALL.len()];
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

fn bucket(micros: u64) -> usize {
    // Bucket i holds [2^(i-1), 2^i) µs; bucket 0 holds under 1 µs.
    let i = micros.bit_width() as usize;
    i.min(BUCKETS - 1)
}

/// Records one `op` that took `took` and failed if `failed`.
pub fn record(op: FsOp, took: Duration, failed: bool) {
    let c = &OPS[op as usize];
    let ns = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
    c.count.fetch_add(1, Ordering::Relaxed);
    if failed {
        c.errors.fetch_add(1, Ordering::Relaxed);
    }
    c.total_ns.fetch_add(ns, Ordering::Relaxed);
    c.max_ns.fetch_max(ns, Ordering::Relaxed);
    c.buckets[bucket(ns / 1000)].fetch_add(1, Ordering::Relaxed);
}

/// Runs `f` as one `op`, recording it.
///
/// # Errors
///
/// `f`'s error, unchanged.
pub fn timed<T, E>(op: FsOp, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let start = Instant::now();
    let r = f();
    record(op, start.elapsed(), r.is_err());
    r
}

/// Awaits `fut` as one `op`, recording it.
///
/// # Errors
///
/// `fut`'s error, unchanged.
pub async fn timed_async<T, E>(
    op: FsOp,
    fut: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let start = Instant::now();
    let r = fut.await;
    record(op, start.elapsed(), r.is_err());
    r
}

/// Counts a content cache lookup.
pub(crate) fn cache_lookup(hit: bool) {
    let c = if hit { &CACHE_HITS } else { &CACHE_MISSES };
    c.fetch_add(1, Ordering::Relaxed);
}

/// One operation's totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpSnapshot {
    /// The operation.
    pub op: FsOp,
    /// Requests served.
    pub count: u64,
    /// Requests that returned an error (including expected ones, such as
    /// a lookup of a name that does not exist).
    pub errors: u64,
    /// Mean latency.
    pub mean: Duration,
    /// Estimated median: the upper bound of its power-of-two bucket.
    pub p50: Duration,
    /// Estimated 99th percentile, the same way.
    pub p99: Duration,
    /// Slowest request.
    pub max: Duration,
}

/// Totals since this process started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Operations served at least once, in [`FsOp::ALL`] order.
    pub ops: Vec<OpSnapshot>,
    /// Content cache hits.
    pub cache_hits: u64,
    /// Content cache misses (reads from the object store or a layer).
    pub cache_misses: u64,
}

/// The upper bound of the bucket holding the `q`-quantile (0 < q <= 1).
fn quantile(buckets: &[u64; BUCKETS], count: u64, q: f64) -> Duration {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // a rank estimate; counts far below 2^52
    let rank = ((count as f64) * q).ceil().max(1.0) as u64;
    let mut seen = 0;
    for (i, n) in buckets.iter().enumerate() {
        seen += n;
        if seen >= rank {
            return Duration::from_micros(1u64 << i);
        }
    }
    Duration::from_micros(1u64 << (BUCKETS - 1))
}

/// The current totals.
#[must_use]
pub fn snapshot() -> Snapshot {
    let ops = FsOp::ALL
        .iter()
        .filter_map(|&op| {
            let c = &OPS[op as usize];
            let count = c.count.load(Ordering::Relaxed);
            if count == 0 {
                return None;
            }
            let buckets: [u64; BUCKETS] =
                std::array::from_fn(|i| c.buckets[i].load(Ordering::Relaxed));
            let total = c.total_ns.load(Ordering::Relaxed);
            Some(OpSnapshot {
                op,
                count,
                errors: c.errors.load(Ordering::Relaxed),
                mean: Duration::from_nanos(total / count),
                p50: quantile(&buckets, count, 0.5),
                p99: quantile(&buckets, count, 0.99),
                max: Duration::from_nanos(c.max_ns.load(Ordering::Relaxed)),
            })
        })
        .collect();
    Snapshot {
        ops,
        cache_hits: CACHE_HITS.load(Ordering::Relaxed),
        cache_misses: CACHE_MISSES.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_powers_of_two_microseconds() {
        assert_eq!(bucket(0), 0);
        assert_eq!(bucket(1), 1);
        assert_eq!(bucket(3), 2);
        assert_eq!(bucket(1000), 10);
        assert_eq!(bucket(u64::MAX), BUCKETS - 1);
    }

    #[test]
    fn quantiles_come_from_the_buckets() {
        let mut b = [0u64; BUCKETS];
        b[3] = 98; // under 8 µs
        b[10] = 2; // under 1 ms
        assert_eq!(quantile(&b, 100, 0.5), Duration::from_micros(8));
        assert_eq!(quantile(&b, 100, 0.99), Duration::from_micros(1024));
    }

    // One test touches the process-wide counters, so parallel tests
    // cannot disturb each other's totals.
    #[test]
    fn records_counts_errors_and_latency() {
        let before = snapshot();
        let count = |s: &Snapshot| {
            s.ops
                .iter()
                .find(|o| o.op == FsOp::Symlink)
                .map_or((0, 0), |o| (o.count, o.errors))
        };
        let _ = timed(FsOp::Symlink, || Ok::<_, ()>(()));
        let _ = timed(FsOp::Symlink, || Err::<(), _>(()));
        record(FsOp::Symlink, Duration::from_millis(3), false);
        let after = snapshot();
        let (c0, e0) = count(&before);
        assert_eq!(count(&after), (c0 + 3, e0 + 1));
        let s = after.ops.iter().find(|o| o.op == FsOp::Symlink).unwrap();
        assert!(s.max >= Duration::from_millis(3));
        cache_lookup(true);
        cache_lookup(false);
        let again = snapshot();
        assert!(again.cache_hits > after.cache_hits);
        assert!(again.cache_misses > after.cache_misses);
    }
}
