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

  it("uses white on a slightly darker button without changing the selected accent", () => {
    const palette = createAccentPalette("#5c85ee");
    expect(palette.base).toBe("#5c85ee");
    expect(palette.actionLight.background).toBe("#4f71cb");
    expect(palette.actionLight.foreground).toBe("#ffffff");
    expect(
      colorContrast(palette.actionLight.background, "#ffffff"),
    ).toBeGreaterThanOrEqual(4.6);
    const variables = accentColorVariables(palette.base);
    expect(variables["--theme-accent-base"]).toBe(palette.base);
    expect(variables["--theme-action-light"]).toBe(palette.actionLight.background);
    expect(variables["--theme-on-action-light-hsl"]).toBe(
      "0.00 0.00% 100.00%",
    );
  });

  it("preserves dark colors with white text and bright colors with black text", () => {
    for (const color of ["#182663", "#2563eb", "#000000"]) {
      const action = createAccentPalette(color).actionLight;
      expect(action.background).toBe(color);
      expect(action.foreground).toBe("#ffffff");
    }
    // These would need more than 18% darkening for white text.
    for (const color of ["#edb53e", "#8b86fd", "#ffffff", "#ffff00"]) {
      const action = createAccentPalette(color).actionLight;
      expect(action.background).toBe(color);
      expect(action.foreground).toBe("#000000");
    }
  });

  it("keeps normal and hover text readable in both themes across the RGB gamut", () => {
    const channels = [0, 51, 102, 153, 204, 255];
    const colors = channels.flatMap((r) =>
      channels.flatMap((g) =>
        channels.map((b) =>
          `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`,
        ),
      ),
    );
    colors.push(...COMMUNITY_ACCENT_PRESETS.map((preset) => preset.color));
    for (const color of colors) {
      const palette = createAccentPalette(color);
      for (const action of [palette.actionLight, palette.actionDark]) {
        expect(
          colorContrast(action.background, action.foreground),
        ).toBeGreaterThanOrEqual(4.6);
        expect(
          colorContrast(action.hoverBackground, action.foreground),
        ).toBeGreaterThanOrEqual(4.6);
      }
    }
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
