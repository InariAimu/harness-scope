import { expect, it } from "vitest";
import {
  formatFirstTokenSeconds,
  getEstimatedTokensPerSecond,
  getOutputTokensPerSecond,
} from "./format";

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
