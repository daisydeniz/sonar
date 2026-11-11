//! Human-readable output of `sonar bench ...`.

use sonar_core::bench::{
    self, AnalyzeBench, DecodeBench, PipelineRun, ScanBench, ScanPhase, SearchBench, SimilarBench,
    WatchBench, WatchPhase,
};

pub(crate) fn print_similar(b: &SimilarBench) {
    println!(
        "machine: {}; scan pool {} threads",
        machine_line(&b.machine),
        b.threads
    );
    println!(
        "corpus: {} files, {} vector rows ({:.1} MB on disk), seed {:#x}; setup {:.1} s (untimed)",
        b.docs,
        b.vector_rows,
        b.vector_bytes as f64 / 1e6,
        b.seed,
        b.setup_secs
    );
    println!(
        "top {} per query; budget p95 < {} ms end to end; filter / scoring timed separately",
        b.limit, b.budget_ms
    );
    println!(
        "{:<11} {:>4} {:>8} {:>8} {:>8} {:>10} {:>10} {:>10} {:>10} {:>11}",
        "class",
        "n",
        "p50 ms",
        "p95 ms",
        "max ms",
        "filt p50",
        "filt p95",
        "score p50",
        "score p95",
        "candidates"
    );
    for c in &b.classes {
        println!(
            "{:<11} {:>4} {:>8.2} {:>8.2} {:>8.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>11.0}{}",
            c.class,
            c.n,
            c.total.p50_ms,
            c.total.p95_ms,
            c.total.max_ms,
            c.filter.p50_ms,
            c.filter.p95_ms,
            c.scoring.p50_ms,
            c.scoring.p95_ms,
            c.mean_candidates,
            if c.total.p95_ms >= b.budget_ms {
                "  OVER BUDGET"
            } else {
                ""
            }
        );
    }
}

fn machine_line(m: &bench::Machine) -> String {
    format!("{}, {} cores, {} build", m.cpu, m.cores, m.profile)
}

pub(crate) fn print_scan(b: &ScanBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "corpus: {} files in {} folders, {:.1} MB (generated in {:.1} s, untimed)",
        b.files,
        b.dirs,
        b.bytes as f64 / 1e6,
        b.generate_secs
    );
    let phase = |name: &str, p: &ScanPhase| {
        println!(
            "{name}: {:.3} s, {:.0} files/s (added {}, changed {}, moved {}, removed {}, restored {}, unchanged {}, errors {})",
            p.secs,
            p.files_per_sec,
            p.added,
            p.changed,
            p.moved,
            p.removed,
            p.restored,
            p.unchanged,
            p.errors
        );
    };
    phase("first scan   ", &b.first);
    phase("no-op rescan ", &b.noop);
    println!(
        "edits: {} changed, {} renamed, {} deleted, {} added",
        b.edits.changed, b.edits.renamed, b.edits.deleted, b.edits.added
    );
    phase("edited rescan", &b.incremental);
    println!("database: {:.1} MB", b.db_bytes as f64 / 1e6);
}

pub(crate) fn print_decode(b: &DecodeBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "files: {} found, {} decodable; {} thread(s), {} pass(es) per run",
        b.files_found, b.files_ok, b.threads, b.repeat
    );
    for r in &b.runs {
        println!(
            "meta {:<3}: {:.3} s, {:.0} files/s, {:.0} x realtime ({:.0} s of source audio)",
            if r.meta { "on" } else { "off" },
            r.secs,
            r.files_per_sec,
            r.realtime_x,
            r.source_audio_secs
        );
    }
}

fn print_watch_phase(p: &WatchPhase) {
    println!(
        "  copy: {} files, {:.1} MB in {:.2} s",
        p.copy_files,
        p.bytes_written as f64 / 1e6,
        p.copy_secs
    );
    println!(
        "  watcher scans: {} (min {} / median {} / max {} ms), duty cycle over the copy {:.0} %, incl. catch-up {:.0} %",
        p.scans.count,
        p.scans.min_ms,
        p.scans.median_ms,
        p.scans.max_ms,
        p.duty_cycle * 100.0,
        p.duty_cycle_incl_catch_up * 100.0
    );
    println!(
        "  catch-up after copy end: {:.2} s ({}); index {} of {} files ({})",
        p.catch_up_secs,
        if p.caught_up {
            "caught up"
        } else {
            "TIMED OUT"
        },
        p.db_files,
        p.expected_files,
        if p.verified { "verified" } else { "MISMATCH" }
    );
    println!("  watcher errors: {}", p.watcher_errors);
    for m in &p.error_messages {
        println!("    {m}");
    }
}

