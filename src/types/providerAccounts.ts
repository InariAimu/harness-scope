import type { UsageSummary } from "./usage";
import type {
  CodexAllowanceMode,
  ResolvedCodexCapacity,
} from "./codexAllowances";

export type AccountProvider = "deepseek" | "opencode-go" | "openai-codex";
export type CodexPlan = "plus" | "pro100" | "pro200" | "pro500";
export interface CodexSettings {
  plan: CodexPlan;
  binaryPath: string | null;
  homePath: string | null;
}
export interface AccountLink {
  harness: string;
  provider: string;
}
export interface AccountSubscription {
  planName: string;
  monthlyFee: string | null;
  currency: string;
  periodStart: string | null;
  renewalDate: string | null;
}
export interface ProviderAccount {
  id: string;
  provider: AccountProvider;
  name: string;
  links: AccountLink[];
  subscription: AccountSubscription;
  refresh: RefreshPolicy;
  codex?: CodexSettings;
}
export interface RefreshPolicy {
  foregroundSeconds: number;
  backgroundSeconds: number;
}
export interface QuotaWindow {
  id: string;
  durationMinutes?: number;
  usedPercent: number;
  status: "ok" | "rate-limited" | "unknown";
  resetsAt: number;
}
export type ProviderData =
  | {
      kind: "balance";
      available: boolean;
      balances: {
        currency: string;
        total: string;
        granted: string;
        toppedUp: string;
      }[];
    }
  | { kind: "quota"; windows: QuotaWindow[] }
  | CodexQuota;
export interface CodexQuota {
  kind: "codexQuota";
  planType: string | null;
  buckets: {
    id: string;
    name: string | null;
    planType: string | null;
    windows: QuotaWindow[];
    credits: {
      hasCredits: boolean | null;
      unlimited: boolean | null;
      balance: string | null;
    } | null;
  }[];
}
export interface ProviderAccountView {
  account: ProviderAccount;
  keyConfigured: boolean;
  snapshot: { fetchedAt: number; data: ProviderData } | null;
  lastAttemptAt: number | null;
  error: string | null;
  refreshing: boolean;
}
export interface AccountSave {
  id?: string;
  provider: AccountProvider;
  name: string;
  links: AccountLink[];
  subscription: AccountSubscription;
  apiKey?: string;
  clearApiKey?: boolean;
  refresh: RefreshPolicy;
  codex?: CodexSettings;
}

export interface CodexReferenceWindow {
  id: "rolling" | "weekly";
  budgetUsd: string | null;
  capacity: ResolvedCodexCapacity | null;
  remainingUsd?: string;
  officialRemainingPercent?: string;
  resetsAt: number | null;
  status: "ready" | "unavailable";
  reason: string | null;
  runtimeSeconds?: number;
  exhaustsBeforeReset?: boolean;
  requestBudget?: string;
  requestsPerDay?: string;
  requestsPerHour?: string;
  tokensPerDay?: string;
  usdPerDay?: string;
  activeHoursPerDay?: string;
  requiredIntensity?: string;
}
export interface CodexReference {
  asOf: number;
  model: string | null;
  models: string[];
  plan: CodexPlan;
  multiplier: number;
  monthlyFeeUsd: string;
  mode: CodexAllowanceMode;
  catalogVersion: string;
  catalogRevision: string;
  quotaFetchedAt: number | null;
  bucket: string;
  pace: string;
  sample: {
    startDate?: number;
    endDate?: number;
    durationSeconds?: number;
    windowStart?: number | null;
    windowMinutes?: number | null;
    requests: number;
    tokens: number;
    unpricedRequests: number;
    costUsd: string;
    spanSeconds: number;
    lastAt: number | null;
    reason: string | null;
    rateUsdPerHour: string;
    requestsPerHour: string;
  };
  calibration: {
    currentWindowTarget: { intensity: string; durationMinutes: number } | null;
    identityVerified: boolean;
    historyPoints: number;
    windows: {
      id: string;
      durationMinutes: number;
      resetsAt: number;
      status: "ready" | "unavailable";
      reason: string | null;
      observations: number;
      sampleStart?: number;
      sampleEnd?: number;
      spanSeconds?: number;
      deltaPercent?: string;
      observedPercentPerHour?: string;
      localRequests?: number;
      unpricedRequests?: number;
      localUsd?: string;
      modelCount?: number;
      modelPercentPerHour?: string;
      requiredPercentPerHour?: string;
      requiredIntensity?: string;
      requiredUsdPerHour?: string;
      requiredRequestsPerHour?: string;
      coversReset?: boolean;
      runtimeSeconds?: number;
    }[];
  };
  windows: CodexReferenceWindow[];
  nextEvent: {
    window: "rolling" | "weekly";
    at: number;
    kind: "exhaustion" | "reset";
  } | null;
  dailyBudget: {
    window: "rolling" | "weekly";
    requests: string;
    tokens: string;
    usd: string;
  } | null;
}
export interface AccountRoute extends AccountLink {
  records: number;
}
export interface AccountUsage {
  linked: boolean;
  startDate: number;
  endDate: number;
  timezone: string;
  summary: UsageSummary;
  models: { model: string; summary: UsageSummary }[];
}
