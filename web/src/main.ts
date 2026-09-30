import "./style.css";
import { runDiff, type DiffOutcome } from "./engine";
import { EXAMPLES, loadExample, type Example } from "./examples";
import { baselineCounts, formatCount, lineCount, templateParts } from "./template";
import type { ContextWindow, DiffResult, Finding, ValueFinding } from "./types";

type Filter = "all" | "new" | "gone" | "changed" | "value";

const state = {
  baselines: [""],
  active: 0,
  target: "",
  filter: "all" as Filter,
  context: 2,
  outcome: undefined as DiffOutcome | undefined,
  /** The target text the current results were computed from, split into lines (line n = index n-1). */
  outcomeLines: [] as string[],
  running: false,
};

const $ = <T extends HTMLElement = HTMLElement>(id: string): T => document.getElementById(id) as T;

function esc(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
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
  try {
    stored = localStorage.getItem("logdelta-theme");
  } catch {
    // storage blocked: follow the system preference
  }
  applyTheme(stored === "light" || stored === "dark" ? stored : undefined);
  $("theme-toggle").addEventListener("click", () => {
    const next = $("theme-toggle").textContent === "Dark" ? "dark" : "light";
    applyTheme(next);
    try {
      localStorage.setItem("logdelta-theme", next);
    } catch {
      // not persisted; fine
    }
  });
}

// ---------------------------------------------------------------------------
// editors
// ---------------------------------------------------------------------------

async function readLogFile(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  // The CLI reads .gz transparently; so does the page (gzip magic bytes, whatever the name).
  if (bytes[0] === 0x1f && bytes[1] === 0x8b) {
    const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
    return new Response(stream).text();
  }
  return new TextDecoder().decode(bytes);
}

function editorHtml(id: string, value: string, label: string, placeholder: string): string {
  return `
    <div class="editor" data-editor="${id}">
      <textarea id="${id}" spellcheck="false" wrap="off" aria-label="${esc(label)}" placeholder="${esc(placeholder)}">${esc(value)}</textarea>
      <div class="editor-bar">
        <span class="editor-count" id="${id}-count">${formatCount(lineCount(value))} lines</span>
        <label class="link-button">Open file…<input type="file" data-file-for="${id}" accept=".log,.txt,.gz,text/plain" class="visually-hidden" /></label>
        <button type="button" class="link-button" data-clear="${id}">Clear</button>
      </div>
    </div>`;
}

function renderBaselineTabs(): void {
  const tabs = state.baselines
    .map(
      (_, i) =>
        `<button type="button" role="tab" class="tab ${i === state.active ? "is-active" : ""}" aria-selected="${i === state.active}" data-tab="${i}">${i + 1}</button>`,
    )
    .join("");
  const remove = state.baselines.length > 1 ? `<button type="button" class="link-button" data-remove-baseline>Remove ${state.active + 1}</button>` : "";
  $("baseline-tabs").innerHTML = `${tabs}<button type="button" class="tab tab-add" data-add-baseline aria-label="Add a baseline run">+</button>${remove}`;
}

function renderBaselineEditor(): void {
  $("baseline-editors").innerHTML = editorHtml(
    "baseline",
    state.baselines[state.active] ?? "",
    `Baseline run ${state.active + 1}`,
    "Paste a log from a run that went fine, or drop a file here. More baselines (the + tab) let logdelta tell flaky lines from real changes.",
  );
}

function renderEditors(): void {
  renderBaselineTabs();
  renderBaselineEditor();
  $("target-editor").innerHTML = editorHtml("target", state.target, "Target run", "Paste the log from the run you're investigating, or drop a file here.");
}

function setEditorText(id: "baseline" | "target", text: string): void {
  if (id === "baseline") state.baselines[state.active] = text;
  else state.target = text;
  const area = $<HTMLTextAreaElement>(id);
  if (area.value !== text) area.value = text;
  $(`${id}-count`).textContent = `${formatCount(lineCount(text))} lines`;
}

