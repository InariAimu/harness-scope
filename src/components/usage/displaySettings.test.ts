import { expect, it } from "vitest";
import {
  formatMoney,
  formatMoneyTick,
  isValidExchangeRate,
  resolveMoneyDisplay,
  resolveSidebarToday,
  USD_DISPLAY,
} from "./format";
import {
  resolveRequestLogColumns,
  setRequestLogColumn,
} from "./requestLogColumns";

it("converts USD for display with a per-currency rate and safe defaults", () => {
  expect(resolveMoneyDisplay(undefined)).toEqual(USD_DISPLAY);
  expect(resolveMoneyDisplay({ currency: "unknown" })).toEqual(USD_DISPLAY);
  expect(
    resolveMoneyDisplay({ currency: "USD", exchangeRates: { USD: 9 } }),
  ).toEqual(USD_DISPLAY);
  const display = resolveMoneyDisplay({
    currency: "CNY",
    exchangeRates: { CNY: 7.25, EUR: 0.9 },
  });
  expect(formatMoney("2", 4, display)).toBe("¥14.50");
  expect(formatMoney("2", 4, USD_DISPLAY)).toBe("$2.0000");
  expect(
    resolveMoneyDisplay({
      currency: "EUR",
      exchangeRates: { CNY: 7.25, EUR: 0.9 },
    }).rate,
  ).toBe(0.9);
});
it("preserves unknown costs, real zero and tiny nonzero converted amounts", () => {
  const display = { currency: "CNY", rate: 6.8 } as const;
  expect(formatMoney(null, 4, display)).toBe("--");
  expect(formatMoney("0", 4, display)).toBe("¥0.00");
  expect(formatMoney("0.00001", 4, display)).toBe("¥0.000068");
  expect(formatMoney(0.0012, 4, { currency: "JPY", rate: 158 })).toBe("¥0.19");
  // Chart ticks already contain converted values; do not apply the rate twice.
  expect(formatMoneyTick(25, display)).toBe("¥25");
});
it("rejects invalid rates and falls back when reading invalid saved rates", () => {
  for (const rate of [0, -1, NaN, Infinity, "7", null]) {
    expect(isValidExchangeRate(rate)).toBe(false);
    expect(
      resolveMoneyDisplay({
        currency: "CNY",
        exchangeRates: { CNY: rate as number },
      }).rate,
    ).toBe(6.8);
  }
});
it("limits sidebar metrics to two known items in the stable menu order", () => {
  expect(resolveSidebarToday(undefined)).toEqual(["cost"]);
  expect(resolveSidebarToday({ sidebarTodayItems: [] })).toEqual([]);
  expect(
    resolveSidebarToday({
      sidebarTodayItems: ["hitRate", "tokens", "cost", "bad", "tokens"],
    }),
  ).toEqual(["cost", "tokens"]);
});
it("keeps the time column locked and stores only changed column preferences", () => {
  const defaults = resolveRequestLogColumns(undefined);
  expect(defaults).toHaveLength(10);
  expect(defaults).not.toContain("duration");
  expect(
    resolveRequestLogColumns({
      time: false,
      app: false,
      statusCode: true,
      duration: true,
      invented: true,
    }),
  ).toEqual(
    defaults.filter((c) => c !== "app").concat(["duration", "statusCode"]),
  );
  expect(setRequestLogColumn({ model: false }, "time", false)).toEqual({
    model: false,
  });
  expect(setRequestLogColumn({ model: false }, "model", true)).toEqual({});
  expect(setRequestLogColumn(undefined, "cacheWrite", true)).toEqual({
    cacheWrite: true,
  });
  expect(resolveRequestLogColumns({})).toEqual(defaults);
});
