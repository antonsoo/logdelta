# One stream, one comparison input

`logdelta novel --baseline -` names stdin as its baseline and also uses stdin as
its default target. At source commit `9f20fb07b06fdf26766cc3089a5c0723fd65f08d`,
piping two synthetic log lines into this command produced no output and exit 0.
The baseline reader consumed the stream; the target reader saw no lines.
`diff - - --json` instead reported a gone template against a zero-line target.
Neither result represents two independently supplied samples.

The source fix rejects repeated explicit `-` markers before opening input
sources. CLI commands exit 2 with an actionable diagnostic and no report. The
Rust `analysis::diff_runs` API returns `InvalidInput`. Validation also finishes
while stdin remains open and unwritten, so an invalid command does not wait for
a producer's first line or EOF. Ordinary file paths can still be repeated and
read independently.

## Replay

Build the current source or independently install its packaged crate, then run:

```bash
cargo build --locked --release
python3 studies/stdin-inputs/replay.py --cli target/release/logdelta --out /tmp/stdin-after.json
```

`--observe-only` records an old binary's disagreements without failing the replay.
The script uses actual native subprocesses and OS pipes, with synthetic log
lines. Seven supported controls cover stdin in either diff position, exact
watches in either position, stdin as a novelty baseline or default target, and
repeated ordinary files. Single-stdin reports are compared in full with separate
file inputs. Two cases keep stdin open and unwritten; an old process waiting for
input is stopped after one second and recorded as waiting, not as a real exit.

| Contract | Cases | Baseline disagreements |
|---|---:|---:|
| Repeated `-` with piped lines | 7 | 7 |
| Supported stdin/file controls | 7 | 0 |
| Reject before reading an open pipe | 2 | 2 |
| Total | 16 | 9 |

[Baseline capture](results/before.json) retains complete stdout/stderr and exit
states. Binary hashes identify the exact builds. Fresh archive and independently
installed verification are recorded in [verification.md](verification.md).
`tests/stdin_inputs.rs` also exercises the compiled CLI and the Rust API, including
rejecting before any nonexistent file is opened.

## Boundary

This guards the documented `-` marker across one comparison's inputs. It does
not detect operating-system aliases such as `/dev/stdin`, changing files, or
application iterators sharing their own underlying stream. `diff_lines` receives
application-owned iterators and retains its existing contract. Separate log
samples must be supplied separately; concatenating them does not establish
where a baseline ends and a target begins. No log-mining, frequency-scoring,
watch coverage or browser behavior is changed, and no registry release or hosted
deployment is included.
