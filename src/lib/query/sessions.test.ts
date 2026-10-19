import { describe, expect, it, vi, beforeEach } from "vitest";
import { sessionsApi } from "@/lib/api/sessions";
import { loadTranscript } from "./sessions";
vi.mock("@/lib/api/sessions", () => ({ sessionsApi: { getTranscript: vi.fn() } }));
const page = (start: number, nextOffset: number | null, fingerprint = "snapshot") => ({
  header: { type: "header" as const, total: 2, turns: [], cached: false, parseMs: 1 },
  messages: [{ id: String(start), role: "user", content: String(start) }], start,
  nextOffset, fingerprint, payloadBytes: 100,
});
beforeEach(() => vi.clearAllMocks());
describe("paged session loading", () => {
  it("renders the first batch before all pages finish and preserves OpenCode session scope", async () => {
    const api = vi.mocked(sessionsApi.getTranscript);
    let finish!: (value: ReturnType<typeof page>) => void;
    api.mockResolvedValueOnce(page(0, 1)).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const progress = vi.fn();
    const result = loadTranscript("opencode", "/native/db", "one", progress);
    await vi.waitFor(() => expect(progress).toHaveBeenCalledTimes(1));
    expect(progress.mock.calls[0][0].messages).toHaveLength(1);
    expect(progress.mock.calls[0][0].done).toBe(false);
    finish(page(1, null));
    const state = await result;
    expect(state.messages.map((m) => m.id)).toEqual(["0", "1"]);
    expect(state.done).toBe(true);
    expect(api.mock.calls[1]).toEqual(["opencode", "/native/db", "one", 1, "snapshot"]);
  });
  it("cancels obsolete selections before fetching another batch", async () => {
    vi.mocked(sessionsApi.getTranscript).mockResolvedValue(page(0, 1));
    const controller = new AbortController();
    await expect(loadTranscript("codex", "/native/log", "one", () => controller.abort(), controller.signal)).rejects.toThrow();
    expect(sessionsApi.getTranscript).toHaveBeenCalledTimes(1);
  });
  it("rejects changed snapshots and invalid continuation instead of duplicating messages", async () => {
    vi.mocked(sessionsApi.getTranscript).mockResolvedValueOnce(page(0, 1)).mockResolvedValueOnce(page(1, null, "changed"));
    await expect(loadTranscript("dsh", "/native/log", "one", () => {})).rejects.toThrow("Session changed");
    vi.mocked(sessionsApi.getTranscript).mockResolvedValueOnce(page(0, 0));
    await expect(loadTranscript("dsh", "/native/log", "one", () => {})).rejects.toThrow("continuation");
  });
});
