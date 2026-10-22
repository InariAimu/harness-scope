import { expect, it } from "vitest";
import {
  defaultHistoryPair,
  historyMetrics,
  historyPoints,
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

it("breaks curves at account changes, gaps and quota recovery, without fabricating full samples", () => {
  const metric = historyMetrics(custom("20"))[1];
  const values = [
    sample(1, 1000, "80"),
    sample(2, 61000, "70"),
    sample(3, 121000, "85"),
    sample(4, 181000, "80", "two"),
    sample(5, 1000000, "75", "two"),
  ];
  const points = historyPoints(values, metric, 120000);
  expect(points.filter((p) => p.sample).length).toBe(5);
  expect(points.filter((p) => !p.sample).length).toBe(3);
  expect(defaultHistoryPair(points)).toEqual([]);
  expect(points.some((p) => p.value === 100)).toBe(false);
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
