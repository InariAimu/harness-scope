import { describe, expect, it } from "vitest";
import type { SessionMessage } from "@/types";
import { selectMessageVersions } from "./versions";

describe("native message versions", () => {
  const user: SessionMessage = { role: "user", id: "question" };
  const old: SessionMessage = { role: "assistant", id: "old", versionGroup: "answer", versionIndex: 0, versionSelected: false };
  const current: SessionMessage = { ...old, id: "current", versionIndex: 1, versionSelected: true };
  it("defaults to the native selection and switches without changing source messages", () => {
    const messages = [user, old, current];
    expect(selectMessageVersions(messages, {})).toEqual([user, current]);
    expect(selectMessageVersions(messages, { answer: 0 })).toEqual([user, old]);
    expect(selectMessageVersions(messages, { answer: 8 })).toEqual([user, current]);
    expect(current.versionSelected).toBe(true);
  });
  it("waits for the selected version across transcript pages and preserves other harnesses", () => {
    expect(selectMessageVersions([user, old], {})).toEqual([user]);
    const plain = [user, { role: "assistant", id: "plain" }];
    expect(selectMessageVersions(plain, {})).toBe(plain);
  });
});
