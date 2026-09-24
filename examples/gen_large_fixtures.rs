//! Generates the large, synthetic CI-log fixtures under `examples/large/`.
//!
//! Not part of the library or CLI; run directly with:
//!   cargo run --release --example gen_large_fixtures
//!
//! Simulates a realistic-looking parallel test run: a `pytest-xdist`-style stream of
//! thousands of individual test results from a fixed, shared 600-test suite (shuffled per
//! worker per run, the bulk of the line count, collapsing to a handful of templates, as real
//! parallel test output does), interleaved with ~50 sharded microservices each logging one of
//! 8 message kinds a stable number of times per run (the bulk of the *template* count: each
//! (service, message-kind) pair is its own log statement shape).
//!
//! What deliberately differs between files:
//! - one message kind's rate (a "retrying connection" warning) varies a lot across the three
//!   baseline files (a genuinely flaky signal), so `diff`'s flakiness down-weighting has
//!   something real to suppress when the target's rate lands in the same noisy range;
//! - the target removes one (service, message) pair entirely (a GONE finding), adds one new
//!   one plus one new structured JSON error event (two NEW findings), and flips one specific,
//!   fixed test's outcome from PASSED (in every baseline) to FAILED (a NEW VALUE finding) —
//!   four findings total, buried in thousands of otherwise-identical lines.
//!
//! A tiny xorshift PRNG keeps this dependency-free and exactly reproducible for a given seed.

use std::fs::{self, File};
use std::io::{BufWriter, Write};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

fn uuid(rng: &mut Rng) -> String {
    let hex = |rng: &mut Rng, n: usize| -> String {
        (0..n)
            .map(|_| std::char::from_digit(rng.below(16) as u32, 16).unwrap())
            .collect()
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(rng, 8),
        hex(rng, 4),
        hex(rng, 4),
        hex(rng, 4),
        hex(rng, 12)
    )
}

fn timestamp(base_ms: u64, offset_ms: u64) -> String {
    let total_ms = base_ms + offset_ms;
    let secs = total_ms / 1000;
    let ms = total_ms % 1000;
    let days = secs / 86400;
    let rem = secs % 86400;
    format!(
        "2026-02-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1 + (days % 27),
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        ms
    )
}

const SERVICE_BASES: &[&str] = &[
    "auth", "cache", "db", "billing", "search", "notify", "gateway", "media", "queue", "sched",
];
const SHARDS: &[&str] = &["0", "1", "2", "3", "4"];
const MESSAGE_KINDS: usize = 8;

// Round, discrete cache-hit-rate buckets rather than a wide uniform range: a real cache
// reports something like "we're at our usual ~90%", not a fresh random number each time, and
// keeping this genuinely low-cardinality (instead of relying on a small sample missing most
// of a 40-value range by chance) is what makes the NEW VALUE demo mean something.
const CACHE_HIT_RATES: &[u64] = &[85, 88, 90, 92, 95];

fn service_message(kind: usize, rng: &mut Rng, dep: &str) -> String {
    match kind {
        0 => format!("connected to upstream in {}ms", 5 + rng.below(80)),
        1 => format!(
            "cache hit rate {}%",
            CACHE_HIT_RATES[rng.below(CACHE_HIT_RATES.len() as u64) as usize]
        ),
        2 => format!("processed batch of {} items", 10 + rng.below(500)),
        3 => "health check ok".to_string(),
        4 => format!(
            "retrying connection to {dep} (attempt {}/3)",
            1 + rng.below(3)
        ),
        5 => format!("warning: slow query took {}ms", 200 + rng.below(2000)),
        6 => format!("closed idle connection after {}s", 30 + rng.below(300)),
        _ => format!(
            "gc pause {}ms, heap {}MB",
            5 + rng.below(120),
            100 + rng.below(900)
        ),
    }
}

