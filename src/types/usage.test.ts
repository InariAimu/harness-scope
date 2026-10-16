import { expect, it } from "vitest";
import { getFreshInputTokens, isUnpricedUsage, type RequestLog } from "./usage";
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
