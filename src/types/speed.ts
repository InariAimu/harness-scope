export interface SpeedDistribution {
  sampleCount: number;
  mean: number | null;
  min: number | null;
  max: number | null;
  centralLower: number | null;
  centralUpper: number | null;
  centralSampleCount: number;
}
export interface SpeedStatistics {
  speed: SpeedDistribution;
  /** Seconds, using exactly the same eligible requests as speed. */
  ttft: SpeedDistribution;
  estimatedSampleCount: number;
}
export interface SpeedBucket extends SpeedStatistics {
  start: number;
  /** Exclusive; the query's inclusive endDate is represented as endDate + 1. */
  end: number;
}
export interface SpeedTrends {
  startDate: number;
  endDate: number;
  bucketSeconds: number;
  selectedRecordCount: number;
  excludedRecordCount: number;
  summary: SpeedStatistics;
  buckets: SpeedBucket[];
}

/** Additive totals for the same strict decode sample population as the interval chart. */
export interface SpeedDailyTotals {
  outputTokens: number;
  generationMs: number;
  firstTokenMs: number;
  sampleCount: number;
  estimatedSampleCount: number;
}
export interface SpeedDay extends SpeedDailyTotals {
  /** Calendar date in the configured application timezone, YYYY-MM-DD. */
  date: string;
}
export interface SpeedDaily {
  days: SpeedDay[];
  selectedRecordCount: number;
  excludedRecordCount: number;
}
