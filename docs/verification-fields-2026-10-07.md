# Exact field watches: local verification, 2026-10-07

Implementation: `63a2685514f92c691ac8cbe465529c41dc0c0347`.
Build dependency patch: `f79700e24053e7c6123001f50c6bd2897b359277`.
Context bounds: `74ab08fe3ca186adc0d6be641690c877541df5a6`.
These changes are local and unreleased. No package publication, push or Pages
deployment was performed in this work.

## The useful difference

The starting native release returned zero findings and exit 0 for 20 JSON
request records whose status changed from 200 to 503. Numeric masking made both
statuses the same token. Exact field watches now operate before masking, with
the same Rust implementation in the CLI and browser.

The committed HTTP fixture is captured from a real loopback server by
[`capture_http.py`](../examples/capture_http.py). Its failure is deliberately
injected. The producer checks every HTTP response and the subprocess exit
against the emitted log. It is a controlled experiment, not production evidence.

| Comparison | Template/value findings | Watched-field findings | CLI exit |
| --- | ---: | ---: | ---: |
| Two good runs against the failed run, default settings | 0 | Not requested | 0 |
| Same inputs, watching `/http/status` and `/exit_code` | 0 | 2 | 1 |
| Good against good, same watches | 0 | 0 | 0 |
| Watch a field absent from a run | Depends on logs | Unknown; visibly incomplete | 2 |

The two new values are status 503 (two occurrences, first at target line 8)
and exit code 1 (one occurrence at line 22). The status counts are 20/20 in
the baselines versus 18 ordinary and 2 failing requests in the target. The
[saved native result](verification-fields-result-2026-10-07.json) retains the
source records and coverage. Gzip, UTF-16 and stdin produced the same JSON as
ordinary UTF-8 files. An actual 80-column pseudo-terminal kept the field output
within its width, with visible clipping and a pointer to full JSON evidence.

## Browser evidence

Both Chromium and Firefox ran the production WASM engine. Actual downloaded
report results matched native `--json -C 2` exactly, including values, counts,
source lines and context. Their reported engine digest matched the module bytes
served to the worker:

```text
e0ee77d72fde9b3b387679255eeee268e849ac1afd4364ea68883f2faf3340c7
```

[Browser comparison manifest](verification-fields-browser-2026-10-07.json).
Screenshots were opened and visually inspected during implementation:

| Browser | Findings | Counts and coverage | Narrow dark view |
| --- | --- | --- | --- |
| Chromium | [Desktop](assets/fields-chromium-desktop-2026-10-07.png) | [Evidence](assets/fields-chromium-evidence-2026-10-07.png) | [390px](assets/fields-chromium-mobile-2026-10-07.png) |
| Firefox | [Desktop](assets/fields-firefox-desktop-2026-10-07.png) | [Evidence](assets/fields-firefox-evidence-2026-10-07.png) | [390px](assets/fields-firefox-mobile-2026-10-07.png) |

The automated production workflows exercise malformed and duplicate-key records,
unobserved paths, cardinality overflow, integers beyond JavaScript's exact range,
large exponents, string/number distinctions, HTML-like values, and correction
after an error. Edits cancel held real worker replies; downloads keep the last
completed comparison's settings. Skipped baseline editors retain their original
labels in both count tables and first-source evidence.

Keyboard and accessibility checks cover complete and incomplete watches at
320px in light and dark modes. All 16 axe scans across the field and existing
workspace flows reported zero violations. The browser suites also checked page
errors, unhandled rejections, CSP violations and off-origin requests; none were
observed. This is automated accessibility evidence, not a screen-reader study.

## Size check with a late change

Generated fixed-template JSON logs, one baseline and one target, put a single
503 at the last target line. This is a controlled size check, not a general
benchmark of arbitrary log formats or hardware.

| Total lines | Native wall time | Native peak RSS | Retained field values |
| ---: | ---: | ---: | ---: |
| 20,000 | 0.08 s | 7,684 KiB | 2 |
| 200,000 | 0.81 s | 7,764 KiB | 2 |

At 200,000 lines / 15,784,000 input bytes, Chromium completed the click-to-render
flow in 2,167 ms and Firefox in 1,880 ms. Respectively 83 and 88 animation frames
ran while the comparison was busy. Actual downloads correctly located the lone
503 at line 100,000. Each is a single local measurement with normal scheduling
noise. The bounded ledger does not bound the existing miner's template count.

[Native measurements](verification-fields-scale-2026-10-07.json) and
[browser measurements](verification-fields-large-browser-2026-10-07.json).
Those size timings were recorded at the first implementation commit, before the
additional excerpt cap below; they are not release performance guarantees.

A further counterexample used 16 watched fields, 40 values and 10 KB payloads
around each finding. A 415,933-byte input produced a 27,905,549-byte report because
context was duplicated for each selected field. The final implementation caps
each watched-value context window at 8 KiB and 10 lines per side, with UTF-8-safe
clipping, nearest lines retained first and `context_truncated: true` in JSON.
The same 624 findings remain, in an 8,240,589-byte report. Source excerpts and
all exact value counts remain available; the interface explicitly labels clipped
context. [Before/after measurements](verification-fields-context-2026-10-07.json).

## Fresh checkout and checks

A detached checkout with no node_modules or compiled artifacts used Node
24.21.0 / npm 11.19.0 and the locked Rust dependencies. It passed:

- `npm ci`, strict typecheck, lint, 57 web unit checks and the production build.
- All 80 Chromium/Firefox workflows, including the two-build browser-cache case.
- 172 native tests, the library doctest, Clippy with warnings denied, formatting,
  and a release binary build.

The field integration cases also passed with the declared Rust 1.85 minimum.
The library built without CLI features; the WASM wrapper test and WASM-target
Clippy passed. All 34 production files from the clean Node 24 build matched the
working Node 26 build by SHA-256. The
[asset manifest](verification-fields-assets-2026-10-07.json) records every file.
The detached checkout was advanced to the context-bounds commit and the full
checks rerun for the final implementation.

Fresh installation exposed source-map-js 1.2.1 through PostCSS/Vite. The lockfile
now selects 1.2.2, the patch for
[CVE-2026-93749](https://github.com/advisories/GHSA-68fv-2mgg-jv7q).
Only that package changed. A new build kept all 34 production files byte-identical
to the tested build; a second clean installation/build with the patched lockfile
was checked as well. `npm audit` reported zero known vulnerabilities at the time
of this verification. This is build tooling; it is not code included in the
browser's log-processing engine.

The behavior and its limits are documented in the
[field-watch guide](watched-fields.md). Field watches identify new pooled scalar
values, not route-specific regressions, changing rates of known values, missing
individual fields or a cause of failure. Original values and excerpts are not
redacted by masking rules.
