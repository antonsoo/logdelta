import { contextHtml, esc, fieldGroupHtml } from "./html";
import { formatCount, printable } from "./template";
import type { FieldRateChange, FieldRateGroup, FieldValue, WatchedField } from "./types";

export function fieldComplete(field: WatchedField): boolean {
  return field.complete && field.rate_comparison?.complete !== false;
}

export function rateFindingCount(field: WatchedField): number {
  return field.rate_comparison?.groups.filter((g) => g.changes.length > 0).length ?? 0;
}

// Keep very small nonzero rates visible; exact fractions are in both downloads.
export function rateNumber(n: number): string {
  return n !== 0 && Math.abs(n) < 0.001 ? n.toExponential(2) : n.toLocaleString("en-US", { maximumFractionDigits: 3 });
}

export function rateFraction(count: number, total: number): string {
  return total === 0 ? "Unobserved" : `${formatCount(count)} / ${formatCount(total)} (${rateNumber(100 * count / total)}%)`;
}

function distance(change: FieldRateChange): string {
  return `${change.range_distance_pp > 0 ? "+" : ""}${rateNumber(change.range_distance_pp)} pp`;
}

export function rateGroup(field: WatchedField, value: FieldValue): FieldRateGroup | undefined {
  return field.rate_comparison?.groups.find((g) => g.group_values_json.length === (value.group_values_json?.length ?? 0) && g.group_values_json.every((key, i) => key === value.group_values_json?.[i]));
}

export function rateCell(field: WatchedField, value: FieldValue, baseline?: number): string {
  const count = baseline === undefined ? value.target_count : value.baseline_counts[baseline]!;
  const group = rateGroup(field, value);
  if (!group) return formatCount(count);
  return `<span class="rate-fraction">${rateFraction(count, baseline === undefined ? group.target_total : group.baseline_totals[baseline]!)}</span>`;
}

export function rateObservation(field: WatchedField, value: FieldValue): string {
  if (!field.rate_comparison) return "";
  const group = rateGroup(field, value);
  const status = !group ? "Rate unavailable: incomplete field coverage" : group.status === "no_baseline_observations" ? "Rate unavailable: no baseline observations" : group.status === "no_target_observations" ? "Rate unavailable: no target observations" : undefined;
  const change = group?.changes.find((c) => field.values[c.value_index] === value);
  return `<span class="rate-observation">${status ?? (change ? `${distance(change)} beyond baseline range; score ${rateNumber(change.score)}` : value.is_new ? "New value; no known-value rate finding" : "No rate finding under these thresholds")}</span>`;
}

export function rateSummary(field: WatchedField): string {
  if (!field.rate_comparison) return "";
  const n = rateFindingCount(field);
  return ` · ${n} rate ${n === 1 ? "finding" : "findings"}${field.rate_comparison.complete ? "" : " · Rates incomplete"}`;
}

export function rateCoverageHtml(field: WatchedField): string {
  const rate = field.rate_comparison;
  if (!rate) return "";
  const unknown = rate.groups.filter((g) => g.status !== "compared").length;
  return `${!rate.complete ? `<p class="field-incomplete">${field.complete ? `${unknown} ${unknown === 1 ? "group has" : "groups have"} no observations on one side. Their rates cannot be compared; unobserved is not 0%. The ledger identifies each group.` : "Rates unavailable: incomplete field coverage cannot supply trustworthy denominators. Retained counts below are not a complete rate comparison."}</p>` : ""}
    <p class="field-known">Rates use matched scalar observations within each group, not all log lines. Minimum change: ${rateNumber(rate.min_change_pp)} percentage points beyond every observed baseline rate; minimum score: ${rateNumber(rate.significance)}. The score is a triage heuristic, not a calibrated p-value. A missing baseline group is unobserved, not 0%.</p>`;
}

