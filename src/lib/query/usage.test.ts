import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { setScope } from "@/lib/client";
import { prefetchHarnessUsage, usageKeys } from "./usage";
vi.mock("@/lib/api/usage", () => ({
  usageApi: { getUsageTrends: vi.fn(), getRequestLogs: vi.fn() },
}));
beforeEach(() => {
  setScope("local");
  vi.clearAllMocks();
  vi.mocked(usageApi.getUsageTrends).mockResolvedValue([]);
  vi.mocked(usageApi.getRequestLogs).mockResolvedValue({
    data: [],
    total: 7,
    page: 0,
    pageSize: 20,
  });
});
describe("harness intent prefetch", () => {
  it("keeps different device scopes in separate caches", async () => {
    const client = new QueryClient();
    await prefetchHarnessUsage(client, { preset: "today" }, "codex", "UTC");
    const localKey = [
      ...usageKeys.logs({ preset: "today", appType: "codex" }, 0, 20),
      "UTC",
    ];
    setScope("all");
    await prefetchHarnessUsage(client, { preset: "today" }, "codex", "UTC");
    const allKey = [
      ...usageKeys.logs({ preset: "today", appType: "codex" }, 0, 20),
      "UTC",
    ];
    expect(localKey).not.toEqual(allKey);
    expect(usageApi.getRequestLogs).toHaveBeenCalledTimes(2);
    expect(client.getQueryData(localKey)).toBeDefined();
    expect(client.getQueryData(allKey)).toBeDefined();
    setScope("local");
    client.clear();
  });
  it("shares visible keys and reuses fresh results until usage invalidation", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const range = { preset: "today" as const };
    await prefetchHarnessUsage(client, range, "codex", "Asia/Shanghai", 200);
    await prefetchHarnessUsage(client, range, "codex", "Asia/Shanghai", 200);
    expect(usageApi.getUsageTrends).toHaveBeenCalledTimes(1);
    expect(usageApi.getRequestLogs).toHaveBeenCalledTimes(1);
    const key = [
      ...usageKeys.logs(
        { preset: "today", appType: "codex", statusCode: 200 },
        0,
        20,
      ),
      "Asia/Shanghai",
    ];
    expect(client.getQueryData(key)).toMatchObject({ total: 7 });
    await client.invalidateQueries({
      queryKey: usageKeys.all,
      refetchType: "none",
    });
    await prefetchHarnessUsage(client, range, "codex", "Asia/Shanghai", 200);
    expect(usageApi.getRequestLogs).toHaveBeenCalledTimes(2);
    client.clear();
  });
  it("uses the unfiltered key for All and keeps status filtering", async () => {
    const client = new QueryClient();
    await prefetchHarnessUsage(client, { preset: "today" }, "all", "UTC", 500);
    expect(usageApi.getRequestLogs).toHaveBeenCalledWith(
      expect.objectContaining({ appType: undefined, statusCode: 500 }),
      0,
      20,
    );
    expect(
      client.getQueryData([
        ...usageKeys.logs({ preset: "today", statusCode: 500 }, 0, 20),
        "UTC",
      ]),
    ).toBeDefined();
    client.clear();
  });
});
