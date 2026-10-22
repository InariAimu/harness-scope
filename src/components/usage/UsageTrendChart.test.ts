import { describe, expect, it } from "vitest";
import {
  buildUsageTrendChartData,
  formatUsageTrendTickLabel,
} from "./UsageTrendChart";

const stat = (date: string, requests = 0) => ({
  date,
  requestCount: requests,
  totalInputTokens: requests * 10,
  totalOutputTokens: requests * 2,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
  totalCost: "0",
});
const ts = (s: string) => Date.parse(s) / 1000;
describe("continuous trend chart categories", () => {
  it("keeps empty hours and formats midnight in the configured timezone", () => {
    const points = buildUsageTrendChartData(
      [
        stat("2026-10-19T00:00:00+08:00"),
        stat("2026-10-19T01:00:00+08:00", 1),
        stat("2026-10-19T02:00:00+08:00"),
      ],
      {
        isHourly: true,
        dateLocale: "zh-CN",
        timeZone: "Asia/Shanghai",
        startDate: ts("2026-10-19T00:00:00+08:00"),
        endDate: ts("2026-10-19T02:30:00+08:00"),
      },
    );
    expect(points).toHaveLength(3);
    expect(points[0].hour).toBe(0);
    expect(points[0].label).toContain("00:00");
    expect(points.map((p) => p.tokens)).toEqual([0, 12, 0]);
    expect(points.map((p) => p.requests)).toEqual([0, 1, 0]);
  });
  it("keeps the calendar date when the source offset differs from the browser zone", () => {
    const points = buildUsageTrendChartData(
      [stat("2026-01-01T00:00:00+14:00")],
      {
        isHourly: false,
        dateLocale: "en-US",
        timeZone: "Pacific/Kiritimati",
        startDate: ts("2025-12-29T00:00:00+14:00"),
        endDate: ts("2026-01-04T12:00:00+14:00"),
      },
    );
    expect(points[0].tooltipLabel).toBe("01/01/2026");
    expect(points[0].label).toBe("01/01/26");
    expect(formatUsageTrendTickLabel(points[0].xKey, points)).toBe(
      points[0].label,
    );
  });
  it("distinguishes repeated DST hours by category and tooltip offset", () => {
    const points = buildUsageTrendChartData(
      [
        { ...stat("2026-11-01T01:00:00-04:00", 1), modelBreakdown: [{ model: "first", requestCount: 1, totalTokens: 12, totalCost: "0" }] },
        { ...stat("2026-11-01T01:00:00-05:00", 1), modelBreakdown: [{ model: "second", requestCount: 1, totalTokens: 12, totalCost: "0" }] },
      ],
      {
        isHourly: true,
        dateLocale: "en-US",
        timeZone: "America/New_York",
        startDate: ts("2026-11-01T00:00:00-04:00"),
        endDate: ts("2026-11-01T04:30:00-05:00"),
      },
    );
    expect(points[0].xKey).not.toBe(points[1].xKey);
    expect(points[0].tooltipLabel).not.toBe(points[1].tooltipLabel);
    expect(points[0].modelBreakdown?.[0].model).toBe("first");
    expect(points[1].modelBreakdown?.[0].model).toBe("second");
  });
});
