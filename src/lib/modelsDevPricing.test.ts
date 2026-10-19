import { describe, it, expect } from "vitest";
import {
  flattenModels,
  toModelPricing,
  resolveModelsDevSelection,
  resolveUsageModelReferences,
} from "./modelsDevPricing";
describe("models.dev import provenance", () => {
  it("matches observed aliases and prefers the model maker for unconfigured routes", () => {
    const entries = flattenModels({
      proxy: {
        models: {
          "deepseek-v4-flash": {
            cost: { input: 0, output: 0 },
            release_date: "2026-10-19",
          },
        },
      },
      deepseek: {
        models: {
          "deepseek-v4.1-flash": { cost: { input: 0.15, output: 0.6 } },
        },
      },
      openai: { models: { "gpt-5.6-luna": { cost: { input: 1, output: 2 } } } },
    });
    const references = resolveUsageModelReferences(entries, [
      "vendor/deepseek-flash:high",
      "deepseek-v4-flash-vision-exp",
      "gpt-5.6-luna-high",
      "missing-model",
    ]);
    expect(references.map((r) => r.key)).toEqual([
      "deepseek/deepseek-v4.1-flash",
      "openai/gpt-5.6-luna",
    ]);
    expect(toModelPricing(references)[0].provider).toBeUndefined();
  });
  it("keeps provider/model provenance after normalization", () => {
    const rows = flattenModels({
      openai: {
        name: "OpenAI",
        models: {
          "gpt-5.4": {
            name: "GPT",
            cost: { input: 2.5, output: 15, cache_read: 0.25 },
          },
        },
      },
    });
    const result = toModelPricing(rows)[0];
    expect(result.modelId).toBe("gpt-5.4");
    expect(result.source).toBe("models_dev");
    expect(result.sourceKey).toBe("openai/gpt-5.4");
    expect(result.inputCostPerMillion).toBe("2.5");
  });
  it("does not import non-text or deprecated models", () => {
    const rows = flattenModels({
      example: {
        name: "Example",
        models: {
          "embedding-model": {
            name: "Embedding",
            cost: { input: 1, output: 0 },
          },
          old: {
            name: "old",
            status: "deprecated",
            cost: { input: 1, output: 1 },
          },
          chat: { name: "Chat", cost: { input: 1, output: 2 } },
        },
      },
    });
    expect(rows.map((r) => r.modelId)).toEqual(["chat"]);
    expect(
      resolveModelsDevSelection(rows, {
        autoSyncEnabled: true,
        includeCommonModels: false,
        selectedModelKeys: ["example/chat"],
        excludedCommonModelKeys: [],
        lastSyncAt: null,
        lastSyncError: null,
      }),
    ).toHaveLength(1);
  });
});
