import { contextHtml, esc, fieldGroupHtml } from "./html";
import { fieldComplete, rateCell, rateObservation, rateSummary, rateCoverageHtml } from "./field-rates";
import { formatCount, printable } from "./template";
import type { FieldCoverage, FieldValue, WatchedField } from "./types";

const PAGE_SIZE = 32;
interface View {
  open: boolean;
  query: string;
  page: number;
  sources: boolean;
}

function coverageHtml(coverage: FieldCoverage): string {
  return [
    `${formatCount(coverage.matched)} matched`,
    `${formatCount(coverage.missing)} absent`,
    `${formatCount(coverage.non_json)} non-JSON`,
    ...([
      [coverage.invalid_json, "invalid JSON"], [coverage.non_scalar, "non-scalar"],
      [coverage.ambiguous, "ambiguous"], [coverage.oversized_records, "oversized"],
      [coverage.untracked, "untracked"], [coverage.group_missing ?? 0, "missing group key"],
      [coverage.group_non_scalar ?? 0, "non-scalar group key"], [coverage.group_ambiguous ?? 0, "ambiguous group key"],
    ] as [number, string][]).filter(([n]) => n > 0).map(([n, label]) => `${formatCount(n)} ${label}`),
  ].join("; ");
}

/** View state never mutates the completed report. Closed ledgers and source folds have
 * no hidden copies of the evidence in the DOM; both exports retain the whole result. */
export class FieldLedger {
  private fields: WatchedField[] = [];
  private labels: string[] = [];
  private views = new Map<string, View>();

