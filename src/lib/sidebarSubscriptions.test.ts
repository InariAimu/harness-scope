import { describe, expect, it } from "vitest";
import { quotaCountdown, quotaWindowShortLabel, snapshotAge } from "./sidebarSubscriptions";

describe("sidebar subscription summaries", () => {
  it("never invents time remaining for expired or invalid resets", () => {
    expect(quotaCountdown(99, 100)).toBe("—");
    expect(quotaCountdown(100, 100)).toBe("—");
    expect(quotaCountdown(NaN, 100)).toBe("—");
    expect(quotaCountdown(101, 100)).toBe("1m");
    expect(quotaCountdown(100 + 163 * 60, 100)).toBe("2h43m");
    expect(quotaCountdown(100 + (3 * 24 + 8) * 3600, 100)).toBe("3d8h");
  });
  it("uses official durations and keeps missing snapshots distinct from fresh data", () => {
    const window = { id: "primary", usedPercent: 12, resetsAt: 100, status: "ok" as const };
    expect(quotaWindowShortLabel({ ...window, durationMinutes: 10080 }, true)).toBe("周");
    expect(quotaWindowShortLabel({ ...window, durationMinutes: 300 }, true)).toBe("5h");
    expect(quotaWindowShortLabel(window, true)).toBe("—");
    expect(snapshotAge(undefined, 100, true)).toBe("未查询");
    expect(snapshotAge(101, 100, true)).toBe("刚刚");
    expect(snapshotAge(100, 280, true)).toBe("3分钟前");
  });
});
