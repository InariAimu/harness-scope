export const DEFAULT_ACCENT_COLOR = "#f97316";

export function normalizeAccentColor(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const hex = value.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(hex)) return hex;
  if (/^#[0-9a-f]{3}$/.test(hex)) {
    return `#${[...hex.slice(1)].map((digit) => digit + digit).join("")}`;
  }
  return null;
}

function rgb(color: string): number[] {
  return [1, 3, 5].map((offset) =>
    parseInt(color.slice(offset, offset + 2), 16),
  );
}

function mix(color: string, target: string, amount: number): string {
  const other = rgb(target);
  return `#${rgb(color)
    .map((value, i) =>
      Math.round(value + (other[i] - value) * amount)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function luminance(color: string): number {
  const channels = rgb(color).map((value) => {
    const channel = value / 255;
    return channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

export function colorContrast(a: string, b: string): number {
  const first = luminance(a);
  const second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

function foreground(background: string): string {
  return colorContrast(background, "#000000") >=
    colorContrast(background, "#ffffff")
    ? "#000000"
    : "#ffffff";
}

function readable(color: string, background: string, target: string): string {
  if (colorContrast(color, background) >= 4.5) return color;
  // Find the smallest adjustment that keeps small text readable on raised surfaces.
  let low = 0;
  let high = 1;
  for (let i = 0; i < 12; i++) {
    const mid = (low + high) / 2;
    if (colorContrast(mix(color, target, mid), background) >= 4.5) high = mid;
    else low = mid;
  }
  return mix(color, target, high);
}

/** HSL channels for existing Tailwind tokens, including /opacity modifiers. */
function hsl(color: string): string {
  const [r, g, b] = rgb(color).map((channel) => channel / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;
  const lightness = (max + min) / 2;
  const saturation =
    delta === 0 ? 0 : delta / (1 - Math.abs(2 * lightness - 1));
  const hue =
    delta === 0
      ? 0
      : max === r
        ? ((g - b) / delta + 6) % 6
        : max === g
          ? (b - r) / delta + 2
          : (r - g) / delta + 4;
  return `${(hue * 60).toFixed(2)} ${(saturation * 100).toFixed(2)}% ${(lightness * 100).toFixed(2)}%`;
}

export function createAccentPalette(value: unknown) {
  const base = normalizeAccentColor(value) ?? DEFAULT_ACCENT_COLOR;
  const minus = mix(base, "#000000", 0.25);
  const plus = mix(base, "#ffffff", 0.25);
  const textLight = readable(minus, "#e0e0e0", "#000000");
  const textDark = readable(plus, "#48484d", "#ffffff");
  return {
    base,
    minus,
    plus,
    textLight,
    textDark,
    foreground: foreground(base),
    foregroundMinus: foreground(minus),
    foregroundPlus: foreground(plus),
  };
}

export function accentColorVariables(value: unknown): Record<string, string> {
  const palette = createAccentPalette(value);
  return {
    "--theme-accent-base": palette.base,
    "--theme-accent-minus": palette.minus,
    "--theme-accent-plus": palette.plus,
    "--theme-accent-on-base": palette.foreground,
    "--theme-accent-on-minus": palette.foregroundMinus,
    "--theme-accent-text-light": palette.textLight,
    "--theme-accent-text-dark": palette.textDark,
    "--theme-accent-base-hsl": hsl(palette.base),
    "--theme-accent-on-base-hsl": hsl(palette.foreground),
    "--theme-accent-minus-hsl": hsl(palette.minus),
    "--theme-accent-on-minus-hsl": hsl(palette.foregroundMinus),
    "--theme-accent-text-light-hsl": hsl(palette.textLight),
    "--theme-accent-text-dark-hsl": hsl(palette.textDark),
  };
}
