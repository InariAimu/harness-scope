import { expect, it } from "vitest";
import {
  defaultHistoryPair,
  historyMetrics,
  historyPoints,
  historyPlotPoints,
} from "./providerHistory";
import type {
  ProviderAccountObservation,
  ProviderData,
} from "@/types/providerAccounts";

const custom = (percent: string | null): ProviderData => ({
  kind: "customUsage",
  items: [
    {
      id: "shared",
      planName: "Plan",
      unit: "USD",
      remaining: "20",
      used: null,
      total: null,
      usedPercent: percent,
      resetsAt: 10000,
      durationMinutes: 300,
      windowType: "fixed",
    },
  ],
});
const sample = (
  sequence: number,
  at: number,
  remaining: string,
  revision = "one",
): ProviderAccountObservation => ({
  version: 1,
  sequence,
  refreshId: String(sequence),
  accountId: "account",
  accountName: "A",
  accountRevision: revision,
  provider: "custom",
  observedAtMs: at,
  observedAtLocal: "",
  timeZone: "Asia/Shanghai",
  utcOffsetSeconds: 28800,
  source: "refresh",
  data: custom(String(100 - Number(remaining))),
  windows: [
    {
      bucketId: null,
      windowId: "shared",
      usedPercent: 100 - Number(remaining),
      remainingPercent: remaining,
      status: "ok",
      durationMinutes: 300,
      durationSource: "customQuery",
      resetsAt: 10000,
      resetsAtLocal: null,
      startsAt: 0,
      startsAtLocal: null,
      activeAtSample: true,
      fullQuotaAnchor: null,
    },
  ],
});

it("identifies balance and quota capabilities independently for mixed custom data", () => {
  expect(historyMetrics(custom(null)).map((m) => m.kind)).toEqual(["custom"]);
  const mixed = historyMetrics(custom("20"));
  expect(mixed.map((m) => m.kind)).toEqual(["custom", "quota"]);
  expect(mixed[0].key).not.toBe(mixed[1].key);
  expect(historyMetrics()).toEqual([]);
});

it("filters regressions within fixed windows and breaks at account changes and gaps", () => {
  const metric = historyMetrics(custom("20"))[1];
  const values = [
    sample(1, 1000, "80"),
    sample(2, 61000, "70"),
    sample(3, 121000, "85"),
    sample(4, 181000, "80", "two"),
    sample(5, 1000000, "75", "two"),
  ];
  const points = historyPoints(values, metric, 120000);
  expect(points.filter((p) => p.sample).length).toBe(4);
  expect(points.filter((p) => !p.sample).length).toBe(2);
  expect(defaultHistoryPair(points)).toEqual([]);
  expect(points.some((p) => p.value === 100)).toBe(false);
});

it("merges interleaved device readings without changing raw samples or dropping resets", () => {
  const metric = historyMetrics(custom("20"))[1];
  const readings = [80, 70, 75, 74, 60, 65, 98, 97].map((value, i) => ({
    ...sample(20 - i, 1000 + i * 60000, String(value), i % 2 ? "b" : "a"),
    refreshId: `reading-${i}`, subscriptionId: "shared", deviceId: i % 2 ? "b" : "a",
  }));
  readings[6].windows[0].resetsAt = 20000;
  readings[7].windows[0].resetsAt = 20000;
  const before = JSON.stringify(readings);
  const points = historyPoints([...readings].reverse(), metric, 300000);
  expect(points.filter((p) => p.sample).map((p) => p.value)).toEqual([80, 70, 60, 98, 97]);
  expect(defaultHistoryPair(points)).toEqual([14, 13]);
  expect(JSON.stringify(readings)).toBe(before);
  expect(historyPoints(readings, metric, 300000)).toEqual(points);
});

it("does not filter replenishment without a fixed reset window or fabricate out-of-range points", () => {
  const metric = historyMetrics(custom("20"))[1];
  const readings = [sample(1, 1000, "70"), sample(2, 61000, "80")];
  for (const r of readings) {
    if (r.data.kind === "customUsage") r.data.items[0].windowType = "rolling";
  }
  expect(historyPoints(readings, metric, 300000).filter((p) => p.sample)).toHaveLength(2);
  const points = historyPoints([sample(1, 1000, "101"), sample(2, 61000, "-1")], metric, 300000);
  expect(points.map((p) => p.value)).toEqual([null, null]);
});

it("keeps duplicate timestamps and only defaults to the latest continuous segment", () => {
  const metric = historyMetrics(custom("20"))[1];
  const points = historyPoints(
    [sample(3, 181000, "70"), sample(1, 1000, "80"), sample(2, 1000, "80")],
    metric,
    300000,
  );
  expect(points.map((p) => p.sequence)).toEqual([1, 2, 3]);
  expect(defaultHistoryPair(points)).toEqual([1, 3]);
  const changed = sample(4, 241000, "65");
  changed.windows[0].resetsAt = 20000;
  expect(
    defaultHistoryPair(
      historyPoints([...points.map((p) => p.sample!), changed], metric, 300000),
    ),
  ).toEqual([]);
});

it("preserves zero while unknown and expired windows produce gaps", () => {
  const metric = historyMetrics(custom("20"))[1];
  const zero = sample(1, 1000, "0");
  const unknown = sample(2, 61000, "50");
  unknown.windows[0].status = "unknown";
  const expired = sample(3, 10000000, "80");
  const points = historyPoints([zero, unknown, expired], metric, 300000).filter(
    (p) => p.sample,
  );
  expect(points.map((p) => p.value)).toEqual([0, null, null]);
});

