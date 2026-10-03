import "./fonts/fonts.css";
import "./style.css";
import { engine } from "./engine";
import { MAX_BASELINES, readLogFile } from "./files";
import { EXAMPLES, loadExample, type Example } from "./examples";
import { comparisonInput, exportReport, type CompletedComparison, type LogInput } from "./report";
import { baselineCounts, formatCount, headAndTail, lineCount, printable, templateParts } from "./template";
import type { Block, ContextWindow, DiffResult, Finding, ValueFinding } from "./types";

type Filter = "all" | "new" | "gone" | "changed" | "value";
type Editor = "baseline" | "target";
interface Source extends LogInput {
  id: number;
  loading?: { controller: AbortController; name: string } | undefined;
}
let nextSourceId = 1;
function newSource(input?: LogInput): Source {
  return { id: nextSourceId++, text: "", name: "No log loaded", origin: "pasted", ready: false, ...input };
}
const state = {
  baselines: [newSource()],
  active: 0,
  target: newSource(),
  revision: 0,
  filter: "all" as Filter,
  page: 0,
  details: new Set<number>(),
  sourceDetails: false,
  report: undefined as CompletedComparison | undefined,
  outcomeLines: [] as string[],
  expanded: new Set<number>(),
  comparison: undefined as AbortController | undefined,
  example: undefined as AbortController | undefined,
};

const $ = <T extends HTMLElement = HTMLElement>(id: string): T => document.getElementById(id) as T;
const errorText = (error: unknown): string => error instanceof Error ? error.message : String(error);
function esc(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function sourceFor(id: Editor): Source {
  return id === "target" ? state.target : state.baselines[state.active]!;
}
function sources(): Source[] {
  return [...state.baselines, state.target];
}
function status(message: string, isError = false): void {
  $("workspace-status").textContent = message;
  $("workspace-status").classList.toggle("is-error", isError);
}
function refreshActivity(): void {
  const importing = !!state.example || sources().some((s) => s.loading);
  const busy = importing || !!state.comparison;
  $<HTMLButtonElement>("run").disabled = busy;
  $("run").textContent = state.comparison ? "Comparing…" : importing ? "Loading logs…" : "Compare runs";
  $("cancel").hidden = !busy;
  $("results").setAttribute("aria-busy", String(!!state.comparison));
  const reportStatus = $("report-status");
  reportStatus.hidden = !state.report;
  if (state.report) {
    const stale = state.report.revision !== state.revision;
    reportStatus.classList.toggle("is-stale", stale);
    reportStatus.textContent = stale
      ? "Inputs changed. This is the last completed comparison; its downloads still describe the sources and settings below. Compare again to update it."
      : busy ? "Showing the last completed comparison while the next one is prepared."
        : "This comparison matches the loaded logs and applied settings.";
  }
}
function stopExample(): void {
  if (!state.example) return;
  state.example.abort();
  state.example = undefined;
  $("example-note").textContent = "Example loading cancelled. Your loaded logs are kept.";
}
function stopComparison(): void {
  if (!state.comparison) return;
  state.comparison.abort();
  state.comparison = undefined;
}
function stopFile(source: Source): void {
  source.loading?.controller.abort();
  source.loading = undefined;
}
function clearExampleSelection(): void {
  for (const button of document.querySelectorAll("[data-example]")) button.setAttribute("aria-pressed", "false");
  $("example-note").textContent = "";
}
/** Every edit revokes pending results before changing any input. Tab selection is not an edit. */
function changed(): void {
  stopExample();
  const wasComparing = !!state.comparison;
  stopComparison();
  state.revision++;
  clearExampleSelection();
  status(wasComparing ? "Comparison cancelled because inputs changed. Compare again when ready." : "");
  refreshActivity();
}

// ---------------------------------------------------------------------------
// theme
// ---------------------------------------------------------------------------

function applyTheme(theme: "light" | "dark" | undefined): void {
  if (theme) document.documentElement.dataset["theme"] = theme;
  const dark = theme === "dark" || (theme === undefined && matchMedia("(prefers-color-scheme: dark)").matches);
  $("theme-toggle").textContent = dark ? "Light" : "Dark";
}
function initTheme(): void {
  let stored: string | null = null;
  try { stored = localStorage.getItem("logdelta-theme"); } catch { /* Follow the system preference. */ }
  applyTheme(stored === "light" || stored === "dark" ? stored : undefined);
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (!document.documentElement.dataset["theme"]) applyTheme(undefined);
  });
  $("theme-toggle").addEventListener("click", () => {
    const next = $("theme-toggle").textContent === "Dark" ? "dark" : "light";
    applyTheme(next);
    try { localStorage.setItem("logdelta-theme", next); } catch { /* This tab still works. */ }
  });
}

