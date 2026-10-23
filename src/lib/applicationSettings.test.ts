import { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { command } from "./client";
import { applicationSettings, saveApplicationSettings } from "./applicationSettings";

vi.mock("./client", () => ({ command: vi.fn() }));
afterEach(() => vi.resetAllMocks());

describe("application switches", () => {
  it("defaults to existing behavior for installations without preferences", () => {
    expect(applicationSettings(undefined, "codex")).toEqual({ collectionEnabled: true, visible: true });
  });

  it("updates only the saved app and preserves aggregates and sessions when hiding its filter", async () => {
    const client = new QueryClient();
    client.setQueryData(["snapshot"], { config: { sources: ["unchanged"] }, applicationSettings: { claude: { collectionEnabled: false, visible: true } } });
    client.setQueryData(["usage", "summary", "local"], { totalRequests: 10 });
    client.setQueryData(["usage", "logs", "all"], { total: 20 });
    client.setQueryData(["sessions"], ["cached"]);
    client.setQueryData(["provider-accounts"], ["unchanged"]);
    vi.mocked(command).mockResolvedValue({ collectionEnabled: true, visible: false });
    await saveApplicationSettings(client, "codex", { visible: false });
    expect(command).toHaveBeenCalledWith("save_application_settings", { harness: "codex", patch: { visible: false } });
    expect(client.getQueryData(["snapshot"])).toEqual({ config: { sources: ["unchanged"] }, applicationSettings: { claude: { collectionEnabled: false, visible: true }, codex: { collectionEnabled: true, visible: false } } });
    expect(client.getQueryData(["usage", "summary", "local"])).toEqual({ totalRequests: 10 });
    expect(client.getQueryData(["usage", "logs", "all"])).toEqual({ total: 20 });
    expect(client.getQueryData(["sessions"])).toEqual(["cached"]);
    expect(client.getQueryData(["provider-accounts"])).toEqual(["unchanged"]);
    client.clear();
  });

  it("refreshes session deletion capabilities when collection changes without dropping usage", async () => {
    const client = new QueryClient();
    client.setQueryData(["sessions"], [{ canDelete: true }]);
    client.setQueryData(["usage", "summary"], { totalRequests: 10 });
    vi.mocked(command).mockResolvedValue({ collectionEnabled: false, visible: true });
    await saveApplicationSettings(client, "codex", { collectionEnabled: false });
    expect(client.getQueryData(["sessions"])).toBeUndefined();
    expect(client.getQueryData(["usage", "summary"])).toEqual({ totalRequests: 10 });
    client.clear();
  });

  it("keeps the prior switch and cached data when persistence fails", async () => {
    const client = new QueryClient();
    const snapshot = { applicationSettings: { codex: { collectionEnabled: true, visible: true } } };
    client.setQueryData(["snapshot"], snapshot);
    client.setQueryData(["usage", "summary"], { totalRequests: 10 });
    vi.mocked(command).mockRejectedValue(new Error("write failed"));
    await expect(saveApplicationSettings(client, "codex", { visible: false })).rejects.toThrow("write failed");
    expect(client.getQueryData(["snapshot"])).toEqual(snapshot);
    expect(client.getQueryData(["usage", "summary"])).toEqual({ totalRequests: 10 });
    client.clear();
  });
});
