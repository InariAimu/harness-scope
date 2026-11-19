import { describe, expect, it } from "vitest";
import { TREND_BAR_ACTIVE_SCALE, trendBarGeometry, trendBarSpan } from "./trendBarShape";

describe("rounded model stack geometry", () => {
  it("recovers the same rounded column for its bottom, middle and thin top segments", () => {
    const bottom = trendBarGeometry({ x: 10, y: 80, width: 36, height: 40 }, 0, 40, 100)!;
    const middle = trendBarGeometry({ x: 10, y: 22, width: 36, height: 58 }, 40, 98, 100)!;
    const top = trendBarGeometry({ x: 10, y: 20, width: 36, height: 2 }, 98, 100, 100)!;
    expect(bottom.column).toEqual({ x: 10, y: 20, width: 36, height: 100 });
    expect(middle.column).toEqual(bottom.column);
    expect(top.column).toEqual(bottom.column);
    expect(top.clipPath).toBe(bottom.clipPath);
    expect(top.radius).toBe(6);
    expect(top.segmentPath).toContain("A 6 6");
    expect(middle.segmentPath).toContain("A 6 6");
    expect(bottom.segmentPath).not.toContain("A ");
  });

  it("clips a thin top slice against the full-radius circular cap", () => {
    const column = { x: 10, y: 20, width: 36, height: 100 };
    expect(trendBarSpan(column, 6, 20)).toEqual({ left: 16, right: 40 });
    const thin = trendBarSpan(column, 6, 22);
    expect(thin.left).toBeCloseTo(16 - Math.sqrt(20));
    expect(thin.right).toBeCloseTo(40 + Math.sqrt(20));
    expect(trendBarSpan(column, 6, 26)).toEqual({ left: 10, right: 46 });
  });

  it("scales around the segment center without changing layout or values", () => {
    const rect = { x: 10, y: 50, width: 36, height: 30 };
    const original = { ...rect };
    const geometry = trendBarGeometry(rect, 40, 70, 100)!;
    expect(TREND_BAR_ACTIVE_SCALE).toBe(1.05);
    const matrix = geometry.activeTransform.match(/[-\d.]+/g)!.map(Number);
    expect(matrix[0]).toBe(1.05);
    expect(matrix[4]).toBeCloseTo(-1.4);
    expect(matrix[5]).toBeCloseTo(-3.25);
    expect(rect).toEqual(original);
  });

  it("keeps the enlarged bottom block and its outline above a fixed baseline", () => {
    const geometry = trendBarGeometry({ x: 10, y: 80, width: 36, height: 40 }, 0, 40, 100)!;
    expect(geometry.baseline).toBe(120);
    expect(geometry.hoverBounds.y + geometry.hoverBounds.height).toBeCloseTo(120);
    expect(geometry.baselineClip.y + geometry.baselineClip.height).toBeCloseTo(120);
    expect(geometry.hoverBounds.width).toBeCloseTo(36 * 1.05 + 2);
    expect(geometry.hoverBounds.height).toBeCloseTo(40 * 1.05 + 2);
  });

  it("bounds the radius for narrow and short columns and rejects invalid geometry", () => {
    expect(trendBarGeometry({ x: 0, y: 0, width: 4, height: 100 }, 0, 100, 100)?.radius).toBe(2);
    expect(trendBarGeometry({ x: 0, y: 0, width: 36, height: 2 }, 0, 2, 2)?.radius).toBe(2);
    for (const [height, start, end, total] of [[0, 0, 10, 10], [10, 5, 5, 10], [10, 0, 10, 9], [10, -1, 10, 10], [Infinity, 0, 10, 10]]) {
      expect(trendBarGeometry({ x: 0, y: 0, width: 36, height }, start, end, total)).toBeNull();
    }
  });

  it("keeps the top cost segment when decimal addition differs only by roundoff", () => {
    expect(trendBarGeometry({ x: 0, y: 10, width: 36, height: 20 }, 0.1, 0.1 + 0.2, 0.3)).not.toBeNull();
  });
});
