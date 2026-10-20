import { describe, expect, it } from "vitest";
import { skinAppearanceVariables, skinPercentage } from "./skinAppearance";

describe("skin appearance", () => {
  it("preserves the original light, dark and narrow gradients for old configs", () => {
    const vars = skinAppearanceVariables(undefined, undefined);
    expect(vars["--skin-opacity"]).toBe("1");
    expect(vars["--skin-veil-light-25"]).toBe("96%");
    expect(vars["--skin-veil-light-60"]).toBe("80%");
    expect(vars["--skin-veil-light-end"]).toBe("66%");
    expect(vars["--skin-veil-dark-25"]).toBe("97%");
    expect(vars["--skin-veil-dark-60"]).toBe("86%");
    expect(vars["--skin-veil-dark-end"]).toBe("76%");
    expect(vars["--skin-veil-narrow-light"]).toBe("92%");
    expect(vars["--skin-veil-narrow-dark"]).toBe("95%");
  });

  it("independently fades the character and reaches clear/opaque veil endpoints", () => {
    expect(skinAppearanceVariables(100, 80)["--skin-opacity"]).toBe("0");
    expect(skinAppearanceVariables(35, 80)["--skin-opacity"]).toBe("0.65");
    for (const [key, value] of Object.entries(skinAppearanceVariables(0, 0))) {
      if (key !== "--skin-opacity") expect(value).toBe("0%");
    }
    for (const [key, value] of Object.entries(skinAppearanceVariables(0, 100))) {
      if (key !== "--skin-opacity") expect(value).toBe("100%");
    }
    const strengths = [0, 20, 50, 79, 80, 81, 90, 100];
    for (let i = 1; i < strengths.length; i++) {
      const before = skinAppearanceVariables(25, strengths[i - 1]);
      const after = skinAppearanceVariables(25, strengths[i]);
      for (const key of Object.keys(before) as (keyof typeof before)[]) {
        expect(parseFloat(after[key])).toBeGreaterThanOrEqual(parseFloat(before[key]));
      }
    }
  });

  it("contains malformed or out-of-range frontend values", () => {
    for (const value of [undefined, null, NaN, Infinity, "50"]) {
      expect(skinPercentage(value, 80)).toBe(80);
    }
    expect(skinPercentage(-5, 80)).toBe(0);
    expect(skinPercentage(120, 80)).toBe(100);
    expect(skinPercentage(42.6, 80)).toBe(43);
  });
});
