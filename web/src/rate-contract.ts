import type { WatchedField } from "./types";

const count = (n: number) => Number.isSafeInteger(n) && n >= 0;
const fraction = (n: number) => Number.isFinite(n) && n >= 0 && n <= 1;
const key = (parts: string[]) => JSON.stringify(parts);

/** Reject stale/mixed workers that ignore the option or omit part of its evidence.
 * The Rust engine computes findings; this boundary checks their shape and denominators. */
export function validRateEvidence(field: WatchedField, requested: number | undefined): boolean {
  const rates = field.rate_comparison;
  if (requested === undefined) return rates === undefined;
  if (!rates || rates.min_change_pp !== requested || !Number.isFinite(rates.significance) || rates.significance < 0 || typeof rates.complete !== "boolean" || !Array.isArray(rates.groups)) return false;
  if (!field.complete) return !rates.complete && rates.groups.length === 0;

  const totals = new Map<string, { baselines: number[]; target: number }>();
  for (const value of field.values) {
    if (!value.baseline_counts.every(count) || !count(value.target_count)) return false;
    const groupKey = key(value.group_values_json ?? []);
    let total = totals.get(groupKey);
    if (!total) {
      total = { baselines: field.baselines.map(() => 0), target: 0 };
      totals.set(groupKey, total);
    }
    value.baseline_counts.forEach((n, i) => { total.baselines[i]! += n; });
    total.target += value.target_count;
  }
  let complete = true;
  for (const group of rates.groups) {
    if (!group || !Array.isArray(group.group_values_json) || !group.group_values_json.every((g) => typeof g === "string")) return false;
    const groupKey = key(group.group_values_json);
    const total = totals.get(groupKey);
    if (!total || !Array.isArray(group.baseline_totals) || group.baseline_totals.length !== field.baselines.length || !group.baseline_totals.every((n, i) => count(n) && n === total.baselines[i]) || !count(group.target_total) || group.target_total !== total.target || !Array.isArray(group.changes)) return false;
    totals.delete(groupKey);
    const status = total.baselines.every((n) => n === 0) ? "no_baseline_observations" : total.target === 0 ? "no_target_observations" : "compared";
    if (group.status !== status) return false;
    if (status !== "compared") {
      complete = false;
      if (group.changes.length) return false;
    }
    const seen = new Set<number>();
    for (const change of group.changes) {
      if (!change || !Number.isSafeInteger(change.value_index)) return false;
      const value = field.values[change.value_index];
      if (!value || value.is_new !== false || key(value.group_values_json ?? []) !== groupKey || seen.has(change.value_index) || !Array.isArray(change.baseline_rates) || change.baseline_rates.length !== field.baselines.length || !change.baseline_rates.every((n, i) => total.baselines[i] === 0 ? n === null : n !== null && fraction(n)) || !fraction(change.target_rate) || !Number.isFinite(change.range_distance_pp) || Math.abs(change.range_distance_pp) > 100 || change.range_distance_pp === 0 || !Number.isFinite(change.score) || change.score < rates.significance) return false;
      seen.add(change.value_index);
    }
  }
  return totals.size === 0 && complete === rates.complete;
}
