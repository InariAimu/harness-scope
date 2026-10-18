import { describe, expect, it } from "vitest";
import { canDeleteSession, getSessionKey } from "./utils";
import type { SessionMeta } from "@/types";

describe("native session capabilities and identity", () => {
  const session: SessionMeta = {
    providerId: "opencode", sessionId: "ses_one", sourcePath: "/synthetic/opencode.db",
    canDelete: false,
  };
  it("keeps sessions in the same database independently selectable", () => {
    expect(getSessionKey(session)).not.toBe(getSessionKey({ ...session, sessionId: "ses_two" }));
  });
  it("only enables deletion when the backend advertises it", () => {
    expect(canDeleteSession(session)).toBe(false);
    expect(canDeleteSession({ ...session, canDelete: undefined })).toBe(false);
    expect(canDeleteSession({ ...session, providerId: "codex", canDelete: true })).toBe(true);
    expect(canDeleteSession({ ...session, canDelete: true, sourcePath: undefined })).toBe(false);
  });
});
