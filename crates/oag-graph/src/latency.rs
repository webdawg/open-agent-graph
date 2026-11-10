//! Request-duration histograms (spec section 86's `search_latency`/`api_latency`).
//!
//! Every other metric in this project is a *count*: how many events, how many peers, how many
//! verification attempts. Counts answer "how much", not "how slow", and until now the only
//! per-request duration this system produced was a `duration_ms` field on a `tracing` span -- which
//! is right for reading one request in a log and useless for answering "is this peer getting
//! slower?" across a million requests. A log line is not an aggregate.
//!
//! Hand-rolled, like the rest of the metrics surface: fixed bucket boundaries, atomic counters, no
//! dependency. The exposition format is Prometheus's histogram shape (`_bucket`/`_sum`/`_count`),
//! rendered by `oag-api`.
//!
//! **Values are milliseconds.** The rest of this project already reports durations in milliseconds
//! (`duration_ms` on every REST/MCP span), and a operator reading `/metrics` next to the logs
//! should not have to mentally convert. The unit is stated in each metric's `# HELP` line rather
//! than encoded in the name, matching how the existing gauges here are named.
//!
//! **Buckets are fixed, not configurable.** A histogram whose boundaries can change at runtime
//! cannot be compared across scrapes, which defeats the point. These cover three orders of
//! magnitude on either side of "interactive": a sub-millisecond cache hit up to a ten-second
//! request that probably timed out somewhere upstream.

use std::sync::atomic::{AtomicU64, Ordering};

/// Bucket upper bounds in milliseconds. The final bucket is `+Inf`, always present.
pub const LATENCY_BUCKETS_MS: [f64; 12] =
    [1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10_000.0];

/// `sum` is accumulated in microseconds as an integer, because `AtomicU64` is the only atomic
/// float-free type available and a monotonic sum of durations is exactly the case where a
/// floating-point `Atomic` would be worse (no `AtomicF64` in std, and a `Mutex<f64>` per request
/// would put a lock on the hottest path in the peer).
const MICROSPER_MS: f64 = 1_000.0;

/// A fixed-bucket latency histogram. Cloneable by sharing: every clone records into the same
/// buckets, which is how a histogram is attached to a service and read back by a metrics layer.
///
/// No `Default`: an empty bucket vector would silently drop every observation, so `new()` is the
/// only constructor.
#[derive(Debug)]
pub struct LatencyHistogram {
    /// One counter per [`LATENCY_BUCKETS_MS`] entry, plus a final `+Inf` bucket.
    buckets: Vec<AtomicU64>,
    sum_us: AtomicU64,
    count: AtomicU64,
}

impl LatencyHistogram {
    pub fn new() -> Self {
        Self { buckets: (0..LATENCY_BUCKETS_MS.len() + 1).map(|_| AtomicU64::new(0)).collect(), sum_us: AtomicU64::new(0), count: AtomicU64::new(0) }
    }

