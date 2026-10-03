// The example logs committed in the repository (examples/), staged into public/ at build time.
// Every one is synthetic; see examples/README.md for how each was made.
import { readBounded, MAX_LOG_BYTES } from "./files";
import { decodeLog } from "./decode";

export interface Example {
  id: string;
  label: string;
  /** What to look for, shown under the buttons once the example is loaded. */
  note: string;
  baselines: string[];
  target: string;
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
