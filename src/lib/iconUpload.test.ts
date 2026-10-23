import { describe, expect, it } from "vitest";
import { cropRectangle, DEFAULT_ICON_CROP } from "./iconUpload";

describe("icon crop geometry", () => {
  it("center-crops wide and tall images before reducing to 128 pixels", () => {
    expect(cropRectangle(800, 400, DEFAULT_ICON_CROP)).toEqual({ x: 200, y: 0, size: 400, output: 128 });
    expect(cropRectangle(400, 800, DEFAULT_ICON_CROP)).toEqual({ x: 0, y: 200, size: 400, output: 128 });
  });
  it("preserves small icons without upscaling", () => {
    expect(cropRectangle(32, 48, DEFAULT_ICON_CROP)).toEqual({ x: 0, y: 8, size: 32, output: 32 });
  });
  it("fits entire artwork with transparent padding and no distortion", () => {
    expect(cropRectangle(800, 400, { ...DEFAULT_ICON_CROP, mode: "fit" })).toEqual({ x: 0, y: -200, size: 800, output: 128 });
  });
  it("keeps zoomed and repositioned crops within the source", () => {
    expect(cropRectangle(800, 400, { mode: "crop", zoom: 2, x: 100, y: 0 })).toEqual({ x: 600, y: 0, size: 200, output: 128 });
    for (const [width, height] of [[1, 2], [200, 800], [16384, 128]]) {
      for (const position of [-500, 0, 50, 100, 500, NaN]) {
        const rect = cropRectangle(width, height, { mode: "crop", zoom: 4, x: position, y: position });
        expect(rect.x).toBeGreaterThanOrEqual(0);
        expect(rect.y).toBeGreaterThanOrEqual(0);
        expect(rect.x + rect.size).toBeLessThanOrEqual(width);
        expect(rect.y + rect.size).toBeLessThanOrEqual(height);
        expect(rect.output).toBeGreaterThanOrEqual(1);
        expect(rect.output).toBeLessThanOrEqual(128);
      }
    }
  });
  it("rejects missing or invalid dimensions", () => {
    expect(() => cropRectangle(0, 10, DEFAULT_ICON_CROP)).toThrow();
    expect(() => cropRectangle(NaN, 10, DEFAULT_ICON_CROP)).toThrow();
  });
});
