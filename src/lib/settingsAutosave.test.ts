import { describe, expect, it, vi } from "vitest";
import { SettingsAutosave } from "./settingsAutosave";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
type Config = { theme: string; name: string };

describe("settings autosave", () => {
  it("serializes writes, coalesces pending edits, and applies only the latest field response", async () => {
    const first = deferred<Config>();
    const second = deferred<Config>();
    const save = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const saved = vi.fn();
    const busy = vi.fn();
    const queue = new SettingsAutosave<Config>({ save, saved, failed: vi.fn(), busy });
    queue.enqueue({ theme: "dark" });
    queue.enqueue({ name: "A" });
    queue.enqueue({ theme: "light" });
    queue.enqueue({ name: "B" });
    expect(save).toHaveBeenCalledTimes(1);
    first.resolve({ theme: "dark", name: "old" });
    await tick();
    expect(saved.mock.calls[0][0]).toEqual({});
    expect(save.mock.calls[1][0]).toEqual({ name: "B", theme: "light" });
    second.resolve({ theme: "light", name: "B" });
    await tick();
    expect(saved.mock.calls[1][0]).toEqual({ name: "B", theme: "light" });
    expect(busy.mock.calls).toEqual([[true], [false]]);
  });

  it("does not roll a newer edit back when an older request fails, and keeps draining", async () => {
    const first = deferred<Config>();
    const failed = vi.fn();
    const saved = vi.fn();
    const save = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValue({ theme: "light", name: "new" });
    const queue = new SettingsAutosave<Config>({ save, saved, failed, busy: vi.fn() });
    queue.enqueue({ theme: "dark", name: "invalid" });
    queue.enqueue({ theme: "light" });
    first.reject(new Error("write failed"));
    await tick();
    expect(failed.mock.calls[0][0]).toEqual({ name: "invalid" });
    expect(saved.mock.calls[0][0]).toEqual({ theme: "light" });
    queue.enqueue({ name: "new" });
    await tick();
    expect(save).toHaveBeenCalledTimes(3);
  });
});
