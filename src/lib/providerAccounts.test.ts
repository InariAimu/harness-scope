import { expect, it } from "vitest";
import { nativeMoney, quotaDisplay, routeKey } from "./providerAccounts";
import type { QuotaWindow } from "@/types/providerAccounts";

it("keeps unknown balances distinct from a real zero and retains native currency", () => {
  for (const missing of [null, undefined, "", "bad"])
    expect(nativeMoney(missing, "CNY", "en-US")).toBe("—");
  expect(nativeMoney("0", "USD", "en-US")).toBe("$0.00");
  expect(nativeMoney("6.68249010", "USD", "en-US")).toBe("$6.6824901");
  expect(nativeMoney("10", "CNY", "en-US")).toContain("10.00");
});

it("treats quota resets as stale rather than inventing a full new window", () => {
  const window: QuotaWindow = {
    id: "rolling",
    status: "ok",
    usedPercent: 23.5,
    resetsAt: 200,
  };
  expect(quotaDisplay(window, 100)).toEqual({
    remaining: 76.5,
    used: 23.5,
    expired: false,
    limited: false,
  });
  expect(quotaDisplay(window, 200).expired).toBe(true);
  expect(
    quotaDisplay({ ...window, status: "rate-limited" }, 100),
  ).toMatchObject({ remaining: 0, limited: true });
  expect(quotaDisplay({ ...window, usedPercent: 103 }, 100)).toMatchObject({
    remaining: 0,
    used: 100,
    limited: true,
  });
});

it("links exact harness and provider pairs without delimiter collisions", () => {
  expect(routeKey({ harness: "dsh", provider: "opencode-go" })).not.toBe(
    routeKey({ harness: "opencode", provider: "opencode-go" }),
  );
  expect(routeKey({ harness: "a/b", provider: "c" })).not.toBe(
    routeKey({ harness: "a", provider: "b/c" }),
  );
});