function wireEditors(): void {
  const form = $<HTMLFormElement>("inputs");
  form.addEventListener("input", (event) => {
    const target = event.target as HTMLElement;
    if (target instanceof HTMLTextAreaElement && (target.id === "baseline" || target.id === "target")) setEditorText(target.id, target.value);
  });
  form.addEventListener("change", async (event) => {
    const input = event.target as HTMLInputElement;
    const id = input.dataset["fileFor"] as "baseline" | "target" | undefined;
    const file = input.files?.[0];
    if (!id || !file) return;
    input.value = "";
    setEditorText(id, await readLogFile(file));
  });
  form.addEventListener("click", (event) => {
    const el = (event.target as HTMLElement).closest<HTMLElement>("button");
    if (!el) return;
    if (el.dataset["clear"]) setEditorText(el.dataset["clear"] as "baseline" | "target", "");
    else if (el.dataset["tab"] !== undefined) {
      state.active = Number(el.dataset["tab"]);
      renderBaselineTabs();
      renderBaselineEditor();
    } else if (el.hasAttribute("data-add-baseline")) {
      state.baselines.push("");
      state.active = state.baselines.length - 1;
      renderBaselineTabs();
      renderBaselineEditor();
      $("baseline").focus();
    } else if (el.hasAttribute("data-remove-baseline")) {
      state.baselines.splice(state.active, 1);
      state.active = Math.max(0, state.active - 1);
      renderBaselineTabs();
      renderBaselineEditor();
    }
  });
  for (const zone of ["baseline-editors", "target-editor"]) {
    const el = $(zone);
    el.addEventListener("dragover", (event) => {
      event.preventDefault();
      el.classList.add("is-drop");
    });
    el.addEventListener("dragleave", () => el.classList.remove("is-drop"));
    el.addEventListener("drop", async (event) => {
      event.preventDefault();
      el.classList.remove("is-drop");
      const file = event.dataTransfer?.files?.[0];
      if (file) setEditorText(zone === "target-editor" ? "target" : "baseline", await readLogFile(file));
    });
  }
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void compare();
  });
}

// ---------------------------------------------------------------------------
// examples
// ---------------------------------------------------------------------------

function renderExamples(): void {
  $("example-buttons").innerHTML = EXAMPLES.map((e) => `<button type="button" class="chip" data-example="${e.id}">${esc(e.label)}</button>`).join("");
  $("example-buttons").addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-example]");
    const example = EXAMPLES.find((e) => e.id === button?.dataset["example"]);
    if (example) void useExample(example);
  });
}

async function useExample(example: Example): Promise<void> {
  const note = $("example-note");
  note.textContent = "Loading the example…";
  try {
    const { baselines, target } = await loadExample(example);
    state.baselines = baselines;
    state.active = 0;
    state.target = target;
    renderEditors();
    note.textContent = example.note;
    for (const b of document.querySelectorAll<HTMLButtonElement>("[data-example]")) b.setAttribute("aria-pressed", String(b.dataset["example"] === example.id));
    await compare();
  } catch (err) {
    note.textContent = `Couldn't load the example: ${err instanceof Error ? err.message : String(err)}`;
  }
}

// ---------------------------------------------------------------------------
// compare + results
// ---------------------------------------------------------------------------

async function compare(): Promise<void> {
  const baselines = state.baselines.filter((b) => b.trim().length > 0);
  const results = $("results");
  if (baselines.length === 0 || state.target.trim().length === 0) {
    results.innerHTML = `<p class="notice">Add at least one good run and the run to investigate, then compare.</p>`;
    return;
  }
  const masks = $<HTMLTextAreaElement>("masks").value.split("\n").filter((l) => l.trim().length > 0);
  const context = Math.max(0, Math.min(10, Number($<HTMLInputElement>("context").value) || 0));
  state.running = true;
  $<HTMLButtonElement>("run").disabled = true;
  results.setAttribute("aria-busy", "true");
  results.classList.add("is-running");
  try {
    state.outcome = await runDiff({ baselines, target: state.target, context, masks });
    // Same line numbering as the engine (str::lines): a trailing newline doesn't start a line.
    state.outcomeLines = state.target.replace(/\r?\n$/, "").split(/\r?\n/);
    state.context = context;
    state.filter = "all";
    renderResults();
  } catch (err) {
    results.innerHTML = `<p class="notice is-error">${esc(err instanceof Error ? err.message : String(err))}</p>`;
  } finally {
    state.running = false;
    $<HTMLButtonElement>("run").disabled = false;
    results.setAttribute("aria-busy", "false");
    results.classList.remove("is-running");
  }
}

