export const TREND_BAR_RADIUS = 6;
export const TREND_BAR_ACTIVE_SCALE = 1.05;
export const TREND_BAR_OUTLINE_WIDTH = 2;
export const TREND_BAR_REST_TRANSFORM = "matrix(1, 0, 0, 1, 0, 0)";

export interface TrendBarRect { x: number; y: number; width: number; height: number }

/** Recover the complete column from a segment and its stacked value interval. */
export function trendBarGeometry(
  segment: TrendBarRect, start: number, end: number, total: number,
) {
  const tolerance = Number.EPSILON * Math.max(1, Math.abs(total), Math.abs(end)) * 8;
  if (![segment.x, segment.y, segment.width, segment.height, start, end, total].every(Number.isFinite)
    || segment.width <= 0 || segment.height <= 0 || start < 0 || end <= start || total + tolerance < end) return null;
  const columnTotal = Math.max(total, end);
  const pixelsPerUnit = segment.height / (end - start);
  const column = {
    x: segment.x,
    y: segment.y - Math.max(0, columnTotal - end) * pixelsPerUnit,
    width: segment.width,
    height: columnTotal * pixelsPerUnit,
  };
  const radius = Math.min(TREND_BAR_RADIUS, column.width / 2, column.height);
  const centerX = segment.x + segment.width / 2;
  const centerY = segment.y + segment.height / 2;
  const halfStroke = TREND_BAR_OUTLINE_WIDTH / 2;
  const scaledWidth = segment.width * TREND_BAR_ACTIVE_SCALE;
  const scaledHeight = segment.height * TREND_BAR_ACTIVE_SCALE;
  const baseline = column.y + column.height;
  const shiftY = Math.min(0, baseline - halfStroke - (centerY + scaledHeight / 2));
  const hoverBounds = {
    x: centerX - scaledWidth / 2 - halfStroke,
    y: centerY - scaledHeight / 2 + shiftY - halfStroke,
    width: scaledWidth + TREND_BAR_OUTLINE_WIDTH,
    height: scaledHeight + TREND_BAR_OUTLINE_WIDTH,
  };
  return {
    column, radius, baseline, hoverBounds,
    // A fixed clip also prevents the partially faded stroke from crossing the
    // baseline while its transform is interpolating back to the original size.
    baselineClip: { x: hoverBounds.x, y: Math.min(column.y, hoverBounds.y) - halfStroke,
      width: hoverBounds.width, height: baseline - Math.min(column.y, hoverBounds.y) + halfStroke },
    clipPath: trendBarSlicePath(column, radius, column.y, column.y + column.height),
    // This is the intersection of the original segment with the column cap.
    // Stroking the intersection outlines the curved edge as well as the slice
    // boundary; stroking a clipped rectangle would lose the curved outline.
    segmentPath: trendBarSlicePath(column, radius, segment.y, segment.y + segment.height),
    activeTransform: `matrix(${TREND_BAR_ACTIVE_SCALE}, 0, 0, ${TREND_BAR_ACTIVE_SCALE}, ${centerX * (1 - TREND_BAR_ACTIVE_SCALE)}, ${centerY * (1 - TREND_BAR_ACTIVE_SCALE) + shiftY})`,
  };
}

/** Horizontal extent inside the column's circular top corners. */
export function trendBarSpan(column: TrendBarRect, radius: number, y: number) {
  const depth = Math.max(0, y - column.y);
  const inset = radius > 0 && depth < radius
    ? radius - Math.sqrt(Math.max(0, radius * radius - (radius - depth) ** 2)) : 0;
  return { left: column.x + inset, right: column.x + column.width - inset };
}

/** A horizontal slice of one top-rounded column, including partial corner arcs. */
function trendBarSlicePath(column: TrendBarRect, radius: number, from: number, to: number) {
  const top = Math.max(column.y, from);
  const bottom = Math.min(column.y + column.height, to);
  if (bottom <= top) return "";
  const upper = trendBarSpan(column, radius, top);
  const lower = trendBarSpan(column, radius, bottom);
  const cornerBottom = column.y + radius;
  const path = [`M ${upper.left} ${top}`, `L ${upper.right} ${top}`];
  if (radius > 0 && top < cornerBottom) {
    const arcBottom = Math.min(bottom, cornerBottom);
    path.push(`A ${radius} ${radius} 0 0 1 ${trendBarSpan(column, radius, arcBottom).right} ${arcBottom}`);
  }
  if (bottom > cornerBottom || radius === 0) path.push(`L ${lower.right} ${bottom}`);
  path.push(`L ${lower.left} ${bottom}`);
  if (radius > 0 && top < cornerBottom) {
    if (bottom > cornerBottom) path.push(`L ${column.x} ${cornerBottom}`);
    path.push(`A ${radius} ${radius} 0 0 1 ${upper.left} ${top}`);
  }
  path.push("Z");
  return path.join(" ");
}
