import { describe, expect, it } from "vitest";
import { COMMUNITY_ACCENT_PRESETS } from "./communityAccentPresets";
import {
  DEFAULT_ACCENT_COLOR,
  accentColorVariables,
  colorContrast,
  createAccentPalette,
  normalizeAccentColor,
} from "./accentColor";

describe("accent colors", () => {
  it("normalizes valid hex inputs and rejects CSS or malformed values", () => {
    expect(normalizeAccentColor(" #ABC ")).toBe("#aabbcc");
    expect(normalizeAccentColor("#2563EB")).toBe("#2563eb");
    for (const input of [
      null,
      undefined,
      "red",
      "#12345g",
      "#12345678",
      "var(--danger)",
    ]) {
      expect(normalizeAccentColor(input)).toBeNull();
      expect(createAccentPalette(input).base).toBe(DEFAULT_ACCENT_COLOR);
    }
  });

  it("generates darker and lighter colors around the selected source", () => {
    const palette = createAccentPalette("#2563eb");
    expect(palette.base).toBe("#2563eb");
    expect(colorContrast(palette.minus, "#ffffff")).toBeGreaterThan(
      colorContrast(palette.base, "#ffffff"),
    );
    expect(colorContrast(palette.plus, "#ffffff")).toBeLessThan(
      colorContrast(palette.base, "#ffffff"),
    );
    expect(accentColorVariables(palette.base)["--theme-accent-base"]).toBe(
      palette.base,
    );
  });

  it("keeps text legible for bright, dark, saturated and grayscale accents", () => {
    for (const color of [
      "#ffffff",
      "#000000",
      "#ffff00",
      "#00ff00",
      "#ff0000",
      "#0000ff",
      "#777777",
      DEFAULT_ACCENT_COLOR,
      ...COMMUNITY_ACCENT_PRESETS.map((preset) => preset.color),
    ]) {
      const palette = createAccentPalette(color);
      expect(
        colorContrast(palette.base, palette.foreground),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        colorContrast(palette.minus, palette.foregroundMinus),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        colorContrast(palette.plus, palette.foregroundPlus),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        colorContrast(palette.textLight, "#e0e0e0"),
      ).toBeGreaterThanOrEqual(4.5);
      expect(colorContrast(palette.textDark, "#48484d")).toBeGreaterThanOrEqual(
        4.5,
      );
    }
  });
});