it("joins sequential devices by shared subscription and event time, not insertion sequence", () => {
  const metric = historyMetrics(custom("20"))[1];
  const early = { ...sample(9, 1000, "80", "a-revision"), subscriptionId: "shared", deviceId: "a" };
  const late = { ...sample(2, 181000, "70", "b-revision"), subscriptionId: "shared", deviceId: "b" };
  const points = historyPoints([late, early], metric, 300000);
  expect(points.filter((p) => !p.sample)).toHaveLength(0);
  expect(defaultHistoryPair(points)).toEqual([9, 2]);
  const changed = { ...sample(3, 241000, "60", "changed-b"), subscriptionId: "shared", deviceId: "b" };
  expect(defaultHistoryPair(historyPoints([early, late, changed], metric, 300000))).toEqual([]);
});

const cycleSample = (sequence: number, at: number, remaining: string, start: number) => {
  const row = sample(sequence, at, remaining);
  Object.assign(row.windows[0], {
    startsAt: start,
    resetsAt: start + 3600,
    durationMinutes: 60,
    fullQuotaAnchor: { at: start, atLocal: null, remainingPercent: "100", source: "resetMinusDuration", observed: false },
  });
  return row;
};

it("joins chart gaps and unknown readings without changing estimate boundaries", () => {
  const metric = historyMetrics(custom("20"))[1];
  const unknown = sample(2, 61000, "75");
  unknown.windows[0].status = "unknown";
  const points = historyPoints([
    sample(1, 1000, "80"), unknown, sample(3, 1000000, "70", "changed"),
  ], metric, 120000);
  const chart = historyPlotPoints(points, metric, 0);
  expect(chart.map((p) => p.value)).toEqual([80, 70]);
  expect(chart.map((p) => p.sequence)).toEqual([1, 3]);
  expect(defaultHistoryPair(points)).toEqual([]);
});

it("starts each known cycle at 100% without connecting the previous cycle to it", () => {
  const metric = historyMetrics(custom("20"))[1];
  const readings = [
    cycleSample(1, 3540000, "5", 0),
    cycleSample(2, 3660000, "97", 3600),
    cycleSample(3, 4260000, "93", 3600),
  ];
  const before = JSON.stringify(readings);
  const points = historyPoints(readings, metric, 900000);
  const chart = historyPlotPoints(points, metric, 0);
  expect(chart.map((p) => [p.at, p.value])).toEqual([
    [0, 100], [3540000, 5], [3600000, null], [3600000, 100], [3660000, 97], [4260000, 93],
  ]);
  expect(chart.filter((p) => p.resetAnchor)).toHaveLength(2);
  expect(chart.filter((p) => p.resetAnchor).every((p) => !p.sample)).toBe(true);
  expect(chart.filter((p) => p.sample)).toHaveLength(3);
  expect(defaultHistoryPair(points)).toEqual([2, 3]);
  expect(JSON.stringify(readings)).toBe(before);
});

it("deduplicates cycle anchors across devices and respects the selected time range", () => {
  const metric = historyMetrics(custom("20"))[1];
  const a = { ...cycleSample(8, 3660000, "97", 3600), subscriptionId: "shared", deviceId: "a" };
  const b = { ...cycleSample(2, 4260000, "93", 3600), subscriptionId: "shared", deviceId: "b", accountRevision: "b" };
  const points = historyPoints([b, a], metric, 900000);
  const chart = historyPlotPoints(points, metric, 3600000);
  expect(chart.map((p) => p.value)).toEqual([100, 97, 93]);
  expect(historyPlotPoints(points, metric, 3600001).map((p) => p.value)).toEqual([97, 93]);
  expect(defaultHistoryPair(points)).toEqual([8, 2]);
});

it("keeps an actual 100% reset reading selectable without a duplicate inferred point", () => {
  const metric = historyMetrics(custom("20"))[1];
  const readings = [cycleSample(1, 3540000, "5", 0), cycleSample(2, 3600000, "100", 3600), cycleSample(3, 3660000, "97", 3600)];
  const points = historyPoints(readings, metric, 900000);
  const chart = historyPlotPoints(points, metric, 1000);
  expect(chart.map((p) => p.value)).toEqual([5, null, 100, 97]);
  expect(chart[2].sample).toBe(readings[1]);
  expect(chart.some((p) => p.resetAnchor)).toBe(false);
  expect(defaultHistoryPair(points)).toEqual([2, 3]);
});

it("does not invent anchors for unknown, future or inconsistent window metadata", () => {
  const metric = historyMetrics(custom("20"))[1];
  const valid = cycleSample(1, 3660000, "97", 3600);
  const missing = structuredClone(valid); missing.windows[0].fullQuotaAnchor = null;
  const inconsistent = structuredClone(valid); inconsistent.windows[0].fullQuotaAnchor!.at += 1;
  const inactive = structuredClone(valid); inactive.windows[0].activeAtSample = false;
  const future = cycleSample(1, 3000000, "97", 3600);
  for (const row of [missing, inconsistent, inactive, future]) {
    expect(historyPlotPoints(historyPoints([row], metric, 900000), metric, 0).some((p) => p.resetAnchor)).toBe(false);
  }
  const balance = historyMetrics(custom("20"))[0];
  expect(historyPlotPoints(historyPoints([valid], balance, 900000), balance, 0).some((p) => p.resetAnchor)).toBe(false);
});
