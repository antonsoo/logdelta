# A known checkout error becomes common

These are real responses from the repository's loopback HTTP server under a
controlled injected fault. They are not a production incident or a claim about
detection accuracy on production logs.

Each run sends 1,000 requests to `/checkout` and 1,000 to `/maintenance`.
Checkout returns 503 for 10, 12 and 200 requests respectively; maintenance
always returns 503. The client records every actual status in
[`http-rate-observed.json`](http-rate-observed.json), independently of server
logging. Each log contains 2,000 request records plus startup and exit records.
Timestamps and durations come from the capture, not a log generator.

The exact field watcher sees the same values in all runs. The optional rate
check finds the checkout increase. [Usage and interpretation](../../docs/field-rates.md).

```sh
# Capture fresh evidence elsewhere; leave the committed observation intact.
python3 examples/capture_http.py --scenario rate-change --output-directory /tmp/http-rates

cargo build --locked --release
target/release/logdelta diff examples/http-rates/http-rate-good.log \
  examples/http-rates/http-rate-good-2.log \
  --target examples/http-rates/http-rate-failed.log \
  --watch-field /http/status --watch-by /route --watch-rate-change 5
# Expected exit 1; remove the last option and its value for exit 0.

# Verification dependencies are separate from the application.
uv venv --python 3.12 /tmp/logdelta-rate-oracle
uv pip install --python /tmp/logdelta-rate-oracle/bin/python scipy==1.18.0 numpy==2.5.3
/tmp/logdelta-rate-oracle/bin/python examples/http-rates/verify.py \
  --binary target/release/logdelta --output /tmp/logdelta-rate-verification

# Build the actual browser engine, then compare native and WASM result objects.
npm --prefix web ci
npm --prefix web run build
node examples/http-rates/verify-wasm.mjs target/release/logdelta \
  web/src/generated/logdelta.wasm /tmp/logdelta-rate-wasm
```

The Python verifier matches all 6,000 client statuses against server records and
ledger counts, checks scores with SciPy/NumPy, exercises nine additional scenarios
and retains a sparse-count counterexample to interpreting the score as a p-value.
The WASM verifier compares complete native and actual compiled-WASM JSON results
for enabled/disabled rates, unchanged input, missing groups and malformed records.
It rounds non-integral numbers to 12 significant digits only for native/WASM
floating-point comparison. Both write source/engine digests and observations.

The raw captures and verification drivers are excluded from the installed crate.
They remain available here in the source repository.
