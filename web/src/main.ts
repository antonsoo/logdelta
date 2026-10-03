import "./fonts/fonts.css";
import "./style.css";
import { decodeLog } from "./decode";
import { runDiff, type DiffOutcome } from "./engine";
import { EXAMPLES, loadExample, type Example } from "./examples";
import { baselineCounts, formatCount, headAndTail, lineCount, printable, templateParts } from "./template";
import type { Block, ContextWindow, DiffResult, Finding, ValueFinding } from "./types";

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
  /** Blocks (by index in the result) the reader asked to see in full. */
  expanded: new Set<number>(),
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
    return decodeLog(new Uint8Array(await new Response(stream).arrayBuffer()));
  }
  return decodeLog(bytes);
}

/** Loads `file` into an editor, or says in the results area why it can't. */
async function loadFile(id: "baseline" | "target", file: File): Promise<void> {
  try {
    setEditorText(id, await readLogFile(file));
  } catch (err) {
    const why = err instanceof Error ? err.message : String(err);
    $("results").innerHTML = `<p class="notice is-error">${esc(`${file.name}: ${why}`)}</p>`;
  }
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
  $("baseline-tabs").innerHTML = `<div class="tab-list" role="tablist" aria-label="Baseline runs">${tabs}</div><button type="button" class="tab tab-add" data-add-baseline aria-label="Add a baseline run">+</button>${remove}`;
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
    await loadFile(id, file);
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
      if (file) await loadFile(zone === "target-editor" ? "target" : "baseline", file);
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
    state.expanded.clear();
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
  html: string;
}

/** Rows in the order `at` gives (a line number), blocks and single findings together. */
function inLineOrder(rows: { at: number; row: Row }[]): Row[] {
  return rows.sort((a, b) => a.at - b.at).map((r) => r.row);
}

/** Order: new (by target line), new values, changed, gone (by line in the first good run). */
function rows(result: DiffResult): Row[] {
  const single = (f: Finding): Row => ({ filter: f.kind, html: findingHtml(f, result) });
  const alone = (kind: Finding["kind"]) => result.findings.filter((f) => f.kind === kind && f.block === undefined);
  const blocks = (kind: Block["kind"]) =>
    result.blocks.flatMap((block, index) => (block.kind === kind ? [{ at: block.first_line_no, row: { filter: kind, html: blockHtml(block, index, result) } as Row }] : []));
  return [
    ...inLineOrder([...blocks("new"), ...alone("new").map((f) => ({ at: f.first_target_line_no ?? Infinity, row: single(f) }))]),
    // A new value on a line inside a block is shown as part of that block.
    ...result.value_findings.filter((v) => v.block === undefined).map((v): Row => ({ filter: "value", html: valueFindingHtml(v) })),
    ...alone("changed").map(single),
    ...inLineOrder([...blocks("gone"), ...alone("gone").map((f) => ({ at: f.first_baseline_line_no ?? Infinity, row: single(f) }))]),
  ];
}

function renderResults(): void {
  const outcome = state.outcome;
  if (!outcome) return;
  const { result, ms } = outcome;
  const all = rows(result);
  const count = (f: Filter) => (f === "all" ? all : all.filter((r) => r.filter === f)).length;
  const total = count("all");
  const ungrouped = result.findings.length + result.value_findings.length;
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
        <span class="figure strong">${formatCount(total)}</span> ${total === 1 ? "finding" : "findings"}${
          result.blocks.length > 0 ? ` <span class="ungrouped">(${formatCount(ungrouped)} before grouping)</span>` : ""
        }
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
  $("results").querySelector<HTMLElement>(".printout")?.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-toggle-block]");
    if (!button) return;
    const index = Number(button.dataset["toggleBlock"]);
    if (state.expanded.has(index)) state.expanded.delete(index);
    else state.expanded.add(index);
    renderResults();
    // The button was replaced with the rest of the results; put focus back on its successor.
    $("results").querySelector<HTMLButtonElement>(`[data-toggle-block="${index}"]`)?.focus();
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
    const button = expanded ? "" : `<button type="button" class="link-button" data-toggle-block="${index}" aria-expanded="false">Show all ${formatCount(total)}</button>`;
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
    const from = Math.max(1, block.first_line_no - state.context);
    const to = Math.min(lines.length, block.last_line_no + state.context);
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
      <details class="block-details">
        <summary>The ${formatCount(members.length)} templates</summary>
        <ol class="block-templates">${members
          .slice(0, BLOCK_MAX_LINES)
          .map((f) => `<li><span class="gutter">${formatCount(lineNo(f))}</span><span class="template">${templateHtml(f.template)}</span>${times(f) > 1 ? `<span class="times">×${formatCount(times(f))}</span>` : ""}</li>`)
          .join("")}</ol>
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
void useExample(EXAMPLES[0]!);
