# Repeated JSON member names must not erase earlier diagnostics

At source `9e34dd6`, a target record contains both
`"message":"database corruption detected"` and a later
`"message":"health check passed"`. The actual CLI reports **zero findings and
exit 0**, matching a baseline containing only the latter value. A constructed
Docker-style envelope with repeated `log` members has the same failure.

The parser's ordinary JSON map kept the last member. Normalization then erased
the earlier text before template mining, including nested objects and array
elements. This is distinct from the existing exact-field lookup, which already
treated a repeated watched name as ambiguous and exited 2.

The correction parses JSON with locally unique member names before interpreting
it. Repeated decoded names at any object depth stop JSON unwrapping/flattening;
the record stays in the existing masked raw-text path, keeping each occurrence.
Normal timestamp/id/custom masking still applies. It does not select a winning
member or reject a whole mixed-format log. Separate objects can reuse a name;
literal JSON text inside a string is not parsed as an object. Exact field
watches retain their scoped lookup rules.

[RFC 8259, section 4](https://www.rfc-editor.org/rfc/rfc8259#section-4) recommends
unique object names and describes inconsistent behavior when names repeat.
Logdelta's raw fallback is a conservative application policy, not a claim that
the RFC forbids all such JSON. The Serde visitor follows its
[documented deserialization contract](https://serde.rs/impl-deserialize.html).

## Actual consumer and CLI replay

```sh
cargo build --locked --release
python3 studies/json-members/replay.py --cli target/release/logdelta \
  --crate-root . --out /tmp/json-members.json --label source
```

[`consumer.rs`](consumer.rs) is built as a separate crate with the tested
`logdelta` path dependency and `default-features=false`. It invokes the public
tokenizer and `diff_lines` API. [`replay.py`](replay.py) then runs the actual CLI
against files, gzip inputs, stdin on either side and novelty streaming. No
production service, Docker daemon or remote logging system is involved.

[`cases.json`](cases.json) contains 18 constructed cases: repeated message/log/
stream members; nested and array members; JSON-escaped equivalent names;
Unicode names; inner Docker payloads; CRI/color prefixes; late nested members;
identical repeated values; and six controls covering unique records, literal
JSON strings, separate objects, distinct Unicode names and primitive values.

The [baseline](results/before.json) disagrees in **72 of 109 workflows**;
[corrected source](results/after.json) disagrees in none. All 36 ordinary
public-API/CLI controls are identical. The extra exact-field watch keeps its
existing incomplete gate; its normal template findings now retain the earlier
diagnostic too. These are synthetic ambiguity counterexamples, not an ecosystem
accuracy benchmark. Source module, binary and input digests accompany captures.

For the baseline, build `git archive 9e34dd6`, pass its crate root and binary,
and add `--observe-only`. Consumer files and raw input/report artifacts stay in
a fresh temporary directory. The replay uses only Python's standard library;
building its consumer uses the existing cached Rust dependencies offline.

This change does not make template mining a complete semantic parser or secret
scanner. Variable masking and Drain's heuristic grouping still apply; use exact
field watches for scalar-value questions. Existing nested-value display bounds
also remain for unambiguous JSON. No crates.io release or hosted deployment is
part of this source correction.
