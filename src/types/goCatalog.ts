export type GoPlan = "go" | "goPlus";
export type GoWindowId = "rolling" | "weekly" | "monthly";
export interface GoTokens {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cacheWrite1h: number;
  reasoning: number;
}
export interface GoRates {
  input: string;
  output: string;
  cacheRead: string;
  cacheWrite: string | null;
  cacheWrite1h: string | null;
}
export interface GoTier {
  maxContextTokens: number | null;
  prices: GoRates;
  peakPrices: GoRates | null;
  monthlyLimits: Record<GoPlan, string | null>;
}
export interface GoModel {
  id: string;
  name: string;
  aliases: string[];
  free: boolean;
  notes: string;
  sampleTokens: GoTokens | null;
  tiers: GoTier[];
}
export interface GoSchedule {
  timezone: "UTC";
  weekdays: number[];
  intervals: [number, number][];
}
export interface GoCatalog {
  schemaVersion: 1;
  version: string;
  sourceUrl: string;
  checkedAt: string;
  notes: string;
  windowFractions: Record<GoWindowId, string>;
  peakSchedule: GoSchedule;
  models: GoModel[];
}
export interface GoTariff {
  catalogVersion: string;
  revision: string;
  model: GoModel;
  peakSchedule: GoSchedule;
  selectedRates?: GoRates;
}
export interface GoCatalogInfo {
  catalog: GoCatalog;
  revision: string;
  path: string;
  file: {
    revision: string | null;
    catalog: GoCatalog | null;
    error: string | null;
  };
}
export interface GoTotal {
  requests: number;
  unmatchedRequests: number;
  costUsd: string;
  percent: string;
}
export interface GoWindowEstimate {
  id: GoWindowId;
  start: number;
  end: number;
  basis: "provider_window" | "manual_period" | "lookback";
  total: GoTotal;
  models: { model: string; pricingModel: string | null; usage: GoTotal }[];
}
export interface GoUsageEstimate {
  plan: GoPlan;
  catalogRevision: string;
  windows: GoWindowEstimate[];
  recentHour: { model: string; requests: number; averageTokens: GoTokens }[];
  asOf: number;
  linked: boolean;
}
export interface GoSimulation {
  model: string;
  plan: GoPlan;
  period: string | null;
  requests: number;
  perRequestUsd: string;
  costUsd: string;
  monthlyLimit: string | null;
  percent: Record<GoWindowId, string>;
  catalogRevision: string;
}

export type GoForecastWindow =
  | {
      id: GoWindowId;
      status: "unavailable";
      reason: string;
      resetsAt: number | null;
    }
  | {
      id: GoWindowId;
      status: "ready";
      reason: null;
      resetsAt: number;
      remainingPercent: string;
      postSnapshotPercent: string;
      exhaustionAt: number | null;
      exhaustsBeforeReset: boolean;
      runtimeSeconds: number;
      expectedUsdToReset: string;
      remainingModelUsd: string;
      requestBudget: string;
      requestsPerDay: string;
      requestsPerHour: string;
      tokensPerDay: string;
      usdPerDay: string;
      activeHoursPerDay: string;
      safePaceMultiplier: string;
    };
export interface GoForecast {
  asOf: number;
  catalogRevision: string;
  model: string | null;
  modelName: string | null;
  latestRawModel: string | null;
  plan: GoPlan;
  lookbackMinutes: number;
  pace: string;
  quotaFetchedAt: number | null;
  quotaMaxAgeSeconds: number;
  sample: {
    requests: number;
    spanSeconds: number;
    unpricedRequests: number;
    reason: string | null;
    observedUsdPerHour: string;
    currentUsdPerHour: string;
    requestsPerHour: string;
    monthlyPercentPerHour: string;
    firstAt: number | null;
    lastAt: number | null;
  };
  windows: GoForecastWindow[];
  nextEvent: {
    window: GoWindowId;
    at: number;
    kind: "reset" | "exhaustion";
  } | null;
  dailyBudget: {
    window: GoWindowId;
    requests: string;
    tokens: string;
    usd: string;
    activeHours: string;
  } | null;
}
