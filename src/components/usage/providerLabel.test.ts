import { expect, it } from "vitest";
import type { TFunction } from "i18next";
import { getUsageProviderLabel } from "./providerLabel";

const t = ((key: string, args?: Record<string, string>) => args?.defaultValue ?? key) as TFunction;

it("displays Google for either local Google client without changing the native route key", () => {
  for (const route of ["gemini (Session)", "Gemini (Session)", "antigravity (Session)"]) {
    const view=getUsageProviderLabel(route,t);
    expect(view.label).toBe("Google");expect(view.shortLabel).toBe("Google");
    expect(view.hint).toContain("本地来源");expect(view.hint).toContain("未提供独立 API 路由");
  }
});
it("respects explicit linked account names including session routes", () => {
  expect(getUsageProviderLabel("antigravity (Session)",t,"Google AI Work").shortLabel).toBe("Google AI Work");
  expect(getUsageProviderLabel("codex (Session)",t,"My routed account").shortLabel).toBe("My routed account");
  expect(getUsageProviderLabel("openai",t).shortLabel).toBe("OpenAI");
  expect(getUsageProviderLabel("codex (Session)",t).shortLabel).toBe("usage.sessionProvider.short");
});