// ---------------------------------------------------------------------------
// editors: operations own Source objects, never a mutable tab index
// ---------------------------------------------------------------------------

function editorHtml(id: Editor, source: Source, label: string, placeholder: string): string {
  return `
    <div class="editor" data-editor="${id}">
      <textarea id="${id}" spellcheck="false" wrap="off" aria-label="${esc(label)}" aria-describedby="${id}-source" placeholder="${esc(placeholder)}">${esc(source.text)}</textarea>
      <p class="editor-source" id="${id}-source"></p>
      <div class="editor-bar">
        <span class="editor-count" id="${id}-count"></span>
        <button type="button" class="link-button" data-empty="${id}">Use empty log</button>
        <label class="link-button">Open file…<input type="file" data-file-for="${id}" aria-label="Open file for ${esc(label.toLowerCase())}" accept=".log,.txt,.gz,text/plain" class="visually-hidden" /></label>
        <button type="button" class="link-button" data-clear="${id}">Clear</button>
      </div>
    </div>`;
}
function refreshEditor(id: Editor): void {
  const source = sourceFor(id);
  const area = $<HTMLTextAreaElement>(id);
  if (area.value !== source.text) area.value = source.text;
  $(`${id}-source`).textContent = source.loading ? `Opening ${source.loading.name}…` : printable(source.name);
  const lines = lineCount(source.text);
  $(`${id}-count`).textContent = source.ready ? `${formatCount(lines)} ${lines === 1 ? "line" : "lines"}` : "No input";
  const emptyButton = document.querySelector<HTMLButtonElement>(`[data-empty="${id}"]`)!;
  emptyButton.hidden = source.ready || !!source.loading;
}
function renderBaselineTabs(focus = false): void {
  const focused = document.activeElement as HTMLElement | null;
  const restore = focused && $("baseline-tabs").contains(focused)
    ? focused.id ? `#${focused.id}` : focused.hasAttribute("data-add-baseline") ? "[data-add-baseline]" : "[data-remove-baseline]"
    : undefined;
  $("baseline-tabs").innerHTML = `<div class="tab-list" role="tablist" aria-label="Baseline runs">${state.baselines.map((source, i) =>
    `<button type="button" role="tab" id="baseline-tab-${source.id}" class="tab ${i === state.active ? "is-active" : ""}" aria-label="Baseline ${i + 1}: ${esc(printable(source.name))}${source.loading ? " (loading)" : ""}" aria-selected="${i === state.active}" aria-controls="baseline-editors" tabindex="${i === state.active ? 0 : -1}" data-tab="${i}">${i + 1}${source.loading ? "…" : ""}</button>`
  ).join("")}</div><button type="button" class="tab tab-add" data-add-baseline aria-label="Add a baseline run" ${state.baselines.length >= MAX_BASELINES ? "disabled" : ""}>+</button>${state.baselines.length > 1 ? `<button type="button" class="link-button" data-remove-baseline>Remove ${state.active + 1}</button>` : ""}`;
  $("baseline-editors").setAttribute("aria-labelledby", `baseline-tab-${sourceFor("baseline").id}`);
  if (focus) $(`baseline-tab-${sourceFor("baseline").id}`).focus();
  else if (restore) $("baseline-tabs").querySelector<HTMLElement>(restore)?.focus();
}
function renderBaselineEditor(): void {
  $("baseline-editors").innerHTML = editorHtml("baseline", sourceFor("baseline"), `Baseline run ${state.active + 1}`,
    "Paste a log from a good run, or open or drop a file. Add more baselines with + to help distinguish flaky lines from changes.");
  refreshEditor("baseline");
}
function renderEditors(): void {
  renderBaselineTabs();
  renderBaselineEditor();
  $("target-editor").innerHTML = editorHtml("target", state.target, "Target run", "Paste the log from the run you're investigating, or open or drop a file.");
  refreshEditor("target");
}
function refreshSource(source: Source): void {
  if (source === state.target) refreshEditor("target");
  else if (state.baselines.includes(source)) {
    // Preserve a tab's focus if an import finishes while the user is navigating.
    const focusedTab = document.activeElement?.getAttribute("role") === "tab";
    renderBaselineTabs(focusedTab);
    if (source === sourceFor("baseline")) refreshEditor("baseline");
  }
  refreshActivity();
}
function setSource(source: Source, input: LogInput): void {
  stopFile(source);
  Object.assign(source, input);
  changed();
  refreshSource(source);
}
async function loadFile(id: Editor, file: File): Promise<void> {
  const source = sourceFor(id);
  stopExample();
  stopComparison();
  stopFile(source);
  const controller = new AbortController();
  source.loading = { controller, name: file.name };
  status(`Opening ${printable(file.name)}…`);
  refreshSource(source);
  try {
    const text = await readLogFile(file, controller.signal);
    if (controller.signal.aborted || source.loading?.controller !== controller) return;
    setSource(source, { text, name: file.name, origin: "file", ready: true });
    status(`Opened ${printable(file.name)}. Compare runs to update the findings.`);
  } catch (error) {
    if (controller.signal.aborted || source.loading?.controller !== controller) return;
    source.loading = undefined;
    status(`${printable(file.name)}: ${errorText(error)} Previous input kept.`, true);
  } finally {
    if (source.loading?.controller === controller) source.loading = undefined;
    refreshSource(source);
  }
}
function selectBaseline(index: number): void {
  state.active = index;
  renderBaselineTabs(true);
  renderBaselineEditor();
}
function wireEditors(): void {
  const form = $<HTMLFormElement>("inputs");
  form.addEventListener("input", (event) => {
    const target = event.target as HTMLElement;
    if (target instanceof HTMLTextAreaElement && (target.id === "baseline" || target.id === "target")) {
      const source = sourceFor(target.id);
      const name = source.origin === "file" || source.origin === "example" ? `${source.name} (edited)` : source.origin === "edited" ? source.name : "Pasted log";
      const origin = source.origin === "file" || source.origin === "example" || source.origin === "edited" ? "edited" : "pasted";
      setSource(source, { text: target.value, name, origin, ready: true });
    } else if (target.id === "masks" || target.id === "context") {
      target.removeAttribute("aria-invalid");
      changed();
    }
  });
  form.addEventListener("change", (event) => {
    const input = event.target as HTMLInputElement;
    const id = input.dataset["fileFor"] as Editor | undefined;
    const file = input.files?.[0];
    if (!id || !file) return;
    input.value = "";
    void loadFile(id, file);
  });
  form.addEventListener("click", (event) => {
    const el = (event.target as HTMLElement).closest<HTMLElement>("button");
    if (!el) return;
    const id = (el.dataset["clear"] ?? el.dataset["empty"]) as Editor | undefined;
    if (id) {
      const empty = !!el.dataset["empty"];
      setSource(sourceFor(id), { text: "", name: empty ? "Empty log" : "No log loaded", origin: "empty", ready: empty });
      $(id).focus();
    } else if (el.dataset["tab"] !== undefined) selectBaseline(Number(el.dataset["tab"]));
    else if (el.hasAttribute("data-add-baseline") && state.baselines.length < MAX_BASELINES) {
      changed();
      state.baselines.push(newSource());
      state.active = state.baselines.length - 1;
      renderBaselineTabs();
      renderBaselineEditor();
      $("baseline").focus();
    } else if (el.hasAttribute("data-remove-baseline") && state.baselines.length > 1) {
      stopFile(sourceFor("baseline"));
      state.baselines.splice(state.active, 1);
      state.active = Math.max(0, state.active - 1);
      changed();
      selectBaseline(state.active);
    }
  });
  $("baseline-tabs").addEventListener("keydown", (event) => {
    if ((event.target as HTMLElement).getAttribute("role") !== "tab") return;
    const n = state.baselines.length;
    const index = event.key === "ArrowRight" ? (state.active + 1) % n : event.key === "ArrowLeft" ? (state.active + n - 1) % n : event.key === "Home" ? 0 : event.key === "End" ? n - 1 : undefined;
    if (index !== undefined) { event.preventDefault(); selectBaseline(index); }
  });
  for (const zone of ["baseline-editors", "target-editor"]) {
    const el = $(zone);
    el.addEventListener("dragover", (event) => { event.preventDefault(); el.classList.add("is-drop"); });
    el.addEventListener("dragleave", () => el.classList.remove("is-drop"));
    el.addEventListener("drop", (event) => {
      event.preventDefault();
      el.classList.remove("is-drop");
      const files = event.dataTransfer?.files;
      if (files && files.length > 1) { status("Open one file per editor. Add a baseline with + for another good run.", true); return; }
      if (files?.[0]) void loadFile(zone === "target-editor" ? "target" : "baseline", files[0]);
    });
  }
  form.addEventListener("submit", (event) => { event.preventDefault(); void compare(); });
  $("cancel").addEventListener("click", () => {
    stopExample();
    stopComparison();
    sources().forEach(stopFile);
    refreshSource(sourceFor("baseline"));
    refreshSource(state.target);
    status("Cancelled. Loaded logs and the last completed comparison are kept.");
    $("run").focus();
  });
  $("new-comparison").addEventListener("click", () => {
    stopExample();
    stopComparison();
    sources().forEach(stopFile);
    engine.cancel(); // Also release an idle WASM instance and its retained memory.
    state.baselines = [newSource()];
    state.target = newSource();
    state.active = 0;
    state.report = undefined;
    state.outcomeLines = [];
    state.expanded.clear();
    state.details.clear();
    state.sourceDetails = false;
    state.page = 0;
    state.revision++;
    $<HTMLInputElement>("context").value = "2";
    $("context").removeAttribute("aria-invalid");
    $<HTMLTextAreaElement>("masks").value = "";
    clearExampleSelection();
    renderEditors();
    renderResults();
    refreshActivity();
    status("New comparison. Open files, paste logs, or load an example.");
    $("baseline").focus();
  });
}

