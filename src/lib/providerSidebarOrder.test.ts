import { describe, expect, it } from "vitest";
import { moveProviderSidebarAccount, reconcileProviderSidebarOrder } from "./providerSidebarOrder";

describe("provider sidebar order", () => {
  it("keeps the initial account order until a custom order is saved", () => {
    expect(reconcileProviderSidebarOrder([], ["a", "b", "c"])).toEqual(["a", "b", "c"]);
  });

  it("preserves edits through polling, renaming, new accounts and deletion", () => {
    const saved = ["c", "a", "b"];
    expect(reconcileProviderSidebarOrder(saved, ["a", "b", "c"])).toEqual(saved);
    expect(reconcileProviderSidebarOrder(saved, ["a", "b", "c", "new"])).toEqual(["c", "a", "b", "new"]);
    expect(reconcileProviderSidebarOrder(saved, ["a", "c"])).toEqual(["c", "a"]);
    expect(reconcileProviderSidebarOrder(["c", "c", "deleted"], ["a", "c"])).toEqual(["c", "a"]);
  });

  it("moves past a hidden account without shifting its reserved slot", () => {
    const order = ["a", "hidden", "b", "c"];
    const moved = moveProviderSidebarAccount(order, ["a", "b", "c"], "b", -1);
    expect(moved).toEqual(["b", "hidden", "a", "c"]);
    expect(order).toEqual(["a", "hidden", "b", "c"]);
    expect(moveProviderSidebarAccount(moved, ["b", "a", "c"], "b", 1)).toEqual(order);
  });

  it("keeps the first and last visible accounts at their boundaries", () => {
    const order = ["hidden", "a", "b"];
    for (const [id, direction] of [["a", -1], ["b", 1], ["deleted", 1]] as const) {
      expect(moveProviderSidebarAccount(order, ["a", "b"], id, direction)).toEqual(order);
    }
  });
});
