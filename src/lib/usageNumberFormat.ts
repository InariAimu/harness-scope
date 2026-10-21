import type { UsageDisplaySettings } from "@/types";
import { parseFiniteNumber } from "@/components/usage/format";

export const NUMBER_UNIT_STYLES = ["international", "chinese", "full"] as const;
export type NumberUnitStyle = (typeof NUMBER_UNIT_STYLES)[number];
export const NUMBER_DECIMAL_PLACES = [0, 1, 2, 3] as const;
export type NumberDecimalPlaces = (typeof NUMBER_DECIMAL_PLACES)[number];

type NumberSettings = Pick<UsageDisplaySettings, "numberUnitStyle" | "numberDecimals">;

export function resolveUsageNumberFormat(settings?: NumberSettings) {
  const unitStyle = NUMBER_UNIT_STYLES.find(style => style === settings?.numberUnitStyle) ?? "international";
  const decimals = NUMBER_DECIMAL_PLACES.find(value => value === settings?.numberDecimals) ?? 1;
  return { unitStyle, decimals };
}

const INTERNATIONAL_UNITS = [[1, ""], [1e3, "K"], [1e6, "M"], [1e9, "B"], [1e12, "T"]] as const;
const CHINESE_UNITS = [[1, ""], [1e4, "万"], [1e8, "亿"], [1e12, "万亿"]] as const;

/** Unit selection is explicit and independent of the interface language. */
export function createUsageNumberFormatter(settings?: NumberSettings, locale = "en-US") {
  const { unitStyle, decimals } = resolveUsageNumberFormat(settings);
  const integer = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
  const compact = new Intl.NumberFormat(locale, { maximumFractionDigits: decimals });
  const units = unitStyle === "chinese" ? CHINESE_UNITS : INTERNATIONAL_UNITS;
  const factor = 10 ** decimals;
  return (value: unknown): string => {
    const num = parseFiniteNumber(value);
    if (num == null) return "--";
    if (unitStyle === "full") return integer.format(num === 0 ? 0 : num);
    const absolute = Math.abs(num);
    let index = 0;
    while (index + 1 < units.length && absolute >= units[index + 1][0]) index++;
    // Promote rounded values at a boundary instead of displaying 1,000K / 10,000万.
    if (index + 1 < units.length) {
      const scaled = absolute / units[index][0];
      const rounded = index === 0 ? Math.round(scaled) : Math.round(scaled * factor) / factor;
      if (rounded >= units[index + 1][0] / units[index][0]) index++;
    }
    const [size, suffix] = units[index];
    return index === 0 ? integer.format(num === 0 ? 0 : num) : `${compact.format(num / size)}${suffix}`;
  };
}