export function rateFindingHtml(field: WatchedField, group: FieldRateGroup, fieldIndex: number, labels: string[]): string {
  // Increases first, then decreases. Keep large ledgers bounded like the exact-value view.
  const changes = [...group.changes].sort((a, b) => Math.sign(b.range_distance_pp) - Math.sign(a.range_distance_pp) || Math.abs(b.range_distance_pp) - Math.abs(a.range_distance_pp)).slice(0, 32);
  const lead = changes[0]!;
  const observed = lead.baseline_rates.filter((n): n is number => n !== null);
  const low = Math.min(...observed), high = Math.max(...observed);
  const range = low === high ? `${rateNumber(low * 100)}%` : `${rateNumber(low * 100)}–${rateNumber(high * 100)}%`;
  return `<li class="finding kind-rate">
    <div class="finding-head"><span class="kind">Field rate</span><span class="counts"><code>${esc(printable(field.pointer))}</code> · ${group.changes.length} known ${group.changes.length === 1 ? "value changed" : "values changed"} in one ${field.group_by?.length ? "group" : "pooled comparison"}</span></div>
    ${fieldGroupHtml(field, group)}
    <p class="rate-headline"><code>${esc(printable(field.values[lead.value_index]!.value_json))}</code> <span>${range} in observed baselines</span> <span aria-hidden="true">→</span> <strong>${rateNumber(lead.target_rate * 100)}% in target</strong><span class="rate-headline-distance">${distance(lead)} beyond the baseline range</span></p>
    <div class="field-table-scroll" tabindex="0" role="group" aria-label="Changed rates for ${esc(field.pointer)}">
      <table class="field-values rate-values"><caption>Counts / group observations (share), before masking</caption>
        <thead><tr><th scope="col">Value (JSON)</th>${labels.map((l) => `<th scope="col">${esc(l)}</th>`).join("")}<th scope="col">Target</th><th scope="col">Beyond baseline range</th><th scope="col">Score</th></tr></thead>
        <tbody>${changes.map((c) => {
          const value = field.values[c.value_index]!;
          return `<tr><th scope="row"><code>${esc(printable(value.value_json))}</code></th>${value.baseline_counts.map((count, i) => `<td class="rate-fraction">${rateFraction(count, group.baseline_totals[i]!)}</td>`).join("")}<td class="rate-fraction rate-target">${rateFraction(value.target_count, group.target_total)}</td><td class="rate-distance">${distance(c)}</td><td>${rateNumber(c.score)}</td></tr>`;
        }).join("")}</tbody>
      </table>
    </div>
    <p class="field-scroll-hint">Scroll the rate table horizontally to compare every run.</p>
    <p class="field-known">One rate finding for this group; complementary values can move together. The score ranks evidence, not the probability of a failure. Inspect the first occurrences below; they locate the value, not the moment its rate changed.</p>
    ${changes.map((c) => {
      const value = field.values[c.value_index]!;
      const at = value.first_target ?? value.first_baseline;
      if (!at) return "";
      const label = at.baseline_index === undefined ? "Target" : labels[at.baseline_index]!;
      return `<details class="rate-source" data-rate-source="${fieldIndex}:${c.value_index}"><summary><code>${esc(printable(value.value_json))}</code> · ${esc(label)}, line ${formatCount(at.line_no)}${at.truncated ? " (excerpt clipped)" : ""}${value.target_count === 0 ? " · absent from target" : ""}</summary><div class="rate-source-body"></div></details>`;
    }).join("")}
    ${changes.length < group.changes.length ? `<p class="field-known">Showing ${changes.length} of ${group.changes.length} changed values. The searchable field ledger and both downloads retain every change.</p>` : ""}
  </li>`;
}

/** Source records enter the DOM only when opened and use the completed comparison. */
export function rateSourceHtml(value: FieldValue): string {
  const at = value.first_target ?? value.first_baseline;
  if (!at) return "";
  return `${contextHtml(at.line_no, at.raw, at.baseline_index === undefined ? value.context : undefined)}
    ${at.truncated || value.context_truncated ? '<p class="field-known">Source or surrounding context clipped. Open the original log at this line for the full record.</p>' : ""}`;
}
