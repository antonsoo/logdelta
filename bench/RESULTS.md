# Throughput benchmarks

Measured on this machine: **AMD Ryzen 7 7800X3D, 14 vCPUs allotted (WSL2), 48 GiB RAM**,
`cargo build --release` (LTO, 1 codegen unit), 2026-09-24. `hyperfine` was not installed, so
these are `/usr/bin/time -f '%e s  %M KB max-rss'`, each command run 2-3 times back to back;
the box was concurrently running unrelated cargo builds for sibling projects during these
runs, which is visible in the variance below — treat the higher-variance numbers as a lower
bound, not a ceiling.

Data: `bench/gen_bench_log.rs` (`cargo run --release --example gen_bench_log -- <lines> <path>`),
a deterministic (seeded) synthetic mix of HTTP access lines, job-completion lines, retry
warnings, S3 upload lines, and a rare error line — the same masking-relevant fields
(timestamps, IPs, hex ids, durations, byte sizes) as the `examples/` fixtures, generated at
1M and 10M lines.

| Command | Lines | File size | Wall time | Throughput | Peak RSS |
|---|---:|---:|---:|---:|---:|
| `templates` | 1,000,000 | 81 MB | 2.80 s (best of 3; 2.80-3.90 s) | ~342k lines/s, ~28 MB/s | 7.0 MB |
| `templates` | 10,000,000 | 803 MB | 29.5-45.3 s (3 runs, contended) | ~221k-339k lines/s, ~18-27 MB/s | 7.1 MB |
| `diff` (1M baseline + 1M target) | 2,000,000 | 162 MB | 5.49 s | ~364k lines/s | 7.2 MB |
| `novel` (1M baseline, 1M target, 0 novel) | 2,000,000 | 162 MB | 5.36 s | ~373k lines/s | 7.0 MB |

**Reading these numbers:**
- Peak RSS stays under ~7.2 MB regardless of input size, for both 1M and 10M lines: memory
  is dominated by the (small, bounded-by-distinct-templates) Drain cluster table, not by the
  input, confirming the stream-processing design — nothing buffers the whole file.
- `diff`/`novel` throughput (~360-370k lines/s) is a bit higher than plain `templates`
  (~220-340k lines/s) on otherwise comparable data; both numbers are within the noise band
  created by concurrent load on this box during measurement, and should be read as "same
  order of magnitude," not as `diff` being reliably faster.
- The bottleneck is the masking regex pipeline (see `src/mask.rs`), which runs ~15 regex
  passes per line; Drain clustering itself is O(number of *distinct templates* in the
  matching length/first-token bucket) per line, which stays tiny for real logs.
- `novel`'s streaming path flushes stdout after every *printed* line (required so it works
  with `tail -f`); this run printed zero novel lines, so that cost isn't reflected here.

Reproduce:

```console
$ cargo build --release --example gen_bench_log
$ ./target/release/examples/gen_bench_log 1000000 bench/data/bench_1m.log
$ ./target/release/examples/gen_bench_log 10000000 bench/data/bench_10m.log
$ cargo build --release
$ /usr/bin/time -f '%e s  %M KB max-rss' ./target/release/logdelta templates bench/data/bench_1m.log -n 1 >/dev/null
```

## Re-measured for 0.3.0 (2026-10-01)

Same machine, same generator, `/usr/bin/time`, three runs each, with a background load
average of about 2.5 from unrelated work. The generator's 1M-line file is 84 MB and its
10M-line file 841 MB.

| Command | Lines | Wall time | Throughput | Peak RSS |
|---|---:|---:|---:|---:|
| `templates` | 1,000,000 | 2.44-2.96 s | ~340k-410k lines/s | 7.3 MB |
| `templates` | 10,000,000 | 24.96-25.59 s | ~395k lines/s | 7.3 MB |
| `diff` (1M baseline + 1M target) | 2,000,000 | 5.31-5.89 s | ~340k-377k lines/s | 7.4 MB |
| `novel` (1M baseline, 1M target, 0 novel) | 2,000,000 | 5.21-5.24 s | ~383k lines/s | 7.4 MB |

0.2.3, built from crates.io and run back to back on the same files: `templates` 2.47-2.49 s,
`diff` 5.69-5.80 s. So 0.3.0 is within noise of it; the table in the README stands.

Two cases this release was checked on specifically:

- **Grouping does not cost memory.** A 1.25M-line target in which every fifth line is one of
  500 templates the 1M-line baseline does not have (250,000 separate runs of new lines):
  6.25 s and 9.3 MB peak with grouping, 9.4-9.5 MB with `--flat`. 0.2.3 on the same pair:
  7.18 s, 8.9 MB.
- **Lines that are mostly placeholders.** 100,000 lines of the form
  `<6 digits> <6 digits> <40 hex digits> ok`, which mask to `<NUM> <NUM> <HEX> ok`. 0.2.3
  never matched such a line to the template it had started (see the 0.3.0 changelog):
  100,000 templates, 77.55 s and 45.7 MB for `templates`; a `diff` of two 20,000-line files
  took 22.83 s and 71 MB. 0.3.0: one template, 0.18 s and 7.2 MB; the `diff` of two
  100,000-line files takes 0.41 s.