    /// Records one observation, in milliseconds.
    ///
    /// A negative duration is a programming error somewhere upstream; it is clamped to zero rather
    /// than allowed to poison the sum.
    pub fn record(&self, ms: f64) {
        let ms = ms.max(0.0);
        let index = self.bucket_index(ms);
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        self.sum_us.fetch_add((ms * MICROSPER_MS) as u64, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    fn bucket_index(&self, ms: f64) -> usize {
        // The last bucket is `+Inf`, so every observation lands somewhere.
        LATENCY_BUCKETS_MS.iter().position(|&upper| ms <= upper).unwrap_or(LATENCY_BUCKETS_MS.len())
    }

    /// A point-in-time copy. Bucket counts are **cumulative**, per the Prometheus exposition
    /// format: `_bucket{le="5"}` counts observations `<= 5ms`, not observations in `(1ms, 5ms]`.
    pub fn snapshot(&self) -> LatencySnapshot {
        let mut cumulative = 0u64;
        let mut buckets = Vec::with_capacity(LATENCY_BUCKETS_MS.len() + 1);
        for (i, &upper) in LATENCY_BUCKETS_MS.iter().enumerate() {
            cumulative += self.buckets[i].load(Ordering::Relaxed);
            buckets.push((format!("{upper}"), cumulative));
        }
        cumulative += self.buckets[LATENCY_BUCKETS_MS.len()].load(Ordering::Relaxed);
        buckets.push(("+Inf".to_string(), cumulative));
        LatencySnapshot {
            buckets,
            sum_ms: self.sum_us.load(Ordering::Relaxed) as f64 / MICROSPER_MS,
            count: self.count.load(Ordering::Relaxed),
        }
    }
}

/// A rendered-ready copy of a [`LatencyHistogram`] at one instant.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LatencySnapshot {
    /// `(le label, cumulative count)`, in ascending bucket order, ending with `+Inf`.
    pub buckets: Vec<(String, u64)>,
    pub sum_ms: f64,
    pub count: u64,
}

impl LatencySnapshot {
    /// Mean latency in milliseconds, or `0.0` when nothing has been recorded.
    pub fn mean_ms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum_ms / self.count as f64
        }
    }

    /// The `le` label for the `count`-th observation -- i.e. an approximate quantile read straight
    /// off the buckets. `q` is in `[0, 1]`; `0.5` is the median. Returns `None` when nothing has
    /// been recorded.
    ///
    /// Approximate by construction: a histogram cannot recover an exact quantile, only the bucket
    /// the value fell in. That is the standard trade for a metric that costs one atomic increment
    /// per request.
    pub fn quantile_ms(&self, q: f64) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let target = (self.count as f64 * q.clamp(0.0, 1.0)).ceil() as u64;
        let target = target.max(1);
        for (i, (_, cumulative)) in self.buckets.iter().enumerate() {
            if *cumulative >= target {
                // The bucket's own upper bound -- the conservative estimate of where the quantile
                // sits. Returning the lower edge would understate it; a histogram cannot do
                // better than a bucket boundary, and pretending otherwise would be worse than
                // reporting the boundary.
                return Some(LATENCY_BUCKETS_MS[i.min(LATENCY_BUCKETS_MS.len() - 1)]);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_histogram_is_empty_not_zero_valued() {
        let h = LatencyHistogram::new();
        let s = h.snapshot();
        assert_eq!(s.count, 0);
        assert_eq!(s.sum_ms, 0.0);
        assert_eq!(s.mean_ms(), 0.0);
        assert_eq!(s.quantile_ms(0.5), None, "no observations means no median");
    }

    #[test]
    fn every_observation_lands_in_a_bucket() {
        let h = LatencyHistogram::new();
        for ms in [0.0, 0.5, 1.0, 1.5, 5.0, 5.5, 99.0, 2500.0, 9999.0, 10_000.0, 20_000.0] {
            h.record(ms);
        }
        let s = h.snapshot();
        assert_eq!(s.count, 11);
        // The `+Inf` bucket is cumulative over everything.
        assert_eq!(s.buckets.last().unwrap().1, 11);
        // And the sum is exactly the total of what was recorded.
        assert!((s.sum_ms - 42_611.5).abs() < 0.001, "got {}", s.sum_ms);
    }

    #[test]
    fn bucket_counts_are_cumulative_per_the_exposition_format() {
        let h = LatencyHistogram::new();
        h.record(2.0); // lands in le=5
        h.record(50.0); // lands in le=100
        let s = h.snapshot();
        let le5 = s.buckets.iter().find(|(le, _)| le == "5").unwrap().1;
        let le100 = s.buckets.iter().find(|(le, _)| le == "100").unwrap().1;
        assert_eq!(le5, 1, "le=5 counts only the 2ms observation");
        assert_eq!(le100, 2, "le=100 is cumulative: both observations are <= 100ms");
    }

    #[test]
    fn a_negative_duration_is_clamped_rather_than_poisoning_the_sum() {
        let h = LatencyHistogram::new();
        h.record(-50.0);
        let s = h.snapshot();
        assert_eq!(s.count, 1);
        assert_eq!(s.sum_ms, 0.0);
    }

    #[test]
    fn quantiles_read_off_the_buckets() {
        let h = LatencyHistogram::new();
        for _ in 0..98 {
            h.record(2.0); // well inside le=5
        }
        for _ in 0..2 {
            h.record(6000.0); // inside le=10000, past the le=5000 boundary
        }
        let s = h.snapshot();
        assert_eq!(s.count, 100);
        assert_eq!(s.quantile_ms(0.5), Some(5.0), "the median is in the le=5 bucket");
        assert_eq!(s.quantile_ms(0.99), Some(10_000.0), "the 99th percentile is one of the slow ones");
    }

    #[test]
    fn clones_share_the_same_buckets() {
        // This is how a histogram travels from a service to a metrics layer: one `Arc`, many
        // handles, no channel.
        let h = std::sync::Arc::new(LatencyHistogram::new());
        let clone = h.clone();
        clone.record(3.0);
        assert_eq!(h.snapshot().count, 1);
    }
}
