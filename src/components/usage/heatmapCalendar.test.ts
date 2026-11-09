import { describe, expect, it } from "vitest";
import { buildCalendarHeatmap, calendarDayKey, calendarLevels, calendarSpanCount, calendarSpanStart } from "./heatmapCalendar";
import { dailySpeedMean, sumSpeedDays } from "./speedDaily";

describe("shared calendar heatmap", () => {
  it("keeps 53-week spans contiguous across leap years and leaves missing/future days empty", () => {
    const today = new Date(2024, 2, 10);
    const days = [{date:"2024-02-29",value:60},{date:"2022-01-01",value:20}];
    const spans = buildCalendarHeatmap(days, day=>day.value, today);
    const cells = spans.flatMap(span=>span.weeks.flat());
    expect(spans).toHaveLength(calendarSpanCount("2022-01-01", today));
    expect(new Set(cells.map(cell=>cell.key)).size).toBe(cells.length);
    expect(cells.find(cell=>cell.key==="2024-02-29")?.value).toBe(60);
    expect(cells.find(cell=>cell.key==="2024-03-01")?.value).toBeNull();
    expect(calendarDayKey(calendarSpanStart(today, 0))).toBe("2023-03-06");
    expect(calendarDayKey(spans[1].end)).toBe("2023-03-05");
    expect(cells.filter(cell=>cell.datum)).toHaveLength(2);
  });
  it("darkens higher speeds and lower TTFT while distinguishing missing samples from zero", () => {
    const levels = calendarLevels([null,10,20,30,40,50,60,70,80]);
    expect(levels(null)).toBe(0);
    expect(levels(80)).toBeGreaterThan(levels(10));
    const latency = calendarLevels([null,0,1,2,3,4,5,6,7], true);
    expect(latency(0)).toBe(4);
    expect(latency(7)).toBe(1);
    expect(latency(null)).toBe(0);
    expect(calendarLevels([null])(null)).toBe(0);
  });
});

it("combines additive daily totals rather than averaging daily means", () => {
  const totals = sumSpeedDays([
    {outputTokens:400,generationMs:20000,firstTokenMs:1000,sampleCount:1,estimatedSampleCount:1},
    {outputTokens:1000,generationMs:10000,firstTokenMs:0,sampleCount:1,estimatedSampleCount:0},
  ]);
  expect(dailySpeedMean(totals,"speed")).toBeCloseTo(1400/30);
  expect(dailySpeedMean(totals,"ttft")).toBe(.5);
  expect(dailySpeedMean(sumSpeedDays([]),"speed")).toBeNull();
  expect(dailySpeedMean({...totals,firstTokenMs:0},"ttft")).toBe(0);
  expect(totals.estimatedSampleCount).toBe(1);
});