const MODULES: &[&str] = &[
    "test_auth",
    "test_billing",
    "test_search",
    "test_notify",
    "test_gateway",
    "test_media",
    "test_queue",
    "test_sched",
    "test_users",
    "test_orders",
    "test_payments",
    "test_reports",
    "test_export",
    "test_import",
    "test_webhooks",
];
const TEST_VERBS: &[&str] = &[
    "create",
    "update",
    "delete",
    "list",
    "validate",
    "sync",
    "retry",
    "cache",
    "expire",
    "notify",
    "render",
    "parse",
    "authorize",
    "paginate",
    "reconcile",
];
const TEST_NOUNS: &[&str] = &[
    "user",
    "session",
    "invoice",
    "refund",
    "webhook",
    "query",
    "index",
    "template",
    "attachment",
    "subscription",
    "shipment",
    "coupon",
    "region",
    "quota",
    "token",
];
const DEPS: &[&str] = &["postgres", "redis", "kafka", "s3", "elasticsearch", "vault"];
const SKIP_REASONS: &[&str] = &[
    "requires-network",
    "flaky-see-issue-482",
    "not-implemented-here",
];

/// A fixed, shared 600-test suite (same test identities in every run) so baselines are
/// genuinely comparable, the way three CI runs of the same commit would be. Built once with
/// its own fixed seed, independent of any particular run's seed.
fn build_suite() -> Vec<(usize, usize, usize)> {
    let mut rng = Rng(0xC0FFEE);
    let mut suite = Vec::with_capacity(600);
    for m in 0..MODULES.len() {
        for v in 0..TEST_VERBS.len() {
            for n in 0..TEST_NOUNS.len() {
                suite.push((m, v, n));
            }
        }
    }
    rng.shuffle(&mut suite);
    suite.truncate(600);

    // Guarantee the designated "flip" test (test_billing::test_reconcile_refund, see
    // `generate`) survives the truncation, however the shuffle landed.
    let flip = (
        MODULES.iter().position(|m| *m == "test_billing").unwrap(),
        TEST_VERBS.iter().position(|v| *v == "reconcile").unwrap(),
        TEST_NOUNS.iter().position(|n| *n == "refund").unwrap(),
    );
    if !suite.contains(&flip) {
        suite[0] = flip;
    }
    suite
}

/// A fixed ~4% subset of the suite is "known flaky": it skips on roughly a third of runs,
/// same tests every time, rather than every test being an independent coin flip each run.
fn is_flaky_skip(idx: usize) -> bool {
    idx.wrapping_mul(2654435761) % 100 < 4
}

