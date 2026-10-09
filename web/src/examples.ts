// The example logs committed in the repository (examples/), staged into public/ at build time.
// Fixtures are synthetic or controlled local experiments; see examples/README.md.
import { readBounded, MAX_LOG_BYTES } from "./files";
import { decodeLog } from "./decode";

export interface Example {
  id: string;
  label: string;
  /** What to look for, shown under the buttons once the example is loaded. */
  note: string;
  baselines: string[];
  target: string;
  watchFields?: string[];
  watchBy?: string[];
  watchRateChange?: number;
}

export const EXAMPLES: Example[] = [
  {
    id: "pytest",
    label: "pytest: one test starts failing",
    note: "A passing pytest run against a failing one. The traceback is new, and one test's status flips from PASSED to FAILED on a line whose shape didn't change.",
    baselines: ["examples/pytest-pass.log"],
    target: "examples/pytest-fail.log",
  },
  {
    id: "k8s",
    label: "Kubernetes: errors after a deploy",
    note: "The same service's container logs (kubectl logs format) before and during an incident. Timestamps, pod IPs and payload sizes are masked, so only the new behavior remains.",
    baselines: ["examples/k8s-service.log"],
    target: "examples/k8s-service-incident.log",
  },
  {
    id: "http",
    label: "HTTP: a failure hidden by masking",
    note: "Controlled local HTTP experiment, with an injected fault: two requests return 503 and the worker exits with code 1. Template comparison alone finds nothing. Watching /http/status and /exit_code exposes both changes; timestamps and durations still stay quiet.",
    baselines: ["examples/http-good.log", "examples/http-good-2.log"],
    target: "examples/http-failed.log",
    watchFields: ["/http/status", "/exit_code"],
  },
  {
    id: "http-routes",
    label: "HTTP: an error on the wrong route",
    note: "Controlled local HTTP experiment: /maintenance always returns 503, while two /checkout requests start returning 503. A pooled status watch finds nothing. Grouping by /route exposes the checkout change. Clear Compare within groups and compare again to see the difference.",
    baselines: ["examples/http-routes-good.log", "examples/http-routes-good-2.log"],
    target: "examples/http-routes-failed.log",
    watchFields: ["/http/status"],
    watchBy: ["/route"],
  },
  {
    id: "http-rates",
    label: "HTTP: a known error becomes common",
    note: "6,000 captured local HTTP responses with deliberately injected faults. Checkout's 503 rate rises from 1% and 1.2% to 20%, while maintenance stays unchanged. Both outcomes were already known. Uncheck Compare rates of known values and compare again: novelty alone finds nothing.",
    baselines: ["examples/http-rates/http-rate-good.log", "examples/http-rates/http-rate-good-2.log"],
    target: "examples/http-rates/http-rate-failed.log",
    watchFields: ["/http/status"],
    watchBy: ["/route"],
    watchRateChange: 5,
  },
  {
    id: "large",
    label: "CI run: 3 baselines, 24,392 lines (1.8 MB)",
    note: "Three passing runs of a simulated parallel test suite against a failing one. 6,042 target lines reduce to a handful of findings, one of them the failing test.",
    baselines: ["examples/large/baseline-1.log", "examples/large/baseline-2.log", "examples/large/baseline-3.log"],
    target: "examples/large/target-failure.log",
  },
];

export async function loadExample(example: Example, signal: AbortSignal): Promise<{ baselines: string[]; target: string }> {
  const fetchText = async (path: string) => {
    const response = await fetch(`${import.meta.env.BASE_URL}${path}`, { signal });
    if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
    if (!response.body) throw new Error(`${path}: no response body`);
    return decodeLog(await readBounded(response.body, MAX_LOG_BYTES, signal));
  };
  const [target, ...baselines] = await Promise.all([fetchText(example.target), ...example.baselines.map(fetchText)]);
  return { baselines, target: target! };
}
