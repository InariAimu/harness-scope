import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it } from "vitest";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "@/i18n/locales/en.json";
import {
  PerformanceCells,
  getStatsEstimatedSpeed,
  getStatsEstimatedSpeedBasis,
} from "./statsColumns";
const translations = createInstance();
beforeAll(async () => {
  await translations
    .use(initReactI18next)
    .init({ lng: "en", resources: { en: { translation: en } } });
});
const renderCells = (stat: Parameters<typeof PerformanceCells>[0]["stat"]) =>
  renderToStaticMarkup(
    createElement(
      I18nextProvider,
      { i18n: translations },
      createElement(PerformanceCells, { stat }),
    ),
  );

describe("estimated performance statistics", () => {
  it("shows eligible sample counts for E2E modes and keeps missing timing unknown", () => {
    const render = (count: number) => renderToStaticMarkup(createElement(I18nextProvider,
      { i18n: translations }, createElement(PerformanceCells, { showSampleCount: true,
        stat: { successRate: null, estSpeedOutputTokens: count ? 1000 : 0,
          estSpeedDurationMs: count ? 5000 : 0, estSpeedSampleCount: count } })));
    expect(render(81)).toContain("81 samples used");
    expect(render(81)).toContain("E2E");
    expect(render(0)).toContain("0 samples used");
    expect(render(0)).not.toContain("≈");
  });
  it("uses weighted totals and marks speed and first token as estimates", () => {
    const stat = {
      successRate: null,
      estSpeedOutputTokens: 1200,
      estSpeedDurationMs: 10000,
      estFirstTokenMedianMs: 250,
    };
    expect(getStatsEstimatedSpeed(stat)).toBe("120");
    const html = renderCells(stat);
    expect(html).toContain("—"); // unknown status is not 0% and cannot crash the tab
    expect(html).toContain("≈120");
    expect(html).toContain("E2E");
    expect(html).toContain("≈0.3s");
  });
  it("does not fabricate timing for an empty sample", () => {
    const html = renderCells({ successRate: null });
    expect(html.match(/—/g)).toHaveLength(3);
    expect(html).not.toContain("≈");
  });
  it("prefers weighted output-phase totals and keeps end-to-end samples separate", () => {
    const stat = {
      successRate: null,
      estDecodeSpeedOutputTokens: 15585,
      estDecodeSpeedGenerationMs: 61639,
      estDecodeSpeedSampleCount: 28,
      estSpeedOutputTokens: 1200,
      estSpeedDurationMs: 10000,
      estSpeedSampleCount: 2,
    };
    expect(getStatsEstimatedSpeed(stat)).toBe("253");
    expect(getStatsEstimatedSpeedBasis(stat)).toBe("decode");
    const html = renderCells(stat);
    expect(html).toContain("≈253");
    expect(html).toContain("28 samples used");
    expect(html).toContain("2 E2E samples kept separate");
    expect(html).not.toContain('text-fg-3">E2E</span>');
    expect(
      getStatsEstimatedSpeed({ ...stat, estDecodeSpeedGenerationMs: 0 }),
    ).toBe("120");
    expect(
      getStatsEstimatedSpeedBasis({ ...stat, estDecodeSpeedGenerationMs: 0 }),
    ).toBe("end_to_end");
  });
});
