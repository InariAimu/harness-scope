interface MonthLabelBounds {
  left: number;
  right: number;
}

/** Keep labels at their calendar columns, preferring the later month on collision. */
export function monthLabelVisibility(
  labels: readonly MonthLabelBounds[],
  gap = 4,
): boolean[] {
  const visible = labels.map(() => false);
  let nextLeft = Infinity;
  for (let i = labels.length - 1; i >= 0; i--) {
    if (labels[i].right + gap <= nextLeft) {
      visible[i] = true;
      nextLeft = labels[i].left;
    }
  }
  return visible;
}
