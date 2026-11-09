import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { setScope } from "@/lib/client";
import { prefetchHarnessUsage, usageKeys } from "./usage";
vi.mock("@/lib/api/usage", () => ({
  usageApi: { getUsageTrends: vi.fn(), getRequestLogs: vi.fn(), getSpeedTrends: vi.fn(), getSpeedDaily: vi.fn() },
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
  it("prefetches calendar data for All with separate device and filter keys", async () => {
    const client = new QueryClient();
    vi.mocked(usageApi.getSpeedDaily).mockResolvedValue({days:[],selectedRecordCount:0,excludedRecordCount:0});
    await prefetchHarnessUsage(client,{preset:"all"},"codex","Asia/Shanghai",undefined,"speed");
    expect(usageApi.getSpeedDaily).toHaveBeenCalledWith("codex");
    expect(usageApi.getSpeedTrends).not.toHaveBeenCalled();
    expect(client.getQueryData([...usageKeys.speedDaily({appType:"codex"}),"Asia/Shanghai"])).toBeDefined();
    const local = usageKeys.speedDaily({providerName:"a",model:"m"});
    expect(local).not.toEqual(usageKeys.speedDaily({providerName:"b",model:"m"}));
    setScope("all");
    expect(local).not.toEqual(usageKeys.speedDaily({providerName:"a",model:"m"}));
    client.clear();
  });
  it("warms only speed data on the speed route and keys filters and devices separately", async () => {
    const client = new QueryClient();
    const range = { preset: "1d" as const };
    vi.mocked(usageApi.getSpeedTrends).mockResolvedValue({ buckets: [] } as any);
    await prefetchHarnessUsage(client, range, "all", "UTC", undefined, "speed");
    expect(usageApi.getSpeedTrends).toHaveBeenCalledOnce();
    expect(usageApi.getRequestLogs).not.toHaveBeenCalled();
    expect(usageApi.getUsageTrends).not.toHaveBeenCalled();
    expect(client.getQueryData([...usageKeys.speedTrends(range, {}), "UTC"])).toEqual({ buckets: [] });
    const local = usageKeys.speedTrends(range, { model: "a", providerName: "route" });
    expect(local).not.toEqual(usageKeys.speedTrends(range, { model: "b", providerName: "route" }));
    setScope("all");
    expect(local).not.toEqual(usageKeys.speedTrends(range, { model: "a", providerName: "route" }));
    client.clear();
  });
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
