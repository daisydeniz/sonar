//! Benchmarks behind `sonar bench ...` (docs/PRODUCT.md performance budgets).
//!
//! Every bench works in a directory the caller owns (the CLI creates a
//! temporary one) and uses its own store file inside it, so the user's real
//! store and libraries are never touched. Corpus generation is never part
//! of a timed phase. Results are plain serializable structs so the CLI can
//! print them as text or JSON and scripts can compare runs.
//!
//! Numbers are only meaningful from a release build.

pub mod analyze;
pub mod corpus;
pub mod decode;
pub mod scan;
pub mod search;
pub mod search_corpus;
pub mod search_queries;
pub mod similar;
mod similar_corpus;
pub mod watch;
mod watch_io;
pub mod watch_pipeline;
pub mod watch_stats;

use std::process::Command;

use serde::Serialize;

pub use analyze::{bench_analyze, AnalyzeBench, AnalyzeBenchOptions};
pub use decode::{bench_decode, DecodeBench, DecodeBenchOptions, DecodeRun};
pub use scan::{bench_scan, ScanBench, ScanPhase};
pub use search::{
    bench_search, ClassLatency, FirstQuery, SearchBench, SearchBenchOptions, SEARCH_BUDGET_MS,
};
pub use search_queries::QueryClass;
pub use similar::{
    bench_similar, Percentiles, SimilarBench, SimilarBenchOptions, SimilarClass,
    SimilarClassLatency,
};
pub use watch::{bench_watch, ContendedPhase, WatchBench, WatchBenchOptions, WatchPhase};
pub use watch_io::MIN_COPY_RATE;
pub use watch_pipeline::PipelineRun;
pub use watch_stats::ScanStats;

/// The machine a benchmark ran on, recorded with every result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Machine {
    /// CPU model (`machdep.cpu.brand_string` on macOS, else `unknown`).
    pub cpu: String,
    /// Logical cores available to the process.
    pub cores: usize,
    /// `release` or `debug` build of sonar-core.
    pub profile: &'static str,
}

/// Describes the current machine and build profile.
pub fn machine() -> Machine {
    let cpu = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    Machine {
        cpu,
        cores: std::thread::available_parallelism().map_or(1, usize::from),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    }
}

/// `count / secs`, 0 when no time elapsed.
pub(crate) fn rate(count: u64, secs: f64) -> f64 {
    if secs > 0.0 {
        count as f64 / secs
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_reports_cores_and_profile() {
        let m = machine();
        assert!(m.cores >= 1);
        assert!(!m.cpu.is_empty());
        assert!(["debug", "release"].contains(&m.profile));
    }

    #[test]
    fn rate_handles_zero_time() {
        assert_eq!(rate(10, 0.0), 0.0);
        assert_eq!(rate(10, 2.0), 5.0);
    }
}