  constructor(private root: HTMLElement) {
    root.addEventListener("toggle", (event) => {
      const details = event.target;
      if (!(details instanceof HTMLDetailsElement) || !details.isConnected) return;
      const section = details.closest<HTMLElement>("[data-field-details]");
      const index = Number(section?.dataset["fieldDetails"]);
      const field = this.fields[index];
      if (!section || !field) return;
      const view = this.view(field);
      if (details === section && view.open !== details.open) {
        view.open = details.open;
        section.querySelector<HTMLElement>(".field-body")!.innerHTML = view.open ? this.body(field, index) : "";
      } else if (details.classList.contains("field-sources") && view.sources !== details.open) {
        view.sources = details.open;
        details.querySelector<HTMLElement>(".field-source-records")!.innerHTML = view.sources ? this.sources(field, this.page(field).values) : "";
      }
    }, true);
    root.addEventListener("input", (event) => {
      const input = event.target;
      if (!(input instanceof HTMLInputElement) || !input.hasAttribute("data-field-query")) return;
      const index = Number(input.dataset["fieldQuery"]);
      const field = this.fields[index];
      if (!field) return;
      Object.assign(this.view(field), { query: input.value, page: 0 });
      this.refreshList(field, index);
    });
    root.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-field-page]");
      if (!button || button.disabled) return;
      const section = button.closest<HTMLElement>("[data-field-details]")!;
      const index = Number(section.dataset["fieldDetails"]);
      const field = this.fields[index];
      if (!field) return;
      this.view(field).page += button.dataset["fieldPage"] === "next" ? 1 : -1;
      this.refreshList(field, index);
      section.querySelector<HTMLElement>(".field-position")!.focus();
    });
  }

  reset(): void {
    this.views.clear();
    this.fields = [];
    this.labels = [];
  }

  html(fields: WatchedField[], labels: string[]): string {
    this.fields = fields;
    this.labels = labels;
    if (!fields.length) return "";
    return `<section class="watched-fields" aria-label="Watched field evidence">
      <h3>Watched fields</h3>
      <p>Exact scalar values, ${fields.some((f) => f.group_by?.length) ? "compared within the selected groups" : "pooled across all JSON records in each run"}. A new value is an observation, not proof of a failure. Types and number spelling are preserved; masks do not redact this evidence.</p>
      ${fields.map((field, index) => {
        const count = field.values.filter((v) => v.is_new === true).length;
        const noun = field.group_by?.length ? "group/value pair" : "value";
        const open = this.view(field).open;
        return `<details class="field-evidence" data-field-details="${index}" ${open ? "open" : ""}>
          <summary><code>${esc(printable(field.pointer))}</code> <span>${field.complete ? `${count} new ${noun}${count === 1 ? "" : "s"}` : "Incomplete"}${rateSummary(field)}</span></summary>
          <div class="field-body">${open ? this.body(field, index) : ""}</div>
        </details>`;
      }).join("")}
    </section>`;
  }

  private view(field: WatchedField): View {
    let view = this.views.get(field.pointer);
    if (!view) {
      view = { open: !fieldComplete(field), query: "", page: 0, sources: false };
      this.views.set(field.pointer, view);
    }
    return view;
  }

  private page(field: WatchedField): { values: FieldValue[]; count: number; start: number } {
    const view = this.view(field);
    const query = view.query.trim().toLowerCase();
    const matches = query ? field.values.filter((v) => [v.value_json, ...v.group_values_json ?? []].some((s) => printable(s).toLowerCase().includes(query))) : field.values;
    view.page = Math.max(0, Math.min(view.page, Math.ceil(matches.length / PAGE_SIZE) - 1));
    const start = view.page * PAGE_SIZE;
    return { values: matches.slice(start, start + PAGE_SIZE), count: matches.length, start };
  }

  private refreshList(field: WatchedField, index: number): void {
    this.root.querySelector<HTMLElement>(`[data-field-list="${index}"]`)!.innerHTML = this.list(field);
  }

  private list(field: WatchedField): string {
    const view = this.view(field);
    const { values, count, start } = this.page(field);
    const grouped = !!field.group_by?.length;
    return `<nav class="field-pages" aria-label="Values for ${esc(field.pointer)}">
      <span class="field-position" tabindex="-1" aria-live="polite" aria-atomic="true">${count ? `Showing ${start + 1}–${start + values.length} of ${formatCount(count)} ${grouped ? "pairs" : "values"}` : "No matching values"}${view.query.trim() ? ` (${formatCount(field.values.length)} total)` : ""}</span>
      ${count > PAGE_SIZE ? `<button type="button" class="link-button" data-field-page="previous" ${start === 0 ? "disabled" : ""}>Previous values</button><button type="button" class="link-button" data-field-page="next" ${start + values.length >= count ? "disabled" : ""}>Next values</button>` : ""}
    </nav>
    ${values.length ? `<div class="field-table-scroll" tabindex="0" role="group" aria-label="Field value counts for ${esc(field.pointer)}">
      <table class="field-values"><caption>${field.rate_comparison?.groups.length ? "Value counts / group observations (share)" : "Value counts"} for <code>${esc(printable(field.pointer))}</code></caption>
        <thead><tr>${(field.group_by ?? []).map((p) => `<th scope="col">Group <code>${esc(printable(p))}</code></th>`).join("")}<th scope="col">Value (JSON)</th>${this.labels.map((label) => `<th scope="col">${esc(label)}</th>`).join("")}<th scope="col">Target</th><th scope="col">Observation</th></tr></thead>
        <tbody>${values.map((v) => `<tr class="${v.is_new === true ? "field-new" : ""}">${(v.group_values_json ?? []).map((g) => `<td class="field-key"><code>${esc(printable(g))}</code></td>`).join("")}<th scope="row"><code>${esc(printable(v.value_json))}</code></th>${v.baseline_counts.map((_, i) => `<td>${rateCell(field, v, i)}</td>`).join("")}<td>${rateCell(field, v)}</td><td>${v.is_new === true ? v.group_seen_in_baseline === false ? "New group" : "New value" : v.is_new === null ? "Unknown" : "Seen in baseline"}${rateObservation(field, v)}</td></tr>`).join("")}</tbody>
      </table>
    </div>
    <p class="field-scroll-hint">Scroll the value table horizontally to see every run and observation.</p>
    <details class="field-sources" ${view.sources ? "open" : ""}><summary>First source occurrences for these ${values.length} ${grouped ? "pairs" : "values"}</summary><div class="field-source-records">${view.sources ? this.sources(field, values) : ""}</div></details>` : ""}`;
  }

  private sources(field: WatchedField, values: FieldValue[]): string {
    return values.map((v) => `${fieldGroupHtml(field, v)}<p><code>${esc(printable(v.value_json))}</code></p>${[v.first_baseline, v.first_target].flatMap((at) => at ? [`<p>${at.baseline_index === undefined ? "Target" : esc(this.labels[at.baseline_index]!)}, line ${formatCount(at.line_no)}${at.truncated ? " (excerpt clipped)" : ""}</p>${contextHtml(at.line_no, at.raw, undefined)}`] : []).join("")}`).join("");
  }

  private body(field: WatchedField, index: number): string {
    const grouped = !!field.group_by?.length;
    return `${!field.complete ? '<p class="field-incomplete">This watch needs a scalar value in every run and no invalid, ambiguous, non-scalar or untracked records or group keys. Novelty is unknown. Check the path and coverage, or narrow your input.</p>' : ""}
      ${rateCoverageHtml(field)}
      <div class="field-search"><label for="field-query-${index}">Find ${grouped ? "group or value" : "value"}</label><input type="search" id="field-query-${index}" data-field-query="${index}" value="${esc(this.view(field).query)}" autocomplete="off" spellcheck="false" placeholder="Case-insensitive text in the JSON values"></div>
      <div data-field-list="${index}">${this.list(field)}</div>
      ${grouped ? '<p class="field-known">New group means no baseline observation of this field for that key. A group can be known in any baseline; it need not occur in every run.</p>' : ""}
      <table class="field-coverage"><caption>Coverage of all input lines (unfiltered)</caption><thead><tr><th scope="col">Run</th><th scope="col">Records</th></tr></thead><tbody>
        ${[...field.baselines, field.target].map((c, i) => `<tr><th scope="row">${esc(this.labels[i] ?? "Target")}</th><td>${coverageHtml(c)}</td></tr>`).join("")}
      </tbody></table>
      ${[...field.baselines, field.target].flatMap((c, i) => c.first_problem ? [`<p class="field-problem">First problem in ${esc(this.labels[i] ?? "Target")}, line ${formatCount(c.first_problem.line_no)}${c.first_problem.truncated ? " (excerpt clipped)" : ""}:</p>${contextHtml(c.first_problem.line_no, c.first_problem.raw, undefined)}`] : []).join("")}
      <p class="field-limits">Limits per field: ${grouped ? "256 distinct group/value pairs, 4 KiB for the combined group key" : "64 distinct values"} and 4 KiB per value. Records over 1 MiB are unassessed. Absent fields and non-JSON lines are counted separately; null is a value.${grouped ? " A selected scalar without all group keys makes the watch incomplete." : ""} Downloads include all retained values and source excerpts, regardless of search or page.</p>`;
  }
}
