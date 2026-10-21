import type { ModelStats } from "@/types/usage";

export function modelStatsRowKey(row: ModelStats) {
  return row.rowKey ?? JSON.stringify([row.model, row.requestMode ?? null]);
}

const modeOrder = (mode?: string | null) =>
  mode === "standard" ? 0 : mode === "fast" ? 1 : mode === "unknown" ? 3 : 2;

/** Paginate whole models, keeping all of a model's modes on the same page. */
export function groupModelStats(stats: readonly ModelStats[]) {
  const groups = new Map<string, { model: string; requests: number; rows: ModelStats[] }>();
  for (const row of stats) {
    const group = groups.get(row.model) ?? { model: row.model, requests: 0, rows: [] };
    group.requests += row.requestCount;
    group.rows.push(row);
    groups.set(row.model, group);
  }
  return [...groups.values()]
    .sort((a, b) => b.requests - a.requests || a.model.localeCompare(b.model))
    .map((group) => ({
      ...group,
      rows: [...group.rows].sort((a, b) =>
        modeOrder(a.requestMode) - modeOrder(b.requestMode)
        || (a.requestMode ?? "").localeCompare(b.requestMode ?? "")),
    }));
}
