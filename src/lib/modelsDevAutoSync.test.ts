import { beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "./api/usage";
import { fetchModelsDev } from "./modelsDev";
import { syncModelsDevPricing } from "./modelsDevAutoSync";
import type { ModelsDevSyncState } from "@/types/usage";

vi.mock("./api/usage", () => ({
  usageApi: {
    getModelsNeedingPrices: vi.fn(),
    getModelsDevSyncConfig: vi.fn(),
    updateModelPricingBatchDetailed: vi.fn(),
    recordModelsDevSyncResult: vi.fn(),
  },
}));
vi.mock("./modelsDev", async (original) => ({
  ...(await original<typeof import("./modelsDev")>()),
  fetchModelsDev: vi.fn(),
}));
const state = (enabled = true): ModelsDevSyncState => ({
  configPath: "fixture",
  config: {
    autoSyncEnabled: enabled,
    includeCommonModels: false,
    selectedModelKeys: ["deepseek/deepseek-flash"],
    excludedCommonModelKeys: [],
    lastSyncAt: Date.now(),
    lastSyncError: null,
  },
});
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(usageApi.getModelsNeedingPrices).mockResolvedValue([]);
  vi.mocked(usageApi.getModelsDevSyncConfig).mockResolvedValue(state());
  vi.mocked(fetchModelsDev).mockResolvedValue({
    deepseek: {
      models: { "deepseek-flash": { cost: { input: 0.15, output: 0.6 } } },
    },
  });
});
describe("automatic pricing refresh", () => {
  it("skips recent or disabled imports without indicating changed usage", async () => {
    expect(await syncModelsDevPricing(state())).toMatchObject({
      skipped: true,
      changed: 0,
      usageChanged: 0,
    });
    expect(await syncModelsDevPricing(state(false))).toMatchObject({
      skipped: true,
      changed: 0,
      usageChanged: 0,
    });
    expect(fetchModelsDev).not.toHaveBeenCalled();
    expect(usageApi.updateModelPricingBatchDetailed).not.toHaveBeenCalled();
  });
  it("distinguishes price refreshes from actual historical cost backfills", async () => {
    vi.mocked(usageApi.updateModelPricingBatchDetailed)
      .mockResolvedValueOnce({ saved: 1, usageChanged: 0 })
      .mockResolvedValueOnce({ saved: 1, usageChanged: 3 });
    expect(await syncModelsDevPricing(state(), true)).toMatchObject({
      changed: 1,
      usageChanged: 0,
    });
    expect(await syncModelsDevPricing(state(), true)).toMatchObject({
      changed: 1,
      usageChanged: 3,
    });
    expect(usageApi.recordModelsDevSyncResult).toHaveBeenCalledTimes(2);
  });
});
