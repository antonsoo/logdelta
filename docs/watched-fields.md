# When a status change disappears into a template

These commands use the current source checkout. Field watches are not in the
published 0.3.4 package.

The HTTP example is a controlled local experiment: a Python HTTP server handles
20 checkout requests per run. Two good runs return 200 throughout. A fault
injected into the third run returns 503 twice and makes the process exit 1. The
capture script checks the actual HTTP responses and subprocess exit against the
server's log records. It does not demonstrate a production incident.

```sh
# Optional: capture fresh logs from a real loopback server (Python standard library).
python examples/capture_http.py

# Same templates and counts: no findings, exit 0.
cargo run --release -- diff examples/http-good.log examples/http-good-2.log \
  --target examples/http-failed.log

# Exact outcome changes: two findings, exit 1.
cargo run --release -- diff examples/http-good.log examples/http-good-2.log \
  --target examples/http-failed.log \
  --watch-field /http/status --watch-field /exit_code
```

| Field | Value (JSON) | Good run 1 | Good run 2 | Target | Observation |
| --- | --- | ---: | ---: | ---: | --- |
| `/http/status` | `200` | 20 | 20 | 18 | Seen in baselines |
| `/http/status` | `503` | 0 | 0 | 2 | New; first at target line 8 |
| `/exit_code` | `0` | 1 | 1 | 0 | Seen in baselines |
| `/exit_code` | `1` | 0 | 0 | 1 | New; target line 22 |

Numbers normally become `<NUM>` during template mining. A field watch runs
separately, before built-in or custom masks. It compares each selected scalar
against the union of values observed in all baselines. No repetition or
significance threshold applies: selecting the field explicitly says that its
exact values matter. Every distinct new target value is reported.

In the local browser build, choose **HTTP: a failure hidden by masking**. It
loads both baseline files and both watches. Open a watched field to inspect
per-run counts, coverage and first source occurrences. Clear the watches and
compare again to see the limitation of template comparison directly. Edits mark
the previous report as stale; its downloads retain its original settings and
evidence until a replacement comparison succeeds.

## Choosing a field

Use one [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901.html) per
`--watch-field`, or one per line in the browser. Pointers start with `/` and are
evaluated against a single-line JSON **object** after recognized log envelopes
are removed. This includes Docker json-file, CRI, journald and leading timestamps.
There are no wildcards, JSONPath expressions or implicit nested-key searches.

| Record | Pointer | Selected value |
| --- | --- | --- |
| `{"status":503}` | `/status` | `503` |
| `{"http":{"status":503}}` | `/http/status` | `503` |
| `{"http.response.status_code":503}` | `/http.response.status_code` | `503` |
| `{"a/b":{"~key":false}}` | `/a~1b/~0key` | `false` |
| `{"attempts":[{"code":1}]}` | `/attempts/0/code` | `1` |
| `{"":null}` | `/` | `null` |

Keys, including spaces and Unicode, are exact. JSON string escapes are decoded,
so `"a"` and `"\u0061"` compare equal. Types and **number spelling** are retained:
`200`, `200.0`, `2e2` and `"200"` are four distinct watched values. Large integers
and exponents never pass through floating-point conversion. This is deliberate
exact comparison, not numeric tolerance or a latency-regression detector.

## Coverage and limits

A watch is complete only if **each baseline and the target** contains at least
one selected scalar, and no relevant records or values were left unassessed.
Null and booleans are scalars. A missing field is counted separately from null;
unrelated plain-text lines are also counted separately. Neither creates a new
field value. They do not make a watch incomplete if each run still has a scalar
observation. This accommodates mixed event types in one file.

| Condition | Result |
| --- | --- |
| Invalid, repeated or oversized pointer; more than 16 watches | Input error |
| No selected scalar in any one run, including an empty run | Incomplete watch |
| Malformed object-shaped JSON or a duplicate member on the selected path | Incomplete; first problem line retained |
| Selected array or object | Incomplete; select a scalar inside it |
| More than 64 distinct values across the combined runs | Incomplete; first 64 retained, other occurrences counted as untracked |
| Encoded scalar larger than 4 KiB | Incomplete; counted as untracked |
| Input line larger than 1 MiB | Incomplete; counted as oversized |
| Raw source excerpt larger than 4 KiB | Excerpt clipped at a UTF-8 boundary and labelled; selected value remains complete |

For incomplete watches, counts and retained source records remain visible, but
`is_new` is `null`. An incomplete baseline cannot prove a value was never seen.
`matched` includes scalar occurrences whose values exceeded a limit; `untracked`
is that subset, not an additional class of lines. A complete watch says the
requested values were assessed, not that the logs themselves are complete.

The CLI emits a report even for incomplete watches and gives them precedence
over findings when choosing the exit status:

| Exit | Meaning |
| ---: | --- |
| 0 | Comparison complete; no findings under these settings |
| 1 | Comparison complete; at least one template, value or watched-field finding |
| 2 | Input/analysis error or at least one incomplete watch |

## Evidence and scope

CLI `--json` adds `watched_fields` only when a watch was requested. Each field
contains `pointer`, `complete`, per-run coverage and `values`. Each value has
per-baseline counts, a target count, `is_new`, first baseline/target occurrences,
and optional target context from `-C`. `value_json` is **a string containing a
JSON scalar**, so JavaScript consumers do not round a large number. Do not parse
it into a JavaScript `Number` if exactness matters. All counts are occurrence
counts, not independent trials or probabilities.

Browser report downloads also record `settings.watch_fields`. The raw JSON
download is the same result shape as the native engine. Both include original
values and source excerpts, including known values used to assess the watch;
mask rules do **not** sanitize either download. Full input logs are not bundled.

Watches pool all records in a run. If one route already returns 503 in a baseline,
a different route starting to return 503 will not introduce a new pooled value.
Filter to the service/route/event of interest before comparing when that
distinction matters. Watches do not detect changed proportions of already-known
values, missing individual fields, record ordering, or changes in a plain-text
access-log status. Default template and frequency findings still run alongside
the watches. A new value is evidence to inspect, not a causal diagnosis.

Field tracking retains a bounded value ledger per watch and streams the input;
it does not retain every record. The existing miner still retains distinct
templates, so overall memory also depends on template diversity.
