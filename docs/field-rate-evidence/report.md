### logdelta diff

Baseline: `/home/antonsoloviev/work/projects/showcases/logdelta/examples/http-rates/http-rate-good.log`, `/home/antonsoloviev/work/projects/showcases/logdelta/examples/http-rates/http-rate-good-2.log` (4004 lines) — Target: `/home/antonsoloviev/work/projects/showcases/logdelta/examples/http-rates/http-rate-failed.log` (2002 lines) — 3 templates, 1 finding


#### Watched JSON fields

Compared before masking, within exact groups. Values retain JSON types and number spelling.

**Compared** <code>/http/status</code>

| Run | Coverage |
|---|---|
| Baseline 1 | 2000 matched; 1 absent; 1 non-JSON |
| Baseline 2 | 2000 matched; 1 absent; 1 non-JSON |
| Target | 2000 matched; 1 absent; 1 non-JSON |

| Group: <code>/route</code> | Baseline counts | Target count | Value (JSON) | Observation |
|---|---|---:|---|---|
| <code>"/checkout"</code> | 990/988 | 800 | <code>200</code> | Seen in baseline |
| <code>"/checkout"</code> | 10/12 | 200 | <code>503</code> | Seen in baseline |
| <code>"/maintenance"</code> | 1000/1000 | 1000 | <code>503</code> | Seen in baseline |

Rate check: at least 5 percentage points outside every observed baseline rate, and score at least 10.83. Percentages are rounded.

**CHANGED RATES**: <code>/route = "/checkout"</code>

Group observations: baseline 1000/1000; target 1000.

| Value (JSON) | Baseline rates | Target rate | Beyond baseline range (pp) | Score |
|---|---|---:|---:|---:|
| <code>200</code> | 99.0000% / 98.8000% | 80.0000% | -18.8000 | 338.52 |
| <code>503</code> | 1.0000% / 1.2000% | 20.0000% | +18.8000 | 287.02 |

**No rate change passed both thresholds**: <code>/route = "/maintenance"</code>

Group observations: baseline 1000/1000; target 1000.

Group <code>/route = "/checkout"</code>: 
<code>200</code> at <code>target:402</code>: <code>{"ts":"2026-10-09T09:04:16.293378+00:00","event":"request_complete","route":"/checkout","http":{"status":200},"elapsed_ms":0.055,"request_id":9007199254741393}</code>

Group <code>/route = "/checkout"</code>: 
<code>503</code> at <code>target:2</code>: <code>{"ts":"2026-10-09T09:04:14.953095+00:00","event":"request_complete","route":"/checkout","http":{"status":503},"elapsed_ms":0.122,"request_id":9007199254740993}</code>

