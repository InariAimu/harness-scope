import { expect, it } from "vitest";
import {
  getFreshInputTokens,
  isFreeUsage,
  isUnpricedUsage,
  type RequestLog,
} from "./usage";
it("does not subtract cache from already normalized Codex input", () => {
  expect(
    getFreshInputTokens({
      appType: "codex",
      inputTokens: 90,
      cacheReadTokens: 20,
    }),
  ).toBe(90);
});
it("distinguishes an unknown amount from known zero", () => {
  const row = {
    inputTokens: 10,
    outputTokens: 1,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    totalCostUsd: "0",
    statusCode: 200,
  } as RequestLog;
  expect(isUnpricedUsage(row)).toBe(false);
  expect(
    isUnpricedUsage({ ...row, totalCostUsd: null as unknown as string }),
  ).toBe(true);
});

const freeRequest = {
  nativeProvider: "opencode",
  model: "deepseek-v4.1-flash-free",
  costSource: "reported",
  totalCostUsd: "0",
  inputTokens: 10,
  outputTokens: 1,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
} as RequestLog;

it("recognizes reported OpenCode free models without changing their names", () => {
  for (const model of [
    "deepseek-v4.1-flash-free",
    "longcat-2.0-free",
    "opencode/deepseek-v4.1-flash-free",
  ])
    expect(isFreeUsage({ ...freeRequest, model })).toBe(true);
  expect(
    isFreeUsage({
      ...freeRequest,
      nativeProvider: "opencode-go",
      model: "beam-free",
      billingBasis: "subscription_usage",
    }),
  ).toBe(true);
});

it("does not confuse unknown, tiny, plan, or paid zero amounts with free models", () => {
  const cases: Partial<RequestLog>[] = [
    { totalCostUsd: null as unknown as string },
    { totalCostUsd: "0.000001" },
    { totalCostUsd: "1e-999" },
    { totalCostUsd: "0garbage" },
    { totalCostUsd: "" },
    { model: "deepseek-v4.1-flash" },
    { nativeProvider: "custom" },
    { nativeProvider: undefined },
    { costSource: "unknown" },
    { model: "deepseek-v4.1-flash", billingBasis: "subscription_usage" },
    { billingBasis: "model_reference" },
    { inputTokens: 0, outputTokens: 0 },
  ];
  for (const patch of cases)
    expect(isFreeUsage({ ...freeRequest, ...patch })).toBe(false);
});

it("recognizes an applied zero tariff but requires every rate to be explicitly zero", () => {
  const pricingSnapshot = {
    inputCostPerMillion: "0.000",
    outputCostPerMillion: "0",
    cacheReadCostPerMillion: "0",
    cacheCreationCostPerMillion: "0",
  };
  const request = {
    ...freeRequest,
    nativeProvider: "custom",
    model: "local-model",
    costSource: "estimated",
    pricingSnapshot,
  };
  expect(isFreeUsage(request)).toBe(true);
  for (const patch of [
    { outputCostPerMillion: "1" },
    { inputCostPerMillion: "0.000000001" },
    { cacheReadCostPerMillion: undefined },
    { pricingRule: "dynamic-price" },
  ])
    expect(
      isFreeUsage({ ...request, pricingSnapshot: { ...pricingSnapshot, ...patch } }),
    ).toBe(false);
  expect(isFreeUsage({ ...request, pricingSnapshot: null })).toBe(false);
  expect(isFreeUsage({ ...request, costSource: "reported" })).toBe(false);
  expect(isFreeUsage({ ...request, billingBasis: "model_reference" })).toBe(true);
  expect(isFreeUsage({ ...request, billingBasis: "subscription_usage" })).toBe(true);
});

it("shows a DSH free model from a zero models.dev reference snapshot as free", () => {
  const request: RequestLog = {
    ...freeRequest,
    appType: "dsh",
    nativeProvider: "opencode-go-plus",
    model: "beam-free",
    costSource: "estimated",
    priceSource: "models_dev",
    billingBasis: "model_reference",
    pricingSnapshot: {
      modelId: "beam-free",
      displayName: "Beam 1.0 (Free)",
      source: "models_dev",
      sourceKey: "vercel/reflection/beam-free",
      inputCostPerMillion: "0",
      outputCostPerMillion: "0",
      cacheReadCostPerMillion: "0",
      cacheCreationCostPerMillion: "0",
    },
  };
  expect(isFreeUsage(request)).toBe(true);
  expect(isFreeUsage({ ...request, totalCostUsd: "0.000001" })).toBe(false);
  expect(
    isFreeUsage({
      ...request,
      pricingSnapshot: {
        ...(request.pricingSnapshot as object),
        outputCostPerMillion: "1",
      },
    }),
  ).toBe(false);
});
