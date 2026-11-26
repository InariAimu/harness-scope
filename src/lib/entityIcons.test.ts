import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AppGlyph } from "@/components/shell/AppGlyph";
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
    expect(defaultHarnessIcon("cherry")).toBe("builtin:cherry");
    expect(defaultProviderIcon("deepseek")).toBe("builtin:deepseek");
    expect(defaultProviderIcon("openai-codex")).toBe("builtin:openai");
    expect(defaultProviderIcon("google")).toBe("builtin:google");
    expect(defaultHarnessIcon("gemini")).toBe("builtin:google-pixel");
    expect(defaultHarnessIcon("antigravity")).toBe("builtin:antigravity");
    expect(defaultProviderIcon("custom:deepseek")).toBeUndefined();
  });
  it("shows the CLI badge on Gemini and Claude Code and the official artwork on Antigravity", () => {
    const client = new QueryClient();
    const glyph = (app: "claude" | "gemini" | "antigravity") => renderToStaticMarkup(
      createElement(QueryClientProvider, { client }, createElement(AppGlyph, { app })),
    );
    expect(glyph("claude")).toContain("lucide-terminal");
    expect(glyph("gemini")).toContain("lucide-terminal");
    const antigravity = glyph("antigravity");
    expect(antigravity).toContain("antigravity.png");
    expect(antigravity).not.toContain("lucide-terminal");
  });
  it("resolves the shared preset catalog to bundled artwork for both community styles", () => {
    expect(new Set(ICON_PRESETS.map(p => p.id)).size).toBe(ICON_PRESETS.length);
    expect(ICON_PRESETS.find(p => p.id === "antigravity")).toMatchObject({ group: "brand", file: "antigravity.png" });
    expect(ICON_PRESETS.find(p => p.id === "google")).toMatchObject({ group: "brand", file: "google-g.png" });
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
      if (preset.file.endsWith(".png")) {
        expect(preset.src).toBeTruthy();
        expect(preset.svg).toBeUndefined();
      } else {
        expect(preset.svg).toContain("<svg");
        expect(preset.src).toBeUndefined();
      }
    }
  });
});
