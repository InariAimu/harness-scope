import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import type { SpeedDistribution, SpeedTrends } from "@/types/speed";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import { SpeedPlot } from "./SpeedDashboard";
import { speedChartLayout, speedDirection, SPEED_OTHER, SPEED_UP } from "./speedChart";

const distribution = (mean: number | null): SpeedDistribution => mean == null
  ? { sampleCount: 0, mean: null, min: null, max: null, centralLower: null, centralUpper: null, centralSampleCount: 0 }
  : { sampleCount: 8, mean, min: mean / 3, max: mean * 2, centralLower: mean / 2, centralUpper: mean * 1.5, centralSampleCount: 6 };
const data: SpeedTrends = {
  startDate: 0, endDate: 1799, bucketSeconds: 300, selectedRecordCount: 40, excludedRecordCount: 0,
  summary: { speed: distribution(40), ttft: distribution(2), estimatedSampleCount: 40 },
  buckets: [40, 60, null, 20, 20, 10].map((mean, index) => ({
    start: index * 300, end: (index + 1) * 300,
    speed: distribution(mean), ttft: distribution(mean == null ? null : 100 / mean), estimatedSampleCount: mean == null ? 0 : 8,
  })),
};
describe("speed chart's distribution geometry", () => {
  it("connects only contiguous populated intervals but compares direction across gaps", () => {
    const chart = speedChartLayout(data, "speed", 700);
    expect(chart.segments.map(s => [s.from, s.to])).toEqual([[0, 1], [3, 4], [4, 5]]);
    expect(chart.bars.map(b => b.color)).toEqual([SPEED_OTHER, SPEED_UP, SPEED_OTHER, SPEED_OTHER, SPEED_OTHER]);
    expect(speedDirection(data.buckets, 3, "speed")).toBe("down");
    expect(speedDirection(data.buckets, 3, "ttft")).toBe("up");
    expect(speedDirection(data.buckets, 4, "speed")).toBe("equal");
    // An increase in TTFT is still red; color is numeric direction, not quality.
    expect(speedDirection(data.buckets, 5, "ttft")).toBe("up");
    const missingInterval = { ...data, buckets: data.buckets.filter((_, i) => i !== 2) };
    expect(speedChartLayout(missingInterval, "speed", 700).segments).toHaveLength(3);
    expect(speedDirection(missingInterval.buckets, 2, "speed")).toBe("down");
  });
  it("uses the most recent valid mean after multiple empty intervals, including rises and ties", () => {
    const resumed: SpeedTrends = { ...data, endDate: 2699,
      buckets: [null, 40, null, null, 60, null, 60, null, 20].map((mean, index) => ({
        start: index * 300, end: (index + 1) * 300,
        speed: distribution(mean), ttft: distribution(mean == null ? null : 100 / mean), estimatedSampleCount: 0,
      })),
    };
    expect([1, 4, 6, 8].map(i => speedDirection(resumed.buckets, i, "speed"))).toEqual(["first", "up", "equal", "down"]);
    expect(speedChartLayout(resumed, "speed", 700).bars.map(b => b.color)).toEqual([SPEED_OTHER, SPEED_UP, SPEED_OTHER, SPEED_OTHER]);
    expect(speedChartLayout(resumed, "speed", 700).segments).toHaveLength(0);
  });
  it("uses observed central endpoints and full extrema at narrow and desktop widths", () => {
    for (const width of [260, 700]) {
      const chart = speedChartLayout(data, "speed", width);
      const first = chart.bars[0];
      expect(first.upperY).toBe(chart.y(60));
      expect(first.lowerY).toBe(chart.y(20));
      expect(first.maxY).toBe(chart.y(80));
      expect(first.minY).toBe(chart.y(40 / 3));
      expect(chart.bars.every(b => b.center >= chart.left && b.center <= width - chart.right)).toBe(true);
      expect(chart.bars.every(b => b.maxY >= chart.top && b.minY <= 180 - chart.bottom)).toBe(true);
    }
    const one = { ...data, buckets: [data.buckets[0]] };
    expect(speedChartLayout(one, "speed", 700).isolated).toHaveLength(1);
    const empty = { ...data, buckets: [data.buckets[2]] };
    expect(speedChartLayout(empty, "speed", 700).bars).toEqual([]);
  });
});

it("renders solid red/green bodies, no median, unknown gaps, and both languages", async () => {
  const translations = createInstance();
  await translations.init({ lng: "zh", resources: { zh: { translation: zh }, en: { translation: en } } });
  const client = new QueryClient({ defaultOptions: { queries: { enabled: false, gcTime: Infinity } } });
  client.setQueryData(["snapshot"], { config: { timezone: "Asia/Shanghai" } });
  const render = (active: number | null) => renderToStaticMarkup(createElement(I18nextProvider, { i18n: translations },
    createElement(QueryClientProvider, { client }, createElement(SpeedPlot, { data, metric: "speed", active, onActive: () => {} }))));
  const chinese = render(null);
  expect(chinese).toContain("加权平均");
  expect(chinese).toMatch(/<rect[^>]*fill="var\(--danger\)"[^>]*data-testid="speed-body"/);
  const meanLines = chinese.match(/<line[^>]*data-testid="speed-mean-line"[^>]*>/g)!;
  expect(meanLines).toHaveLength(3);
  expect(meanLines.every(line => line.includes('stroke="var(--chart-mean)"'))).toBe(true);
  expect(chinese).not.toMatch(/fill-opacity|stroke-dasharray|中位数/);
  expect(render(2)).toContain("此时段无有效样本");
  expect(render(1)).toContain("覆盖 6 / 8 个样本");
  await translations.changeLanguage("en");
  expect(render(null)).toContain("Weighted mean");
  expect(render(2)).toContain("No valid samples in this interval");
  client.clear();
});
