import { expect, it } from "vitest";
import { monthLabelVisibility } from "./heatmapMonths";

it("hides a partial October label next to November without moving month boundaries", () => {
  const labels = [{ left: 0, right: 22 }, { left: 13, right: 35 }, { left: 65, right: 87 }];
  expect(monthLabelVisibility(labels)).toEqual([false, true, true]);
  expect(labels[1].left).toBe(13);
});

it("restores the first month when wider week columns provide enough space", () => {
  expect(monthLabelVisibility([{ left: 0, right: 22 }, { left: 32, right: 54 }]))
    .toEqual([true, true]);
});

it("uses measured text widths for longer labels and retains noncolliding earlier months", () => {
  expect(monthLabelVisibility([
    { left: 0, right: 20 }, { left: 40, right: 96 }, { left: 80, right: 105 },
  ])).toEqual([true, false, true]);
  expect(monthLabelVisibility([])).toEqual([]);
  expect(monthLabelVisibility([{ left: 0, right: 32 }])).toEqual([true]);
});