// ---------------------------------------------------------------------------
// examples
// ---------------------------------------------------------------------------

function renderExamples(): void {
  $("example-buttons").innerHTML = EXAMPLES.map((e) => `<button type="button" class="chip" aria-pressed="false" data-example="${e.id}">${esc(e.label)}</button>`).join("");
  $("example-buttons").addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-example]");
    const example = EXAMPLES.find((e) => e.id === button?.dataset["example"]);
    if (example) void useExample(example);
  });
}
async function useExample(example: Example): Promise<void> {
  stopExample();
  stopComparison();
  sources().forEach(stopFile);
  refreshSource(sourceFor("baseline"));
  refreshSource(state.target);
  const controller = new AbortController();
  state.example = controller;
  $("example-note").textContent = `Loading ${example.label}…`;
  status("Loading example logs…");
  refreshActivity();
  try {
    const { baselines, target } = await loadExample(example, controller.signal);
    if (controller.signal.aborted || state.example !== controller) return;
    state.example = undefined;
    state.baselines = baselines.map((text, i) => newSource({ text, name: example.baselines[i]!, origin: "example", ready: true }));
    state.active = 0;
    state.target = newSource({ text: target, name: example.target, origin: "example", ready: true });
    state.revision++;
    renderEditors();
    $("example-note").textContent = example.note;
    for (const b of document.querySelectorAll<HTMLButtonElement>("[data-example]")) b.setAttribute("aria-pressed", String(b.dataset["example"] === example.id));
    await compare();
  } catch (error) {
    if (controller.signal.aborted || state.example !== controller) return;
    controller.abort(); // Stop sibling requests if one example file fails.
    state.example = undefined;
    $("example-note").textContent = "Example not loaded. Choose it again to retry; your loaded logs are kept.";
    status(`Couldn't load the example: ${errorText(error)}`, true);
  } finally {
    if (state.example === controller) state.example = undefined;
    refreshActivity();
  }
}

