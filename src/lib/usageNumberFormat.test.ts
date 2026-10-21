import { describe, expect, it } from "vitest";
import { createUsageNumberFormatter, resolveUsageNumberFormat } from "./usageNumberFormat";

describe("usage quantity display", () => {
  it("defaults old settings and invalid preferences to international units with one decimal", () => {
    expect(resolveUsageNumberFormat()).toEqual({ unitStyle: "international", decimals: 1 });
    expect(resolveUsageNumberFormat({ numberUnitStyle: "auto", numberDecimals: 8 }))
      .toEqual({ unitStyle: "international", decimals: 1 });
    expect(createUsageNumberFormatter()(60_234_567)).toBe("60.2M");
  });

  it.each(["en-US", "zh-CN", "zh-TW", "ja-JP"])("does not change units with locale %s", locale => {
    const international = createUsageNumberFormatter({ numberUnitStyle: "international" }, locale);
    const chinese = createUsageNumberFormatter({ numberUnitStyle: "chinese" }, locale);
    expect([12_345, 60_234_567, 1_234_567_890].map(international)).toEqual(["12.3K", "60.2M", "1.2B"]);
    expect([12_345, 60_234_567, 1_234_567_890].map(chinese)).toEqual(["1.2万", "6,023.5万", "12.3亿"]);
  });

  it("respects decimal precision, omits trailing zeros, and keeps small counts as integers", () => {
    const format = createUsageNumberFormatter({ numberDecimals: 3 });
    expect([0, 413, 1_000, 12_345, 12_340, -12_345].map(format))
      .toEqual(["0", "413", "1K", "12.345K", "12.34K", "-12.345K"]);
    expect(createUsageNumberFormatter({ numberDecimals: 0 })(12_345)).toBe("12K");
    expect(createUsageNumberFormatter({ numberDecimals: 2 })(12_345)).toBe("12.35K");
  });

  it("promotes rounded boundaries to the next unit", () => {
    const format = createUsageNumberFormatter();
    expect([999.9, 999_949, 999_950, 999_950_000, -999_950, 1e12].map(format))
      .toEqual(["1K", "999.9K", "1M", "1B", "-1M", "1T"]);
    const chinese = createUsageNumberFormatter({ numberUnitStyle: "chinese" });
    expect([9_999.9, 99_999_500, 1e12].map(chinese)).toEqual(["1万", "1亿", "1万亿"]);
  });

  it("shows full grouped integers without abbreviating or losing precision to the decimal setting", () => {
    const format = createUsageNumberFormatter({ numberUnitStyle: "full", numberDecimals: 0 });
    expect(format(1_234_567_890)).toBe("1,234,567,890");
    expect(format(60_234_567)).toBe("60,234,567");
  });

  it("uses locale decimal separators without changing the selected unit system", () => {
    expect(createUsageNumberFormatter({}, "tr-TR")(12_345)).toBe("12,3K");
    expect(createUsageNumberFormatter({ numberUnitStyle: "full" }, "tr-TR")(12_345)).toBe("12.345");
  });

  it("distinguishes missing values from zero and accepts numeric API strings", () => {
    const format = createUsageNumberFormatter();
    expect([null, undefined, "", "invalid", Infinity, NaN].map(format)).toEqual(Array(6).fill("--"));
    expect(format("12345")).toBe("12.3K");
    expect(format(-0)).toBe("0");
  });
});
