import { describe, expect, it } from "vitest";
import { ICON_PRESETS, configuredIcon, defaultHarnessIcon, defaultProviderIcon, iconUses, type IconLibrary } from "./entityIcons";

describe("entity icon identity and references", () => {
  const library: IconLibrary = { revision: 1, assets: {}, harnesses: { claude: "asset:shared" }, accounts: { first: "asset:shared", second: "builtin:openai" } };
  it("shares assets while keeping account and harness assignments independent", () => {
    expect(iconUses(library, "shared")).toBe(2);
    expect(configuredIcon(library, { kind: "account", id: "first" })).toBe("asset:shared");
    expect(configuredIcon(library, { kind: "account", id: "claude" })).toBeUndefined();
    expect(configuredIcon(library, { kind: "harness", id: "first" })).toBeUndefined();
  });
  it("uses stable provider types for brand defaults and leaves custom providers generic", () => {
    expect(defaultHarnessIcon("claude-desktop")).toBe("builtin:claude");
    expect(defaultHarnessIcon("dsh")).toBe("builtin:deepseek");
    expect(defaultProviderIcon("deepseek")).toBe("builtin:deepseek");
    expect(defaultProviderIcon("openai-codex")).toBe("builtin:openai");
    expect(defaultProviderIcon("custom:deepseek")).toBeUndefined();
  });
  it("resolves the shared preset catalog to bundled artwork for both community styles", () => {
    expect(new Set(ICON_PRESETS.map(p => p.id)).size).toBe(ICON_PRESETS.length);
    expect(ICON_PRESETS.filter(p => p.group === "brand")).toHaveLength(5);
    const providers = ["anthropic", "deepseek", "bytedance", "google", "zhipu", "openai", "xai", "moonshot"];
    for (const group of ["portrait", "pixel"]) {
      expect(ICON_PRESETS.filter(p => p.group === group)).toHaveLength(providers.length);
      for (const provider of providers) {
        const preset = ICON_PRESETS.find(p => p.id === `${provider}-${group}`);
        expect(preset?.group).toBe(group);
        expect(preset?.src).toBeTruthy();
        expect(preset?.svg).toBeUndefined();
      }
    }
    for (const preset of ICON_PRESETS.filter(p => p.group === "brand")) {
      expect(preset.svg).toContain("<svg");
      expect(preset.src).toBeUndefined();
    }
  });
});
