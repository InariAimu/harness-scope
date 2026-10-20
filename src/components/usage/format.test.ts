import { expect, it } from "vitest";
import {
  formatFirstTokenSeconds,
  getEstimatedTokensPerSecond,
  getOutputTokensPerSecond,
} from "./format";
import type { SessionSpeedEstimate } from "@/types/usage";

it("calculates proxy generation speed after the first token", () => {
  expect(
    getOutputTokensPerSecond({
      dataSource: "proxy",
      outputTokens: 400,
      latencyMs: 3000,
      firstTokenMs: 1000,
    }),
  ).toBe(200);
  expect(formatFirstTokenSeconds(1250)).toBe("1.3s");
});
it("keeps missing or invalid timing unknown instead of reporting zero speed", () => {
  expect(
    getOutputTokensPerSecond({
      dataSource: "harness_scope",
      outputTokens: 400,
      latencyMs: null,
      firstTokenMs: null,
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      dataSource: "harness_scope",
      outputTokens: 400,
      latencyMs: null,
    }),
  ).toBeNull();
  expect(formatFirstTokenSeconds(null)).toBeNull();
  expect(
    getOutputTokensPerSecond({
      dataSource: "proxy",
      outputTokens: 400,
      latencyMs: 3000,
      firstTokenMs: -1000,
    }),
  ).toBeNull();
});
it("labels the whole session window as an estimate and applies its sample thresholds", () => {
  expect(
    getOutputTokensPerSecond({
      dataSource: "codex_session",
      outputTokens: 400,
      latencyMs: 3000,
      firstTokenMs: 1000,
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      dataSource: "codex_session",
      outputTokens: 400,
      latencyMs: 2000,
    }),
  ).toBe(200);
  expect(
    getEstimatedTokensPerSecond({
      dataSource: "codex_session",
      outputTokens: 199,
      latencyMs: 2000,
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      dataSource: "codex_session",
      outputTokens: 400,
      latencyMs: 999,
    }),
  ).toBeNull();
  expect(
    getOutputTokensPerSecond({
      dataSource: "proxy",
      outputTokens: 99,
      latencyMs: 2000,
      firstTokenMs: 1000,
    }),
  ).toBeNull();
  expect(
    getOutputTokensPerSecond({
      dataSource: "proxy",
      outputTokens: 400,
      latencyMs: 1099,
      firstTokenMs: 1000,
    }),
  ).toBeNull();
});

it("uses the DSH output window with conservative sample thresholds", () => {
  const estimatedSpeed: SessionSpeedEstimate = {
    basis: "decode",
    durationMs: 2482,
    eligible: true,
    minOutputTokens: 200,
    minDurationMs: 1000,
  };
  const row = {
    dataSource: "harness_scope",
    outputTokens: 488,
    latencyMs: 6846,
    firstTokenMs: 4364,
    estimatedSpeed,
  };
  expect(getEstimatedTokensPerSecond(row)).toBeCloseTo(196.6156, 3);
  expect(getOutputTokensPerSecond(row)).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      outputTokens: 122,
      estimatedSpeed: { ...estimatedSpeed, durationMs: 494 },
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      outputTokens: 200,
      estimatedSpeed: { ...estimatedSpeed, durationMs: 1000 },
    }),
  ).toBe(200);
  for (const [outputTokens, durationMs] of [
    [124, 160], // Observed 775 tok/s burst.
    [1311, 778], // Token count alone cannot exclude buffered delivery.
    [199, 2000],
    [200, 999],
  ]) {
    expect(
      getEstimatedTokensPerSecond({
        ...row,
        outputTokens,
        estimatedSpeed: { ...estimatedSpeed, durationMs },
      }),
    ).toBeNull();
  }
});

it("never substitutes total latency for an excluded or invalid output window", () => {
  const row = {
    dataSource: "harness_scope",
    outputTokens: 488,
    latencyMs: 6846,
    estimatedSpeed: {
      basis: "decode" as const,
      durationMs: 29,
      eligible: false,
      minOutputTokens: 200,
      minDurationMs: 1000,
    },
  };
  expect(getEstimatedTokensPerSecond(row)).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      estimatedSpeed: { ...row.estimatedSpeed, eligible: true, durationMs: 0 },
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      estimatedSpeed: { ...row.estimatedSpeed, eligible: true, durationMs: 999 },
    }),
  ).toBeNull();
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      outputTokens: 199,
      estimatedSpeed: {
        ...row.estimatedSpeed,
        eligible: true,
        durationMs: 1000,
      },
    }),
  ).toBeNull();
});

it("keeps explicit and legacy end-to-end estimates on the same basis", () => {
  const row = {
    dataSource: "harness_scope",
    outputTokens: 400,
    latencyMs: 3000,
    firstTokenMs: 1000,
  };
  expect(getEstimatedTokensPerSecond(row)).toBeCloseTo(133.3333, 3);
  expect(
    getEstimatedTokensPerSecond({
      ...row,
      estimatedSpeed: {
        basis: "end_to_end",
        durationMs: 3000,
        eligible: true,
        minOutputTokens: 200,
        minDurationMs: 1000,
      },
    }),
  ).toBeCloseTo(133.3333, 3);
});
