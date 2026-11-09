import type { SpeedDailyTotals } from "@/types/speed";
import type { SpeedMetric } from "./speedChart";

export function sumSpeedDays(rows: SpeedDailyTotals[]): SpeedDailyTotals {
  return rows.reduce((sum, row) => ({
    outputTokens: sum.outputTokens + row.outputTokens,
    generationMs: sum.generationMs + row.generationMs,
    firstTokenMs: sum.firstTokenMs + row.firstTokenMs,
    sampleCount: sum.sampleCount + row.sampleCount,
    estimatedSampleCount: sum.estimatedSampleCount + row.estimatedSampleCount,
  }), { outputTokens: 0, generationMs: 0, firstTokenMs: 0, sampleCount: 0, estimatedSampleCount: 0 });
}
export function dailySpeedMean(row: SpeedDailyTotals | undefined, metric: SpeedMetric): number | null {
  if (!row?.sampleCount) return null;
  return metric === "speed" ? row.generationMs > 0 ? row.outputTokens * 1000 / row.generationMs : null
    : row.firstTokenMs / row.sampleCount / 1000;
}
