import type { UsageSummary } from "./usage";

export type AccountProvider = "deepseek" | "opencode-go";
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
}
export interface QuotaWindow {
  id: "rolling" | "weekly" | "monthly";
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
  | { kind: "quota"; windows: QuotaWindow[] };
export interface ProviderAccountView {
  account: ProviderAccount;
  keyConfigured: boolean;
  snapshot: { fetchedAt: number; data: ProviderData } | null;
  lastAttemptAt: number | null;
  error: string | null;
}
export interface AccountSave {
  id?: string;
  provider: AccountProvider;
  name: string;
  links: AccountLink[];
  subscription: AccountSubscription;
  apiKey?: string;
  clearApiKey?: boolean;
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