// ---------------------------------------------------------------------------
// compare + results
// ---------------------------------------------------------------------------

async function compare(): Promise<void> {
  if (state.example || sources().some((s) => s.loading)) { status("Wait for the logs to finish loading, or cancel the import."); return; }
  stopComparison();
  const context = $<HTMLInputElement>("context");
  let input;
  try {
    input = comparisonInput(state.baselines, state.target, context.value, $<HTMLTextAreaElement>("masks").value);
  } catch (error) {
    status(errorText(error), true);
    if (context.value.trim() === "" || !context.validity.valid) {
      context.setAttribute("aria-invalid", "true");
      context.closest("details")!.open = true;
      context.focus();
    }
    refreshActivity();
    return;
  }
  const controller = new AbortController();
  const revision = state.revision;
  state.comparison = controller;
  status("Comparing logs locally. You can cancel or keep editing.");
  refreshActivity();
  try {
    const outcome = await engine.run(input.request, controller.signal);
    if (controller.signal.aborted || state.comparison !== controller || state.revision !== revision) return;
    state.report = { ...input, outcome, revision, completedAt: new Date().toISOString() };
    // str::lines: a final newline does not start a line. Always use the captured target.
    state.outcomeLines = input.request.target === "" ? [] : input.request.target.replace(/\r?\n$/, "").split(/\r?\n/);
    state.filter = "all";
    state.expanded.clear();
    state.details.clear();
    state.sourceDetails = false;
    state.page = 0;
    renderResults();
    status(`Comparison complete: ${formatCount(outcome.result.findings.length + outcome.result.value_findings.length)} findings before grouping.`);
  } catch (error) {
    if (!controller.signal.aborted && state.comparison === controller) status(`Comparison failed: ${errorText(error)} Correct the inputs or try comparing again.`, true);
  } finally {
    if (state.comparison === controller) state.comparison = undefined;
    refreshActivity();
  }
}