fn print_pipeline(name: &str, r: &PipelineRun) {
    println!(
        "  {name}: {} run(s), {} files in {:.2} s active, {:.0} files/s, longest gap between progress events {} ms",
        r.runs, r.files_done, r.active_secs, r.files_per_sec, r.max_progress_gap_ms
    );
    if let Some(e) = &r.error {
        println!("    pipeline FAILED: {e}");
    }
}

pub(crate) fn print_watch(b: &WatchBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "library: {} files (generated in {:.1} s, scanned in {:.2} s, untimed); watcher start-up scan {} ms; copy rate {}",
        b.files,
        b.generate_secs,
        b.initial_scan_secs,
        b.watcher_start_scan_ms,
        b.copy_rate
            .map_or("unthrottled".to_owned(), |r| format!("{r:.0} files/s"))
    );
    println!("copy only:");
    print_watch_phase(&b.copy_only);
    if let Some(base) = &b.baseline {
        println!("pipeline alone:");
        print_pipeline("no contention", base);
    }
    if let Some(c) = &b.contended {
        println!(
            "copy + watcher + pipeline ({} scans overlapped a pipeline run, {} ms in total):",
            c.overlapping_scans, c.overlapping_scan_ms
        );
        print_watch_phase(&c.watch);
        print_pipeline("with contention", &c.pipeline);
    }
}

pub(crate) fn print_analyze(b: &AnalyzeBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "library: {} files ({:.1} MB, copies of {} fixtures); threads {}, batch {}",
        b.files,
        b.bytes as f64 / 1e6,
        b.distinct_sources,
        b.threads,
        b.batch
    );
    println!(
        "setup (untimed): write {:.1} s, scan {:.1} s, cache warm-up {:.1} s",
        b.generate_secs, b.scan_secs, b.warmup_secs
    );
    println!(
        "page cache: {}",
        match (b.warm_cache, b.cloned) {
            (true, _) =>
                "warm (every file read once before the stages; numbers are CPU + database work)",
            (false, true) => "cold (--cold --clone: fresh APFS clones, no cached pages)",
            (false, false) =>
                "as left by the writes (--cold; add --clone or run `sudo purge` for a truly cold cache)",
        }
    );
    for s in &b.stages {
        println!(
            "{}: {} files, {:.3} s, {:.0} files/s (ok {}, partial {}, errors {}, skipped {})",
            s.stage,
            s.done,
            s.seconds,
            s.files_per_sec(),
            s.ok,
            s.partial,
            s.errors,
            s.skipped
        );
        let t = &s.timing;
        println!(
            "  pool: utilization {:.0} % of {} threads x wall; {} batches, slowest file {:.1} ms/batch",
            t.utilization(s.seconds) * 100.0,
            t.threads,
            t.batches,
            t.max_file_secs * 1e3 / t.batches.max(1) as f64
        );
        let pct = |x: f64| x / s.seconds.max(1e-9) * 100.0;
        println!(
            "  coordinator: claim {:.3} s ({:.1} %), commit {:.3} s ({:.1} %), waiting for workers {:.3} s ({:.1} %)",
            t.claim_secs,
            pct(t.claim_secs),
            t.commit_secs,
            pct(t.commit_secs),
            t.wait_secs,
            pct(t.wait_secs)
        );
    }
    println!("job total: {:.3} s", b.job_secs);
}

pub(crate) fn print_search(b: &SearchBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "corpus: {} files in {} libraries, {} indexed (seed {:#x}; store written in {:.1} s, untimed)",
        b.docs, b.libraries, b.indexed_docs, b.seed, b.generate_secs
    );
    println!(
        "index (on disk): built in {:.2} s, {:.1} MB, {} segment(s); re-open {:.1} ms",
        b.build_secs,
        b.index_bytes as f64 / 1e6,
        b.segments,
        b.open_ms
    );
    println!(
        "first query after open (`{}` + facets): {:.2} ms, {} matches",
        b.first_query.query, b.first_query.ms, b.first_query.matches
    );
    println!(
        "{} timed round(s) after 1 warm-up, every query with facets; budget p95 < {} ms",
        b.rounds, b.budget_ms
    );
    println!(
        "{:<14} {:>6} {:>8} {:>8} {:>8} {:>8} {:>12}",
        "class", "n", "p50 ms", "p95 ms", "p99 ms", "max ms", "mean matches"
    );
    for c in &b.classes {
        println!(
            "{:<14} {:>6} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>12.0}{}",
            c.class,
            c.n,
            c.p50_ms,
            c.p95_ms,
            c.p99_ms,
            c.max_ms,
            c.mean_matches,
            if c.p95_ms >= b.budget_ms {
                "  OVER BUDGET"
            } else {
                ""
            }
        );
    }
    println!("slowest query per class (median):");
    for c in &b.classes {
        println!(
            "  {:<14} {:>8.2} ms  {}",
            c.class, c.slowest_query_p50_ms, c.slowest_query
        );
    }
}
