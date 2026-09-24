# Example logs

Every file in this directory is **synthetic**, hand-written to look like real tool output
(pytest, Vite/npm, a Kubernetes container log stream) for demos and tests. None of it comes
from a real run.

| File | What it is |
|---|---|
| `pytest-pass.log` / `pytest-fail.log` | Same pytest suite, one clean run and one with a failing test and its traceback. |
| `npm-build.log` | A Vite production build. |
| `k8s-service.log` / `k8s-service-incident.log` | A container's stdout/stderr as `kubectl logs --timestamps` would show it: JSON startup line, an access-log line per request, then (in the incident file) a failed DB connection and a Go panic. |

Try, from the repository root:

```console
$ cargo run --release -- diff examples/pytest-pass.log --target examples/pytest-fail.log
$ cargo run --release -- templates examples/k8s-service.log
$ cargo run --release -- diff examples/k8s-service.log --target examples/k8s-service-incident.log
```