interface Row {
  filter: Exclude<Filter, "all">;
  render: () => string;
}

/** Rows in the order `at` gives (a line number), blocks and single findings together. */
function inLineOrder(rows: { at: number; row: Row }[]): Row[] {
  return rows.sort((a, b) => a.at - b.at).map((r) => r.row);
}

/** Order: new (by target line), new values, changed, gone (by line in the first good run). */
function rows(result: DiffResult): Row[] {
  const single = (f: Finding): Row => ({ filter: f.kind, render: () => findingHtml(f, result) });
  const alone = (kind: Finding["kind"]) => result.findings.filter((f) => f.kind === kind && f.block === undefined);
  const blocks = (kind: Block["kind"]) =>
    result.blocks.flatMap((block, index) => (block.kind === kind ? [{ at: block.first_line_no, row: { filter: kind, render: () => blockHtml(block, index, result) } as Row }] : []));
  return [
    ...inLineOrder([...blocks("new"), ...alone("new").map((f) => ({ at: f.first_target_line_no ?? Infinity, row: single(f) }))]),
    // A new value on a line inside a block is shown as part of that block.
    ...result.value_findings.filter((v) => v.block === undefined).map((v): Row => ({ filter: "value", render: () => valueFindingHtml(v) })),
    ...alone("changed").map(single),
    ...inLineOrder([...blocks("gone"), ...alone("gone").map((f) => ({ at: f.first_baseline_line_no ?? Infinity, row: single(f) }))]),
  ];
}

function download(data: unknown, filename: string): void {
  let url: string | undefined;
  try {
    url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
    const anchor = Object.assign(document.createElement("a"), { href: url, download: filename });
    document.body.append(anchor);
    try { anchor.click(); } finally { anchor.remove(); }
    status(`Prepared ${filename} for download.`);
  } catch (error) {
    status(`Couldn't download the report: ${errorText(error)} Try again.`, true);
  } finally {
    // Revoking in the same task can cancel downloads in some browsers.
    if (url) { const ownedUrl = url; setTimeout(() => URL.revokeObjectURL(ownedUrl), 1000); }
  }
}

