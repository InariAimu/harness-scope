import { describe, expect, it } from "vitest";
import { placeTrendTooltip, TREND_TOOLTIP_GAP } from "./trendTooltip";

describe("model stack tooltip clearance", () => {
  const size = { width: 260, height: 220 };
  const viewport = { width: 1000, height: 800 };

  it("reserves the complete enlarged block, rather than just a cursor offset", () => {
    const block = { x: 400, y: 300, width: 42, height: 80 };
    const right = placeTrendTooltip({ x: 422, y: 330 }, size, block, viewport);
    expect(right.side).toBe("right");
    expect(right.x).toBe(block.x + block.width + TREND_TOOLTIP_GAP);
    const left = placeTrendTooltip({ x: 410, y: 330 }, size, block, viewport);
    expect(left.side).toBe("left");
    expect(left.x + size.width).toBe(block.x - TREND_TOOLTIP_GAP);
  });

  it("follows pointer movement while maintaining block clearance", () => {
    const block = { x: 400, y: 250, width: 42, height: 180 };
    const before = placeTrendTooltip({ x: 425, y: 280 }, size, block, viewport);
    const after = placeTrendTooltip({ x: 425, y: 340 }, size, block, viewport);
    expect(after.y - before.y).toBe(60);
    expect(after.x).toBe(before.x);
  });

  it("flips away from either viewport edge and clamps vertical placement", () => {
    const right = placeTrendTooltip({ x: 975, y: 790 }, size, { x: 950, y: 700, width: 42, height: 80 }, viewport);
    expect(right.side).toBe("left");
    expect(right.y + size.height).toBe(viewport.height - 8);
    const left = placeTrendTooltip({ x: 12, y: 5 }, size, { x: 10, y: 0, width: 42, height: 80 }, viewport);
    expect(left.side).toBe("right");
    expect(left.y).toBe(8);
  });

  it("uses vertical clearance when neither horizontal side fits", () => {
    const block = { x: 180, y: 400, width: 42, height: 80 };
    const top = placeTrendTooltip({ x: 202, y: 440 }, size, block, { width: 400, height: 800 });
    expect(top.side).toBe("above");
    expect(top.y + size.height).toBe(block.y - TREND_TOOLTIP_GAP);
    const bottom = placeTrendTooltip({ x: 202, y: 40 }, size, { ...block, y: 0 }, { width: 400, height: 800 });
    expect(bottom.side).toBe("below");
    expect(bottom.y).toBe(80 + TREND_TOOLTIP_GAP);
  });
});
