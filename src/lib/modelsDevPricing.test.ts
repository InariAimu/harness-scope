import { describe, it, expect } from "vitest";
import {
  flattenModels,
  toModelPricing,
  resolveModelsDevSelection,
} from "./modelsDevPricing";
describe("models.dev import provenance", () => {
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
