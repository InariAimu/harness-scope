export const DEFAULT_SKIN_TRANSPARENCY = 0;
export const DEFAULT_SKIN_MASK_STRENGTH = 80;

export function skinPercentage(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(100, Math.max(0, Math.round(value)))
    : fallback;
}

export function skinAppearanceVariables(transparency: unknown, strength: unknown) {
  const amount = skinPercentage(strength, DEFAULT_SKIN_MASK_STRENGTH);
  // 80 preserves the original theme-specific gradients. Both ends remain useful:
  // 0 removes the veil, while 100 makes it completely opaque in every theme.
  const stop = (original: number) =>
    `${amount <= 80
      ? original * amount / 80
      : original + (100 - original) * (amount - 80) / 20}%`;
  return {
    "--skin-opacity": String(1 - skinPercentage(transparency, DEFAULT_SKIN_TRANSPARENCY) / 100),
    "--skin-veil-start": stop(100),
    "--skin-veil-light-25": stop(96),
    "--skin-veil-light-60": stop(80),
    "--skin-veil-light-end": stop(66),
    "--skin-veil-dark-25": stop(97),
    "--skin-veil-dark-60": stop(86),
    "--skin-veil-dark-end": stop(76),
    "--skin-veil-narrow-light": stop(92),
    "--skin-veil-narrow-dark": stop(95),
  };
}
