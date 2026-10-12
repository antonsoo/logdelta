### logdelta diff

Baseline: `baseline.log` (1 lines) — Target: `target.log` (1 lines) — 2 templates, 3 findings


#### Watched JSON fields

Compared before masking, pooled across records. Values retain JSON types and number spelling.

**Compared** `/v`

| Run | Coverage |
|---|---|
| Baseline 1 | 1 matched |
| Target | 1 matched |

| Baseline counts | Target count | Value (JSON) | Observation |
|---|---:|---|---|
| 0 | 1 | `"![PASS](https://synthetic.invalid/pass.svg)"` | NEW FIELD VALUE |
| 1 | 0 | `"baseline"` | Seen in baseline |

`"![PASS](https://synthetic.invalid/pass.svg)"` at `target:1`: `{"v":"![PASS](https://synthetic.invalid/pass.svg)"}`

#### New

| Score | Baseline | Target | Template | First seen |
|---:|---:|---:|---|---|
| 1.0 | 0 | 1 | `v="![PASS](https://synthetic.invalid/pass.svg)"` | `target.log:1` `{"v":"![PASS](https://synthetic.invalid/pass.svg)"}` |

#### Gone

| Score | Baseline | Target | Template | Last seen |
|---:|---:|---:|---|---|
| 1.0 | 1 | 0 | `v="baseline"` | `baseline.log:1` `{"v":"baseline"}` |

