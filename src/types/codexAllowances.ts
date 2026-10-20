import type { CodexPlan } from "./providerAccounts";
export type CodexAllowanceMode = "modeldial" | "theoretical";
export interface CodexSubscriptionSpeed {
  sourceUrl: string;
  checkedAt: string;
  fastMultiplier: string;
  purchasedCreditsFastMultiplier: string;
  fastModelIds: string[];
}
export interface CodexCapacity {
  apiEquivalentUsd: string;
  tokens: string | null;
}
export interface ResolvedCodexCapacity extends CodexCapacity {
  basis: "modeldial" | "theoretical" | "weekly_division";
  fromPlan: string;
  fromWindow: string;
  multiplier: string;
}
export interface CodexAllowanceCatalog {
  schemaVersion: number;
  version: string;
  sourceUrl: string;
  checkedAt: string;
  notes: string;
  subscriptionSpeed: CodexSubscriptionSpeed;
  defaultMode: CodexAllowanceMode;
  theoreticalFallback: boolean;
  plusWeeklyToFiveHourDivisor: string | null;
  bucketIds: string[];
  plans: {
    id: CodexPlan;
    name: string;
    monthlyFeeUsd: string;
    theoreticalMultiplier: number;
  }[];
  models: {
    id: string;
    name: string;
    aliases: string[];
    notes: string;
    estimates: Partial<
      Record<
        CodexPlan,
        { weekly: CodexCapacity | null; rolling: CodexCapacity | null }
      >
    >;
  }[];
}
export interface CodexAllowanceInfo {
  catalog: CodexAllowanceCatalog;
  revision: string;
  path: string;
  file: {
    revision: string | null;
    catalog: CodexAllowanceCatalog | null;
    error: string | null;
  };
  resolved: Record<
    CodexAllowanceMode,
    {
      id: string;
      name: string;
      notes: string;
      plans: Record<CodexPlan, ResolvedCodexCapacity | null>;
    }[]
  >;
}
