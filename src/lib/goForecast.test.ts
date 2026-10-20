import { expect, it } from "vitest";
import { forecastDuration, forecastNumber, forecastReason } from "./goForecast";

it("shows bounded time through reset without inventing infinite lifetime", () => {
  expect(forecastDuration(0, true)).toBe("0 秒");
  expect(forecastDuration(61, true)).toBe("2分钟");
  expect(forecastDuration(90061, true)).toBe("1天 1小时");
  expect(forecastDuration(1800, false)).toBe("30m");
  expect(forecastDuration(Infinity, true)).toBe("—");
  expect(forecastDuration(-1, false)).toBe("—");
});
it("preserves small positive budgets and explains missing quota or samples", () => {
  expect(forecastNumber("0.000031", "en-US")).toBe("0.000031");
  expect(forecastNumber("1200", "en-US")).toBe("1,200");
  expect(forecastNumber("12.5", "en-US")).toBe("12.5");
  expect(forecastNumber("invalid", "en-US")).toBe("—");
  expect(forecastReason("stale_quota", true)).toContain("10 分钟");
  expect(forecastReason("insufficient_samples", false)).toContain("3 requests");
  expect(forecastReason("unpriced_since_snapshot", true)).toContain("快照之后");
});