/// Writes one full synthetic run. `retry_rate` is the deliberately-varying number of
/// "retrying connection" lines per service instance (baselines: 1, 6, 3; target: 3 — inside
/// the noisy range every baseline already showed). `inject_failure` (target only) adds the
/// one new service message, the one new structured error event, and flips the designated
/// test's outcome.
fn generate(
    path: &str,
    seed: u64,
    retry_rate: u64,
    inject_failure: bool,
    suite: &[(usize, usize, usize)],
) -> std::io::Result<usize> {
    let mut rng = Rng(seed | 1);
    let base_ms = 1_770_000_000_000u64;

    // -- Build every line as (offset_ms, worker_or_tag, text), then sort by offset so the
    // service stream and the test stream interleave the way concurrent workers really would.
    let mut lines: Vec<(u64, String)> = Vec::with_capacity(4000);
    let mut t = 0u64;

    // Test stream: the whole suite, once, in a per-run-shuffled order across 6 workers.
    let mut order: Vec<usize> = (0..suite.len()).collect();
    rng.shuffle(&mut order);
    for idx in order {
        let (m, v, n) = suite[idx];
        t += 3 + rng.below(20);
        let worker = rng.below(6);
        let dur = 1 + rng.below(250);
        let trace = uuid(&mut rng);
        let is_flip = inject_failure
            && MODULES[m] == "test_billing"
            && TEST_VERBS[v] == "reconcile"
            && TEST_NOUNS[n] == "refund";
        let skip = !is_flip && is_flaky_skip(idx) && rng.below(3) == 0;

        if is_flip {
            lines.push((
                t,
                format!(
                    "[pytest-xdist] gw{worker} {}.py::test_{}_{} FAILED in {dur}ms trace={trace}",
                    MODULES[m], TEST_VERBS[v], TEST_NOUNS[n]
                ),
            ));
            for tb in [
                "    def test_reconcile_refund():",
                "        result = reconcile(refund_id=refund.id)",
                ">       assert result.status == \"settled\"",
                "E       AssertionError: expected 'settled', got 'pending_manual_review'",
            ] {
                t += 1;
                lines.push((t, format!("[pytest-xdist] gw{worker} {tb}")));
            }
        } else if skip {
            let reason = SKIP_REASONS[idx % SKIP_REASONS.len()];
            lines.push((
                t,
                format!(
                    "[pytest-xdist] gw{worker} {}.py::test_{}_{} SKIPPED (reason: {reason})",
                    MODULES[m], TEST_VERBS[v], TEST_NOUNS[n]
                ),
            ));
        } else {
            lines.push((
                t,
                format!(
                    "[pytest-xdist] gw{worker} {}.py::test_{}_{} PASSED in {dur}ms trace={trace}",
                    MODULES[m], TEST_VERBS[v], TEST_NOUNS[n]
                ),
            ));
        }
    }
    let test_stream_end = t;

    // Service stream: every (tag, kind) pair gets a stable ~5 occurrences, spread evenly
    // across the run, except the deliberately-flaky "retrying connection" kind (index 4).
    // Each tag retries against one fixed "usual" dependency (a service mostly talks to its
    // own primary dependency, not a fresh random one every time) so the retry line's target
    // value stays genuinely low-cardinality and *stable* across baselines instead of a wide
    // per-line-random choice occasionally missing baseline coverage by sampling accident.
    for base in SERVICE_BASES {
        for (shard_idx, shard) in SHARDS.iter().enumerate() {
            let tag = format!("[svc-{base}-{shard}]");
            let usual_dep = DEPS[(base.len() + shard_idx) % DEPS.len()];
            let removed_kind = if inject_failure && *base == "search" && *shard == "3" {
                Some(3) // "health check ok" for this one tag only: a clean GONE finding
            } else {
                None
            };
            for kind in 0..MESSAGE_KINDS {
                if removed_kind == Some(kind) {
                    continue;
                }
                let count = if kind == 4 { retry_rate } else { 14 };
                for _ in 0..count {
                    let offset = rng.below(test_stream_end.max(1));
                    lines.push((
                        offset,
                        format!("{tag} {}", service_message(kind, &mut rng, usual_dep)),
                    ));
                }
            }
        }
    }
    if inject_failure {
        // One new (service, message) pair never seen in any baseline: a clean NEW finding.
        lines.push((
            test_stream_end / 3,
            "[svc-gateway-1] circuit breaker opened for downstream payments-api".to_string(),
        ));
        // One new structured error event, buried in the middle: another clean NEW finding.
        lines.push((
            test_stream_end / 2,
            format!(
                "[svc-billing-2] {{\"level\":\"error\",\"msg\":\"refund settlement webhook timed out\",\"refund_id\":\"{}\",\"attempts\":3}}",
                uuid(&mut rng)
            ),
        ));
    }

    lines.sort_by_key(|(offset, _)| *offset);

    let file = File::create(path)?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    for (offset, text) in &lines {
        writeln!(out, "{} {text}", timestamp(base_ms, *offset))?;
    }
    out.flush()?;
    Ok(lines.len())
}

fn main() -> std::io::Result<()> {
    let dir = "examples/large";
    fs::create_dir_all(dir)?;
    let suite = build_suite();

    let n1 = generate(&format!("{dir}/baseline-1.log"), 1001, 4, false, &suite)?;
    let n2 = generate(&format!("{dir}/baseline-2.log"), 2002, 22, false, &suite)?;
    let n3 = generate(&format!("{dir}/baseline-3.log"), 3003, 11, false, &suite)?;
    let nt = generate(&format!("{dir}/target-failure.log"), 4004, 11, true, &suite)?;

    println!("baseline-1.log: {n1} lines");
    println!("baseline-2.log: {n2} lines");
    println!("baseline-3.log: {n3} lines");
    println!("target-failure.log: {nt} lines");
    Ok(())
}