function renderResults(): void {
  const report = state.report;
  if (!report) {
    $("report-content").innerHTML = '<p class="notice">Load logs and compare to see what changed. Nothing is uploaded or saved by this page.</p>';
    return;
  }
  const { result, ms } = report.outcome;
  const all = rows(result);
  const count = (f: Filter) => (f === "all" ? all : all.filter((r) => r.filter === f)).length;
  const total = count("all");
  const ungrouped = result.findings.length + result.value_findings.length;
  const baselineLines = result.baseline_totals.reduce((a, b) => a + b, 0);
  const filters: [Filter, string][] = [["all", "All"], ["new", "New"], ["gone", "Gone"], ["changed", "Changed"], ["value", "New value"]];
  const visible = state.filter === "all" ? all : all.filter((r) => r.filter === state.filter);
  const pageSize = 50;
  state.page = Math.min(state.page, Math.max(0, Math.ceil(visible.length / pageSize) - 1));
  const start = state.page * pageSize;
  const end = Math.min(start + pageSize, visible.length);
  const runs = result.baseline_totals.length === 1 ? "1 good run" : `${result.baseline_totals.length} good runs`;

  $("report-content").innerHTML = `
    <div class="summary">
      <p class="summary-line">
        <span class="figure">${formatCount(baselineLines)}</span> baseline lines (${runs})
        <span class="arrow" aria-hidden="true">→</span>
        <span class="figure">${formatCount(result.target_total)}</span> target lines ·
        <span class="figure">${formatCount(result.total_templates)}</span> templates ·
        <span class="figure strong">${formatCount(total)}</span> ${total === 1 ? "finding" : "findings"}${result.blocks.length > 0 ? ` <span class="ungrouped">(${formatCount(ungrouped)} before grouping)</span>` : ""}
      </p>
      <p class="summary-meta">Compared in ${ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`} in this tab. <button type="button" class="link-button" id="download-report">Download report</button> <button type="button" class="link-button" id="download-json">Download JSON</button></p>
    </div>
    <details class="provenance" id="source-details" ${state.sourceDetails ? "open" : ""}>
      <summary>Compared sources and settings</summary>
      <table><caption>Baseline counts in the findings follow this order.</caption><thead><tr><th scope="col">Input</th><th scope="col">Source</th><th scope="col">Lines</th></tr></thead>
        <tbody>${[...report.sources.baselines, report.sources.target].map((source) => `<tr><th scope="row">${esc(source.label)}</th><td>${esc(printable(source.name))}</td><td>${formatCount(source.lines)}</td></tr>`).join("")}</tbody>
      </table>
      ${report.sources.omitted_baselines.length ? `<p>Unused baseline editors omitted: ${report.sources.omitted_baselines.join(", ")}. Empty imported logs are included.</p>` : ""}
      <p>Context: ${report.request.context} lines. Extra masks: ${report.request.masks.length}.</p>
      ${report.request.masks.length ? `<pre>${esc(report.request.masks.map(printable).join("\n"))}</pre>` : ""}
      <p>The report includes these source names, settings and all findings. Both downloads include original log excerpts; full input logs are not bundled.</p>
    </details>
    ${total === 0
      ? `<p class="notice">No findings under these settings. Only template, frequency and established-value changes are reported; this does not prove the logs are identical.</p>`
      : `<div class="filters" role="group" aria-label="Show findings">${filters.filter(([f]) => f === "all" || count(f) > 0).map(([f, label]) => `<button type="button" class="filter filter-${f}" aria-pressed="${state.filter === f}" data-filter="${f}">${label} <span>${count(f)}</span></button>`).join("")}</div>
        ${visible.length > pageSize ? `<nav class="finding-pages" aria-label="Finding pages"><span id="findings-page" tabindex="-1">Showing ${start + 1}–${end} of ${formatCount(visible.length)} findings</span><button type="button" class="link-button" data-page="previous" ${state.page === 0 ? "disabled" : ""}>Previous</button><button type="button" class="link-button" data-page="next" ${end === visible.length ? "disabled" : ""}>Next</button></nav>` : ""}
        <ol class="printout" start="${start + 1}">${visible.slice(start, end).map((r) => r.render()).join("")}</ol>`}`;

  $("report-content").querySelector<HTMLElement>(".filters")?.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-filter]");
    if (!button) return;
    state.filter = button.dataset["filter"] as Filter;
    state.page = 0;
    renderResults();
    document.querySelector<HTMLButtonElement>(`[data-filter="${state.filter}"]`)?.focus();
  });
  document.querySelectorAll<HTMLButtonElement>("[data-page]").forEach((button) => button.addEventListener("click", () => {
    state.page += button.dataset["page"] === "next" ? 1 : -1;
    renderResults();
    $("findings-page").focus();
  }));
  $("report-content").querySelector<HTMLElement>(".printout")?.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-toggle-block]");
    if (!button) return;
    const index = Number(button.dataset["toggleBlock"]);
    if (state.expanded.has(index)) state.expanded.delete(index);
    else state.expanded.add(index);
    renderResults();
    document.querySelector<HTMLButtonElement>(`[data-toggle-block="${index}"]`)?.focus();
  });
  $("download-json").addEventListener("click", () => download(result, "logdelta-diff.json"));
  $("download-report").addEventListener("click", () => download(exportReport(report), "logdelta-report.json"));
}

function templateHtml(template: string, emphasizeToken?: number): string {
  return templateParts(printable(template))
    .map((part) =>
      part.kind === "text"
        ? esc(part.text)
        : `<span class="slot ${part.token === emphasizeToken ? "slot-flipped" : ""}" title="masked: ${esc(part.name)}">${esc(part.text)}</span>`,
    )
    .join("");
}

function logLine(no: number, text: string, cls: string, times = 1): string {
  const repeat = times > 1 ? `<span class="times">×${formatCount(times)}</span>` : "";
  return `<div class="log-line ${cls}"><span class="gutter">${formatCount(no)}</span><span class="log-text">${esc(printable(text))}</span>${repeat}</div>`;
}

function contextHtml(lineNo: number | null, raw: string | null, context: ContextWindow | undefined): string {
  if (lineNo === null || raw === null) return "";
  return `<div class="log" tabindex="0" role="group" aria-label="Log lines">${(context?.before ?? []).map(([n, t]) => logLine(n, t, "is-context")).join("")}${logLine(lineNo, raw, "is-hit")}${(context?.after ?? []).map(([n, t]) => logLine(n, t, "is-context")).join("")}</div>`;
}

const KIND_LABEL = { new: "New", gone: "Gone", changed: "Changed" } as const;

// How much of a block the page shows before the reader asks for the rest, and the most it
// will put on the page at once (a block can be most of a large log).
const BLOCK_PREVIEW_LINES = 36;
const BLOCK_PREVIEW_TEMPLATES = 12;
const BLOCK_MAX_LINES = 2000;

/** The rows of a block, cut to its start and end unless the reader expanded it. */
function clipped(total: number, limit: number, expanded: boolean, index: number, unit: string, row: (i: number) => string): string {
  const shown = expanded ? Math.min(total, BLOCK_MAX_LINES) : total;
  const { head, tail } = expanded ? { head: shown, tail: 0 } : headAndTail(total, limit);
  const hidden = total - head - tail;
  const out: string[] = [];
  for (let i = 0; i < head; i++) out.push(row(i));
  if (hidden > 0) {
    const label = expanded ? `${formatCount(hidden)} more ${unit} not shown` : `${formatCount(hidden)} more ${unit}`;
    const button = expanded ? "" : `<button type="button" class="link-button" data-toggle-block="${index}" aria-expanded="false">${total > BLOCK_MAX_LINES ? `Show first ${formatCount(BLOCK_MAX_LINES)}` : `Show all ${formatCount(total)}`}</button>`;
    out.push(`<div class="log-gap"><span class="gutter" aria-hidden="true">⋯</span><span>${label}</span>${button}</div>`);
  }
  for (let i = total - tail; i < total; i++) out.push(row(i));
  if (expanded && total > limit) {
    out.push(`<div class="log-gap"><span class="gutter" aria-hidden="true"></span><button type="button" class="link-button" data-toggle-block="${index}" aria-expanded="true">Show less</button></div>`);
  }
  return out.join("");
}

/**
 * A block: what it is, its templates on request, and its lines. A new block shows the target
 * as it reads from the block's first line to its last; a gone block shows one line of the
 * first good run per template, because the lines between them are still in the target.
 */
function blockHtml(block: Block, index: number, result: DiffResult): string {
  const members = block.findings.map((i) => result.findings[i]).filter((f): f is Finding => f !== undefined);
  const expanded = state.expanded.has(index);
  const runs = result.baseline_totals.length === 1 ? "run" : "runs";
  const isNew = block.kind === "new";
  const lineNo = (f: Finding) => (isNew ? f.first_target_line_no : f.first_baseline_line_no) ?? 0;
  const times = (f: Finding) => (isNew ? f.target_count : (f.baseline_counts[0] ?? 0));

  let log: string;
  if (isNew) {
    const lines = state.outcomeLines;
    const from = Math.max(1, block.first_line_no - state.report!.request.context);
    const to = Math.min(lines.length, block.last_line_no + state.report!.request.context);
    // The first line of each new template stands out; the lines between them are the same
    // templates again, or the few known lines the block reaches across.
    const firsts = new Set(members.map(lineNo));
    const cls = (no: number) => (firsts.has(no) ? "is-hit" : no >= block.first_line_no && no <= block.last_line_no ? "is-within" : "is-context");
    log = clipped(to - from + 1, BLOCK_PREVIEW_LINES, expanded, index, "lines", (i) => logLine(from + i, lines[from + i - 1] ?? "", cls(from + i)));
  } else {
    log = clipped(members.length, BLOCK_PREVIEW_TEMPLATES, expanded, index, "templates", (i) => {
      const f = members[i]!;
      return logLine(lineNo(f), f.first_baseline_raw ?? f.template, "is-hit", times(f));
    });
  }

  const elsewhere = block.lines_elsewhere > 0 ? `, and ${formatCount(block.lines_elsewhere)} more of these lines further on` : "";
  const what = isNew
    ? `${formatCount(members.length)} templates on ${formatCount(block.line_count)} lines, never in the good ${runs}${elsewhere}`
    : `${formatCount(members.length)} templates on ${formatCount(block.line_count)} lines of the ${result.baseline_totals.length === 1 ? "good run" : "first good run"}, missing from the target`;
  const where = `lines ${formatCount(block.first_line_no)}–${formatCount(block.last_line_no)}${isNew ? "" : result.baseline_totals.length === 1 ? " of the good run" : " of good run 1"}`;
  return `
    <li class="finding kind-${block.kind} is-block">
      <div class="finding-head">
        <span class="kind">${KIND_LABEL[block.kind]}</span>
        <span class="counts">${what}</span>
        <span class="where">${where}</span>
      </div>
      <details class="block-details" data-block-details="${index}" ${state.details.has(index) ? "open" : ""}>
        <summary>The ${formatCount(members.length)} templates</summary>
        <ol class="block-templates">${members
          .slice(0, state.details.has(index) ? BLOCK_MAX_LINES : BLOCK_PREVIEW_TEMPLATES)
          .map((f) => `<li><span class="gutter">${formatCount(lineNo(f))}</span><span class="template">${templateHtml(f.template)}</span>${times(f) > 1 ? `<span class="times">×${formatCount(times(f))}</span>` : ""}</li>`)
          .join("")}</ol>${members.length > BLOCK_MAX_LINES ? `<p>Showing the first ${formatCount(BLOCK_MAX_LINES)} templates. Downloads contain every finding.</p>` : ""}
      </details>
      <div class="log" tabindex="0" role="group" aria-label="Log lines">${log}</div>
    </li>`;
}

function findingHtml(f: Finding, result: DiffResult): string {
  let what: string;
  if (f.kind === "new") what = `${formatCount(f.target_count)} in the target, never in the good ${result.baseline_totals.length === 1 ? "run" : "runs"}`;
  else if (f.kind === "gone") what = `${baselineCounts(f.baseline_counts)}, missing from the target`;
  else what = `${baselineCounts(f.baseline_counts)} → ${formatCount(f.target_count)} in the target (${f.direction === "up" ? "more often" : f.direction === "down" ? "less often" : "same rate"})`;
  return `
    <li class="finding kind-${f.kind}">
      <div class="finding-head">
        <span class="kind">${KIND_LABEL[f.kind]}</span>
        <span class="counts">${what}</span>
        ${
          f.first_target_line_no !== null
            ? `<span class="where">first at line ${formatCount(f.first_target_line_no)}</span>`
            : f.first_baseline_line_no !== undefined
              ? `<span class="where">line ${formatCount(f.first_baseline_line_no)} of ${result.baseline_totals.length === 1 ? "the good run" : "good run 1"}</span>`
              : ""
        }
      </div>
      <p class="template">${templateHtml(f.template)}</p>
      ${f.first_target_line_no !== null ? contextHtml(f.first_target_line_no, f.first_target_raw, f.context) : contextHtml(f.first_baseline_line_no ?? null, f.first_baseline_raw ?? null, undefined)}
    </li>`;
}

function valueFindingHtml(v: ValueFinding): string {
  const seen = v.baseline_values.map((value) => `<code>${esc(printable(value))}</code>`).join(", ");
  return `
    <li class="finding kind-value">
      <div class="finding-head">
        <span class="kind">New value</span>
        <span class="counts"><code class="new-value">${esc(printable(v.new_value))}</code> where the good runs only had ${seen}</span>
        <span class="where">first at line ${formatCount(v.first_target_line_no)}</span>
      </div>
      <p class="template">${templateHtml(v.template, v.position)}</p>
      ${contextHtml(v.first_target_line_no, v.first_target_raw, v.context)}
    </li>`;
}

// ---------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------

initTheme();
renderExamples();
renderEditors();
wireEditors();
$("report-content").addEventListener("toggle", (event) => {
  const details = event.target as HTMLDetailsElement;
  if (details.id === "source-details") state.sourceDetails = details.open;
  if (details.dataset["blockDetails"] !== undefined) {
    const index = Number(details.dataset["blockDetails"]);
    if (state.details.has(index) === details.open) return;
    if (details.open) state.details.add(index);
    else state.details.delete(index);
    renderResults();
    document.querySelector<HTMLElement>(`[data-block-details="${index}"] summary`)?.focus();
  }
}, true);
renderResults();
void useExample(EXAMPLES[0]!);
