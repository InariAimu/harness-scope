import { expect, it } from "vitest";
import { goPercent, goQueryPrompt } from "./goCatalog";
import type { GoCatalogInfo } from "@/types/goCatalog";
it("copies only catalog maintenance context and explains unknown rates and manual apply", () => {
  const info = {
    path: "C:/local/scope.opencode-go.json",
    catalog: {
      version: "test",
      checkedAt: "2026-10-20",
      sourceUrl: "https://opencode.ai/v2/docs/console/go/#usage-limits",
      models: [],
    },
    credentials: "must-never-copy-this",
    file: { error: "must-never-copy-this" },
  } as unknown as GoCatalogInfo;
  const prompt = goQueryPrompt(info);
  expect(prompt).toContain(info.path);
  expect(prompt).toContain(info.catalog.sourceUrl);
  expect(prompt).toContain("null");
  expect(prompt).toContain("从文件载入");
  expect(prompt).toContain("不要自行乘倍率");
  expect(prompt).not.toContain("must-never-copy-this");
});
it("does not round small positive allowance consumption to zero or cap exhausted windows", () => {
  expect(goPercent("0")).toBe("0%");
  expect(goPercent("0.00000001")).toBe("<0.0001%");
  expect(goPercent("125.123456")).toBe("125.1235%");
  expect(goPercent("bad")).toBe("—");
});
