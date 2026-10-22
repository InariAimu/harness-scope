import { expect, it } from "vitest";
import { nativeMoney, quotaDisplay, routeKey, accountCanRefresh, accountProviderName, customProviderName, customRemainingPercent } from "./providerAccounts";
import type { ProviderAccountView, CustomUsageItem } from "@/types/providerAccounts";
import type { QuotaWindow } from "@/types/providerAccounts";

const customAccount = (id: string, harness: string, name: string): ProviderAccountView => ({
  account: { id, provider: `custom:${id}`, name, showInSidebar: true,
    links: [{ harness, provider: "shared-route" }], subscription: { planName: "", monthlyFee: null, currency: "USD", periodStart: null, renewalDate: null },
    refresh: { foregroundSeconds: 60, backgroundSeconds: 300 }, custom: { enabled: false, baseUrl: "", script: "", timeoutSeconds: 10 } },
  keyConfigured: false, snapshot: null, lastAttemptAt: null, error: null, refreshing: false,
});

it("keeps custom providers independent and resolves only exact unambiguous route links", () => {
  const a = customAccount("a", "codex", "Relay A");
  const b = customAccount("b", "opencode", "Relay B");
  expect(accountProviderName(a.account.provider, [a, b])).toBe("Relay A");
  expect(accountProviderName(b.account.provider, [a, b])).toBe("Relay B");
  expect(customProviderName([a, b], "shared-route", "codex")).toBe("Relay A");
  expect(customProviderName([a, b], "shared-route", "claude")).toBeUndefined();
  expect(customProviderName([a, b], "shared-route")).toBeUndefined();
  expect(customProviderName([a, b], "SHARED-ROUTE", "codex")).toBeUndefined();
});

it("queries require an enabled configuration and only require a key when referenced", () => {
  const a = customAccount("a", "codex", "A");
  expect(accountCanRefresh(a)).toBe(false);
  a.account.custom!.enabled = true;
  expect(accountCanRefresh(a)).toBe(true);
  a.account.custom!.script = "{{apiKey}}";
  expect(accountCanRefresh(a)).toBe(false);
  a.keyConfigured = true;
  expect(accountCanRefresh(a)).toBe(true);
});

it("missing and expired custom percentages remain unknown", () => {
  const row = { usedPercent: null, resetsAt: null } as CustomUsageItem;
  expect(customRemainingPercent(row, 100)).toBeNull();
  row.usedPercent = "0";
  expect(customRemainingPercent(row, 100)).toBe(100);
  row.usedPercent = "120";
  expect(customRemainingPercent(row, 100)).toBe(0);
  row.resetsAt = 100;
  expect(customRemainingPercent(row, 100)).toBeNull();
});

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
