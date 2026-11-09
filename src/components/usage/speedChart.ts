import type { SpeedBucket, SpeedDistribution, SpeedTrends } from "@/types/speed";

export type SpeedMetric = "speed" | "ttft";
export const SPEED_UP = "var(--danger)";
export const SPEED_OTHER = "var(--success)";
export const SPEED_MEAN = "var(--chart-mean)";
export function hasSpeedSample(value: SpeedDistribution): boolean {
  return value.sampleCount > 0 && [value.mean, value.min, value.max, value.centralLower, value.centralUpper]
    .every((n) => n != null && Number.isFinite(n));
}
export function speedDirection(buckets: SpeedBucket[], index: number, metric: SpeedMetric) {
  const current = buckets[index];
  // Direction survives gaps; only the mean line requires adjacent intervals.
  for (let previous = index - 1; previous >= 0; previous--) {
    if (!hasSpeedSample(buckets[previous][metric])) continue;
    const delta = current[metric].mean! - buckets[previous][metric].mean!;
    return delta > 0 ? "up" : delta < 0 ? "down" : "equal";
  }
  return "first";
}

export function speedChartLayout(data: SpeedTrends, metric: SpeedMetric, width: number, height = 180) {
  const left = 48, right = 12, top = 8, bottom = 26;
  const plotWidth = Math.max(1, width - left - right);
  const start = data.startDate, end = data.endDate + 1, span = Math.max(1, end - start);
  const peak = Math.max(1, ...data.buckets.filter(b => hasSpeedSample(b[metric])).map(b => b[metric].max!));
  const rawStep = peak * 1.04 / 4, magnitude = 10 ** Math.floor(Math.log10(rawStep));
  const step = [1, 2, 2.5, 5, 10].find(n => n * magnitude >= rawStep)! * magnitude;
  const ceiling = Math.ceil(peak * 1.04 / step) * step;
  const x = (at: number) => left + (at - start) / span * plotWidth;
  const y = (value: number) => height - bottom - value / ceiling * (height - top - bottom);
  const bars = data.buckets.flatMap((bucket, index) => {
    const value = bucket[metric];
    if (!hasSpeedSample(value)) return [];
    const center = x((bucket.start + bucket.end) / 2);
    const width = Math.max(1, Math.min(9, (x(bucket.end) - x(bucket.start)) * .62));
    const direction = speedDirection(data.buckets, index, metric);
    return [{ index, center, width, color: direction === "up" ? SPEED_UP : SPEED_OTHER,
      meanY: y(value.mean!), minY: y(value.min!), maxY: y(value.max!),
      lowerY: y(value.centralLower!), upperY: y(value.centralUpper!) }];
  });
  const segments = bars.flatMap((bar, i) => {
    const previous = bars[i - 1];
    if (!previous || previous.index + 1 !== bar.index ||
      data.buckets[previous.index].end !== data.buckets[bar.index].start) return [];
    return [{ from: previous.index, to: bar.index, x1: previous.center, y1: previous.meanY,
      x2: bar.center, y2: bar.meanY }];
  });
  const linked = new Set(segments.flatMap(s => [s.from, s.to]));
  const stride = Math.max(1, Math.ceil(data.buckets.length / Math.max(1, Math.floor(plotWidth / 110))));
  const times = data.buckets.filter((_, i) => i % stride === 0).map(b => b.start);
  // Keep the final label from colliding with the penultimate one.
  if (times.length > 1 && x(end) - x(times[times.length - 1]) < 80) times.pop();
  times.push(end);
  return { left, right, top, bottom, x, y, bars, segments, times,
    isolated: bars.filter(b => !linked.has(b.index)),
    ticks: Array.from({ length: Math.round(ceiling / step) + 1 }, (_, i) => step * i) };
}
