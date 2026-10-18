import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PerformanceCells, getStatsEstimatedSpeed } from "./statsColumns";

describe("estimated performance statistics", () => {
  it("uses weighted totals and marks speed and first token as estimates", () => {
    const stat = {
      successRate: null,
      estSpeedOutputTokens: 1200,
      estSpeedDurationMs: 10000,
      estFirstTokenMedianMs: 250,
    };
    expect(getStatsEstimatedSpeed(stat)).toBe("120");
    const html = renderToStaticMarkup(
      createElement(PerformanceCells, { stat }),
    );
    expect(html).toContain("—"); // unknown status is not 0% and cannot crash the tab
    expect(html).toContain("≈120");
    expect(html).toContain("≈0.3s");
  });
  it("does not fabricate timing for an empty sample", () => {
    const html = renderToStaticMarkup(
      createElement(PerformanceCells, { stat: { successRate: null } }),
    );
    expect(html.match(/—/g)).toHaveLength(3);
    expect(html).not.toContain("≈");
  });
});
