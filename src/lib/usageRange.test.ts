import { describe, expect, it } from "vitest";
import {
  resolveUsageRange,
  usageDateParts,
  usageTimestamp,
} from "./usageRange";

const ts = (date: string) => Date.parse(date) / 1000;
describe("usage range timezone boundaries", () => {
  it("starts today at configured midnight, even when it is a different local date", () => {
    const now = Date.parse("2026-10-19T03:23:45Z");
    expect(
      resolveUsageRange({ preset: "today" }, now, "Asia/Shanghai"),
    ).toEqual({
      startDate: ts("2026-10-19T00:00:00+08:00"),
      endDate: now / 1000,
    });
    expect(
      resolveUsageRange({ preset: "today" }, now, "America/Los_Angeles")
        .startDate,
    ).toBe(ts("2026-10-18T00:00:00-07:00"));
    expect(
      resolveUsageRange({ preset: "today" }, now, "Asia/Kathmandu").startDate,
    ).toBe(ts("2026-10-19T00:00:00+05:45"));
  });
  it("keeps the last-24-hour preset distinct from today", () => {
    const now = Date.parse("2026-03-08T12:00:00-04:00");
    const today = resolveUsageRange(
      { preset: "today" },
      now,
      "America/New_York",
    );
    const rolling = resolveUsageRange(
      { preset: "1d" },
      now,
      "America/New_York",
    );
    expect(today.startDate).toBe(ts("2026-03-08T00:00:00-05:00"));
    expect(rolling.endDate - rolling.startDate).toBe(86400);
    expect(rolling.startDate).not.toBe(today.startDate);
  });
  it("counts lookback days by the calendar across daylight saving", () => {
    const range = resolveUsageRange(
      { preset: "7d" },
      Date.parse("2026-03-09T00:30:00-04:00"),
      "America/New_York",
    );
    expect(range.startDate).toBe(ts("2026-03-03T00:00:00-05:00"));
  });
  it("does not replace an exact midnight with a rolling day", () => {
    const now = Date.parse("2026-10-19T00:00:00+08:00");
    const range = resolveUsageRange({ preset: "today" }, now, "Asia/Shanghai");
    expect(range.startDate).toBe(range.endDate);
  });
  it("keeps custom instants and live endpoints intact", () => {
    const now = Date.parse("2026-10-19T03:23:45Z");
    expect(
      resolveUsageRange(
        { preset: "custom", customStartDate: 1234, customEndDate: 5678 },
        now,
        "Pacific/Kiritimati",
      ),
    ).toEqual({ startDate: 1234, endDate: 5678 });
    expect(
      resolveUsageRange(
        { preset: "custom", customStartDate: 1234, liveEndTime: true },
        now,
        "Pacific/Kiritimati",
      ),
    ).toEqual({ startDate: 1234, endDate: now / 1000 });
  });
  it("resolves DST overlaps early and nonexistent wall times forward", () => {
    expect(
      usageTimestamp(
        { year: 2026, month: 10, day: 25, hour: 2, minute: 30 },
        "Europe/Berlin",
      ),
    ).toBe(ts("2026-10-25T02:30:00+02:00"));
    expect(
      usageTimestamp(
        { year: 2026, month: 3, day: 8, hour: 2, minute: 30 },
        "America/New_York",
      ),
    ).toBe(ts("2026-03-08T03:30:00-04:00"));
  });
  it("round-trips calendar input through an IANA zone", () => {
    const parts = {
      year: 2026,
      month: 10,
      day: 19,
      hour: 0,
      minute: 15,
      second: 0,
    };
    expect(
      usageDateParts(
        usageTimestamp(parts, "Pacific/Kiritimati"),
        "Pacific/Kiritimati",
      ),
    ).toEqual(parts);
  });
});
