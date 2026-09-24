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
