import { beforeAll, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "@/i18n/locales/en.json";
import { usageKeys } from "@/lib/query/usage";
import { aggregateSummaries, UsageHero } from "./UsageHero";
import type { UsageSummary } from "@/types/usage";

it("counts model measurements separately and preserves incomplete cost coverage", () => {
  const models: UsageSummary = { totalRequests: 0, modelUsageCount: 2, incompleteUsageCount: 1, upperBoundCount: 1, totalCost: "0.0104", totalInputTokens: 1000, totalOutputTokens: 100, totalCacheCreationTokens: 0, totalCacheReadTokens: 0, successRate: null, realTotalTokens: 1100, cacheHitRate: 0, unpricedCount: 1, cacheBreakdownKnown: false };
  expect(aggregateSummaries([models])).toMatchObject({totalRequests:0, modelUsageCount:2, pricedCount:1, incompleteUsageCount:1, upperBoundCount:1, cacheBreakdownKnown:false});
});

it("preserves totals while marking mixed LM Studio cache coverage incomplete", () => {
  const known: UsageSummary = { totalRequests: 1, totalCost: "1", totalInputTokens: 10, totalOutputTokens: 5, totalCacheCreationTokens: 0, totalCacheReadTokens: 10, successRate: null, realTotalTokens: 25, cacheHitRate: 0.5 };
  const local: UsageSummary = { ...known, totalInputTokens: 20, totalCacheReadTokens: 0, cacheHitRate: 0, totalCost: "0", unpricedCount: 1, unpricedTokens:25, cacheBreakdownKnown: false };
  expect(aggregateSummaries([known, local])).toMatchObject({ totalRequests: 2, realTotalTokens: 50, totalCost: "1.000000", unpricedCount: 1, unpricedTokens:25, pricedCount:1, cacheBreakdownKnown: false });
  expect(aggregateSummaries([known]).cacheBreakdownKnown).toBe(true);
  expect(aggregateSummaries([known, local]).cacheHitRate).toBe(0.5);
});

const translations = createInstance();
beforeAll(async () => {
  await translations.use(initReactI18next).init({
    lng: "en", resources: { en: { translation: en } },
  });
});
const mixedSummary: UsageSummary = {
  totalRequests: 1, modelUsageCount: 1, totalCost: "1.25",
  totalInputTokens: 1020, totalOutputTokens: 100,
  totalCacheCreationTokens: 0, totalCacheReadTokens: 80,
  realTotalTokens: 1200, successRate: null, cacheBreakdownKnown: false,
  cacheHitRate: 0.8, cacheHitRateInputTokens: 100, cacheHitRateReadTokens: 80,
};
function renderHero(summary: UsageSummary, appType?: string) {
  const client = new QueryClient({ defaultOptions: { queries: { enabled: false, gcTime: Infinity } } });
  client.setQueryData(["snapshot"], { config: { timezone: "UTC" } });
  client.setQueryData(
    [...usageKeys.summaryByApp("today", undefined, undefined, {}), "UTC"],
    [{ appType: "gpt-img-studio", summary }],
  );
  return renderToStaticMarkup(createElement(I18nextProvider, { i18n: translations },
    createElement(QueryClientProvider, { client },
      createElement(UsageHero, { range: { preset: "today" }, appType, refreshIntervalMs: 0 })),
  ));
}

it("weights eligible token counts even when one app mixes known and unknown records", () => {
  const other: UsageSummary = { ...mixedSummary, cacheBreakdownKnown: true,
    totalInputTokens: 210, totalCacheReadTokens: 90, realTotalTokens: 400,
    cacheHitRate: 0.3, cacheHitRateInputTokens: 300, cacheHitRateReadTokens: 90 };
  expect(aggregateSummaries([mixedSummary, other])).toMatchObject({
    cacheBreakdownKnown: false, cacheHitRate: 0.425,
    cacheHitRateInputTokens: 400, cacheHitRateReadTokens: 170,
    realTotalTokens: 1600, totalCost: "2.500000",
  });
});

it("displays the eligible hit rate for both all-app and single-app views", () => {
  expect(renderHero(mixedSummary)).toContain(">80.0%</span>");
  expect(renderHero(mixedSummary, "gpt-img-studio")).toContain(">80.0%</span>");
});

it("keeps wholly unknown cache usage unknown while a known cache miss stays 0%", () => {
  const unknown: UsageSummary = { ...mixedSummary, cacheHitRate: null,
    cacheHitRateInputTokens: 0, cacheHitRateReadTokens: 0 };
  expect(aggregateSummaries([unknown]).cacheHitRate).toBeNull();
  expect(renderHero(unknown)).toContain(">—</span>");
  expect(renderHero(unknown)).not.toContain(">0.0%</span>");
  const miss: UsageSummary = { ...mixedSummary, cacheBreakdownKnown: true,
    cacheHitRate: 0, cacheHitRateInputTokens: 100, cacheHitRateReadTokens: 0 };
  expect(renderHero(miss)).toContain(">0.0%</span>");
});