interface Row {
  filter: Exclude<Filter, "all">;
  /** How many findings the row stands for (a block of consecutive new lines is one row). */
  weight: number;
  html: string;
}

// New lines this close together (a traceback, a panic) read as one event, so they share a card.
const BLOCK_GAP = 2;

/** NEW findings in line order, runs of nearby lines grouped. */
function newBlocks(findings: Finding[]): Finding[][] {
  const located = findings.filter((f) => f.kind === "new" && f.first_target_line_no !== null).sort((a, b) => a.first_target_line_no! - b.first_target_line_no!);
  const blocks: Finding[][] = [];
  for (const f of located) {
    const block = blocks[blocks.length - 1];
    const last = block?.[block.length - 1];
    if (block && last && f.first_target_line_no! - last.first_target_line_no! <= BLOCK_GAP) block.push(f);
    else blocks.push([f]);
  }
  return blocks;
}

/** Order: new (by line, blocks grouped), new values, changed, gone. */
function rows(result: DiffResult): Row[] {
  const blocks = newBlocks(result.findings);
  const unlocatedNew = result.findings.filter((f) => f.kind === "new" && f.first_target_line_no === null);
  const single = (f: Finding): Row => ({ filter: f.kind, weight: 1, html: findingHtml(f, result) });
  return [
    ...blocks.map((block): Row => (block.length === 1 ? single(block[0]!) : { filter: "new", weight: block.length, html: blockHtml(block, result) })),
    ...unlocatedNew.map(single),
    ...result.value_findings.map((v): Row => ({ filter: "value", weight: 1, html: valueFindingHtml(v) })),
    ...result.findings.filter((f) => f.kind === "changed").map(single),
    ...result.findings.filter((f) => f.kind === "gone").map(single),
  ];
}

