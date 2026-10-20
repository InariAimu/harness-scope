import { expect, it } from "vitest";
import { codexRequestHint, codexRequestMode } from "./codexRequest";

it("identifies only explicit Codex tiers without changing raw model names", () => {
  const fast = {
    appType: "codex",
    model: "openai/gpt-6.1-sol",
    requestMetadata: { serviceTier: "priority", reasoningEffort: "xhigh" },
  };
  expect(codexRequestMode(fast)).toBe("Fast");
  expect(codexRequestHint(fast, true)).toContain("xhigh");
  expect(codexRequestHint(fast, false)).toContain("not server-confirmed");
  expect(fast.model).toBe("openai/gpt-6.1-sol");
  expect(
    codexRequestMode({
      appType: "codex",
      requestMetadata: { serviceTier: "fast" },
    }),
  ).toBe("Fast");
  expect(
    codexRequestMode({
      appType: "codex",
      requestMetadata: { serviceTier: "default" },
    }),
  ).toBe("Standard");
  expect(codexRequestMode({ appType: "codex" })).toBeNull();
  expect(
    codexRequestMode({
      appType: "codex",
      requestMetadata: { serviceTier: "auto" },
    }),
  ).toBeNull();
  expect(codexRequestMode({ ...fast, appType: "dsh" })).toBeNull();
});