function renderResults(): void {
  const outcome = state.outcome;
  if (!outcome) return;
  const { result, ms } = outcome;
  const all = rows(result);
  const count = (f: Filter) => (f === "all" ? all : all.filter((r) => r.filter === f)).reduce((n, r) => n + r.weight, 0);
  const total = count("all");
  const baselineLines = result.baseline_totals.reduce((a, b) => a + b, 0);
  const filters: [Filter, string][] = [
    ["all", "All"],
    ["new", "New"],
    ["gone", "Gone"],
    ["changed", "Changed"],
    ["value", "New value"],
  ];
  const visible = state.filter === "all" ? all : all.filter((r) => r.filter === state.filter);
  const runs = result.baseline_totals.length === 1 ? "1 good run" : `${result.baseline_totals.length} good runs`;

  $("results").innerHTML = `
    <div class="summary">
      <p class="summary-line">
        <span class="figure">${formatCount(baselineLines)}</span> baseline lines (${runs})
        <span class="arrow" aria-hidden="true">→</span>
        <span class="figure">${formatCount(result.target_total)}</span> target lines ·
        <span class="figure">${formatCount(result.total_templates)}</span> templates ·
        <span class="figure strong">${formatCount(total)}</span> ${total === 1 ? "finding" : "findings"}
      </p>
      <p class="summary-meta">Compared in ${ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`} in this tab. <button type="button" class="link-button" id="download-json">Download JSON</button></p>
    </div>
    ${
      total === 0
        ? `<p class="notice">No differences: every template in the target appears in the good runs at a similar rate, with values they've seen before.</p>`
        : `<div class="filters" role="group" aria-label="Show findings">${filters
            .filter(([f]) => f === "all" || count(f) > 0)
            .map(([f, label]) => `<button type="button" class="filter filter-${f}" aria-pressed="${state.filter === f}" data-filter="${f}">${label} <span>${count(f)}</span></button>`)
            .join("")}</div>
          <ol class="printout">${visible.map((r) => r.html).join("")}</ol>`
    }`;

  $("results").querySelector<HTMLElement>(".filters")?.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-filter]");
    if (!button) return;
    state.filter = button.dataset["filter"] as Filter;
    renderResults();
  });
  $("download-json").addEventListener("click", () => {
    const blob = new Blob([JSON.stringify(result, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = Object.assign(document.createElement("a"), { href: url, download: "logdelta-diff.json" });
    a.click();
    // Revoking in the same task can cancel the download in some browsers.
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
}

function templateHtml(template: string, emphasizeToken?: number): string {
  return templateParts(template)
    .map((part) =>
      part.kind === "text"
        ? esc(part.text)
        : `<span class="slot ${part.token === emphasizeToken ? "slot-flipped" : ""}" title="masked: ${esc(part.name)}">${esc(part.text)}</span>`,
    )
    .join("");
}

function contextHtml(lineNo: number | null, raw: string | null, context: ContextWindow | undefined): string {
  if (lineNo === null || raw === null) return "";
  const line = (no: number, text: string, cls: string) => `<div class="log-line ${cls}"><span class="gutter">${formatCount(no)}</span><span class="log-text">${esc(text)}</span></div>`;
  return `<div class="log">${(context?.before ?? []).map(([n, t]) => line(n, t, "is-context")).join("")}${line(lineNo, raw, "is-hit")}${(context?.after ?? []).map(([n, t]) => line(n, t, "is-context")).join("")}</div>`;
}

const KIND_LABEL = { new: "New", gone: "Gone", changed: "Changed" } as const;

/** A run of new lines: every template, then one excerpt of the target covering the whole run. */
function blockHtml(block: Finding[], result: DiffResult): string {
  const first = block[0]!.first_target_line_no!;
  const last = block[block.length - 1]!.first_target_line_no!;
  const lines = state.outcomeLines;
  const from = Math.max(1, first - state.context);
  const to = Math.min(lines.length, last + state.context);
  const hits = new Set(block.map((f) => f.first_target_line_no!));
  const excerpt = [];
  for (let no = from; no <= to; no++) {
    const cls = hits.has(no) ? "is-hit" : "is-context";
    excerpt.push(`<div class="log-line ${cls}"><span class="gutter">${formatCount(no)}</span><span class="log-text">${esc(lines[no - 1] ?? "")}</span></div>`);
  }
  const runs = result.baseline_totals.length === 1 ? "run" : "runs";
  return `
    <li class="finding kind-new is-block">
      <div class="finding-head">
        <span class="kind">New</span>
        <span class="counts">${block.length} consecutive new templates, never in the good ${runs}</span>
        <span class="where">lines ${formatCount(first)}–${formatCount(last)}</span>
      </div>
      <details class="block-details">
        <summary>The ${block.length} templates</summary>
        <ol class="block-templates">${block
        .map((f) => `<li><span class="gutter">${formatCount(f.first_target_line_no!)}</span><span class="template">${templateHtml(f.template)}</span>${f.target_count > 1 ? `<span class="times">×${formatCount(f.target_count)}</span>` : ""}</li>`)
        .join("")}</ol>
      </details>
      <div class="log">${excerpt.join("")}</div>
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
        ${f.first_target_line_no !== null ? `<span class="where">first at line ${formatCount(f.first_target_line_no)}</span>` : ""}
      </div>
      <p class="template">${templateHtml(f.template)}</p>
      ${contextHtml(f.first_target_line_no, f.first_target_raw, f.context)}
    </li>`;
}

function valueFindingHtml(v: ValueFinding): string {
  const seen = v.baseline_values.map((value) => `<code>${esc(value)}</code>`).join(", ");
  return `
    <li class="finding kind-value">
      <div class="finding-head">
        <span class="kind">New value</span>
        <span class="counts"><code class="new-value">${esc(v.new_value)}</code> where the good runs only had ${seen}</span>
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
void useExample(EXAMPLES[0]!);
