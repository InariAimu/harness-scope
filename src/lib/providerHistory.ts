import type {
  ProviderAccountObservation,
  ProviderData,
} from "@/types/providerAccounts";

export interface HistoryMetric {
  key: string;
  kind: "balance" | "custom" | "quota";
  name: string;
  unit: string;
  bucketId: string | null;
  id: string;
  field: "remaining" | "used" | "total";
  durationMinutes?: number;
}
export interface HistoryPoint {
  at: number;
  sequence: number;
  value: number | null;
  segment: string;
  sample?: ProviderAccountObservation;
  /** Display-only start of a known quota cycle; never an observation for estimates. */
  resetAnchor?: true;
}

function numeric(value: unknown) {
  if (value == null || value === "") return null;
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}
/** Capabilities come from returned data; a manual subscription is not a quota. */
export function historyMetrics(data?: ProviderData): HistoryMetric[] {
  const metrics: HistoryMetric[] = [];
  const add = (
    kind: HistoryMetric["kind"],
    id: string,
    name: string,
    unit: string,
    bucketId: string | null = null,
    field: HistoryMetric["field"] = "remaining",
    durationMinutes?: number,
  ) => {
    metrics.push({
      key: JSON.stringify([kind, bucketId, id, field, unit]),
      kind,
      name,
      unit,
      bucketId,
      id,
      field,
      durationMinutes,
    });
  };
  if (data?.kind === "balance")
    data.balances.forEach((b) =>
      add("balance", b.currency, b.currency, b.currency),
    );
  if (data?.kind === "customUsage")
    data.items.forEach((item) => {
      const field =
        item.remaining != null
          ? "remaining"
          : item.used != null
            ? "used"
            : "total";
      if (item[field] != null)
        add(
          "custom",
          item.id,
          item.planName ?? item.id,
          item.unit ?? "",
          null,
          field,
        );
      if (item.usedPercent != null)
        add(
          "quota",
          item.id,
          item.planName ?? item.id,
          "%",
          null,
          "remaining",
          item.durationMinutes ?? undefined,
        );
    });
  if (data?.kind === "quota")
    data.windows.forEach((w) =>
      add("quota", w.id, w.id, "%", null, "remaining", w.durationMinutes),
    );
  if (data?.kind === "codexQuota")
    data.buckets.forEach((b) =>
      b.windows.forEach((w) =>
        add(
          "quota",
          w.id,
          b.name ?? b.id,
          "%",
          b.id,
          "remaining",
          w.durationMinutes,
        ),
      ),
    );
  return metrics;
}

export function observationPoint(
  sample: ProviderAccountObservation,
  metric: HistoryMetric,
): HistoryPoint {
  const data = sample.data;
  let value: number | null = null;
  let boundary: unknown = null;
  if (metric.kind === "quota") {
    const w = sample.windows.find(
      (w) => w.bucketId === metric.bucketId && w.windowId === metric.id,
    );
    if (
      w &&
      ["ok", "rate-limited"].includes(w.status) &&
      (w.resetsAt == null || w.resetsAt * 1000 > sample.observedAtMs) &&
      (w.startsAt == null || w.startsAt * 1000 <= sample.observedAtMs)
    )
      value = numeric(w.remainingPercent);
    if (value != null && (value < 0 || value > 100)) value = null;
    const custom =
      data.kind === "customUsage"
        ? data.items.find((i) => i.id === metric.id)
        : null;
    const bucket =
      data.kind === "codexQuota"
        ? data.buckets.find((b) => b.id === metric.bucketId)
        : null;
    boundary = [
      w?.resetsAt,
      w?.durationMinutes,
      bucket?.planType,
      custom?.windowType,
      custom?.unit,
      custom?.total,
      custom?.planName,
    ];
  } else if (metric.kind === "balance" && data.kind === "balance") {
    value = numeric(data.balances.find((b) => b.currency === metric.id)?.total);
  } else if (metric.kind === "custom" && data.kind === "customUsage") {
    const item = data.items.find(
      (i) => i.id === metric.id && (i.unit ?? "") === metric.unit,
    );
    value = numeric(item?.[metric.field]);
    boundary = [item?.resetsAt, item?.windowType];
  }
  const segment = JSON.stringify([
    sample.subscriptionId ? (sample.provider.startsWith("custom") ? "custom" : sample.provider) : sample.provider,
    sample.subscriptionId ?? sample.accountRevision,
    data.identity,
    data.identitySource,
    data.kind === "codexQuota" ? data.planType : null,
    boundary,
  ]);
  return {
    at: sample.observedAtMs,
    sequence: sample.sequence,
    value,
    segment,
    sample,
  };
}

/** Preserve raw samples; omit stale quota readings only within known fixed reset windows. */
export function historyPoints(
  samples: ProviderAccountObservation[],
  metric: HistoryMetric,
  maxGapMs: number,
) {
  const ordered = [...samples].sort(
    (a, b) => a.observedAtMs - b.observedAtMs || a.refreshId.localeCompare(b.refreshId),
  );
  const points: HistoryPoint[] = [];
  const revisions = new Map<string, string>();
  const remainingFloor = new Map<string, number>();
  let epoch = 0;
  for (const sample of ordered) {
    const point = observationPoint(sample, metric);
    if (sample.subscriptionId) {
      const source = sample.deviceId ?? "local";
      const previousRevision = revisions.get(source);
      if (previousRevision != null && previousRevision !== sample.accountRevision) epoch++;
      revisions.set(source, sample.accountRevision);
      point.segment = `${point.segment}:${epoch}`;
    }
    const w = sample.windows.find((w) => w.bucketId === metric.bucketId && w.windowId === metric.id);
    const fixed = metric.kind === "quota" && w?.resetsAt != null &&
      (sample.data.kind !== "customUsage" || sample.data.items.find((i) => i.id === metric.id)?.windowType === "fixed");
    if (fixed && point.value != null) {
      const floor = remainingFloor.get(point.segment);
      if (floor != null && point.value > floor) continue;
      remainingFloor.set(point.segment, point.value);
    }
    const previous = points.at(-1);
    if (
      previous &&
      (previous.segment !== point.segment ||
        point.at - previous.at > maxGapMs ||
        (metric.kind === "quota" &&
          previous.value != null &&
          point.value != null &&
          point.value > previous.value))
    ) {
      points.push({ ...point, value: null, sample: undefined });
    }
    points.push(point);
  }
  return points;
}

/** Join display gaps; only a known 100% cycle start begins a new line. */
export function historyPlotPoints(
  points: HistoryPoint[],
  metric: HistoryMetric,
  startAtMs: number,
): HistoryPoint[] {
  const samples = points.filter((p) => p.sample && p.value != null);
  const anchors = new Map<number, HistoryPoint>();
  if (metric.kind === "quota") {
    for (const point of samples) {
      const w = point.sample!.windows.find(
        (w) => w.bucketId === metric.bucketId && w.windowId === metric.id,
      );
      const anchor = w?.fullQuotaAnchor;
      if (
        !anchor || !w.activeAtSample || w.resetsAt == null ||
        w.durationMinutes == null || w.durationMinutes <= 0 ||
        anchor.at !== w.startsAt || anchor.at !== w.resetsAt - w.durationMinutes * 60
      ) continue;
      const at = anchor.at * 1000;
      if (!Number.isSafeInteger(at) || at < 0 || at < startAtMs || at > point.at) continue;
      anchors.set(at, { at, value: 100, sequence: 0, segment: `reset:${at}`, resetAnchor: true });
    }
  }
  // Keep a real 100% reading at the exact reset time instead of drawing it twice.
  const observedFull = new Set(samples.filter((p) => p.value === 100).map((p) => p.at));
  const ordered = [
    ...samples,
    ...[...anchors.values()].filter((p) => !observedFull.has(p.at)),
  ].sort((a, b) => a.at - b.at || Number(!!b.resetAnchor) - Number(!!a.resetAnchor));
  const result: HistoryPoint[] = [];
  const breaks = new Set<number>();
  for (const point of ordered) {
    if (anchors.has(point.at) && !breaks.has(point.at)) {
      if (result.length) {
        result.push({ at: point.at, value: null, sequence: 0, segment: `reset:${point.at}` });
      }
      breaks.add(point.at);
    }
    result.push(point);
  }
  return result;
}

export function defaultHistoryPair(points: HistoryPoint[]) {
  const valid = points.filter((p) => p.sample && p.value != null);
  const last = valid.at(-1);
  if (!last) return [];
  // A break, even one which later returns to the same key, ends the usable segment.
  const lastIndex = points.indexOf(last);
  let start = lastIndex;
  while (
    start > 0 &&
    points[start - 1].sample &&
    points[start - 1].value != null &&
    points[start - 1].segment === last.segment
  )
    start--;
  const first = points
    .slice(start, lastIndex)
    .find((p) => last.at - p.at >= 60_000);
  return first ? [first.sequence, last.sequence] : [];
}

export function historyEstimateReason(reason: string | null, zh: boolean) {
  const messages: Record<string, [string, string]> = {
    insufficient_span: [
      "两个采样点需相隔至少 1 分钟，且时间不能晚于当前。",
      "Choose observations at least one minute apart, before the current time.",
    ],
    account_changed: [
      "账号配置已修改，请在最近一次保存之后重新采样。",
      "Account settings changed. Collect observations after the latest save.",
    ],
    different_window: [
      "所选区间跨账号版本、套餐或窗口重置，请重新选择同一窗口内的点。",
      "The interval crosses an account, plan or window boundary. Choose points in one window.",
    ],
    unverified_identity: [
      "当前快照无法验证 Codex 账号身份，暂不能测算。",
      "The Codex account identity cannot be verified.",
    ],
    unsupported_window: [
      "此查询未声明固定窗口，无法把净变化当作额度消耗。",
      "This query does not declare a fixed window; net changes cannot establish consumption.",
    ],
    missing_window: [
      "所选区间有缺失或不匹配的窗口数据。",
      "Window data is missing or inconsistent in this interval.",
    ],
    missing_reset: [
      "缺少窗口重置时间，无法确认两点属于同一周期。",
      "A reset time is required to establish a shared window.",
    ],
    expired_sample: [
      "采样点落在窗口有效时间之外，请刷新后重新采样。",
      "An observation is outside its active window. Refresh and collect new samples.",
    ],
    unknown_quota: [
      "所选区间有未知额度状态，请缩小采样范围。",
      "Quota status is unknown within the interval. Select a narrower interval.",
    ],
    quota_out_of_range: [
      "额度已超出 0–100%，不能可靠反推总量。",
      "Quota is outside 0–100%; capacity cannot be inferred reliably.",
    ],
    quota_decreased: [
      "结束点的额度读数已回退，请选择未被筛除的最新采样点。",
      "The ending quota reading regressed. Choose a latest sample that has not been filtered out.",
    ],
    invalid_time: [
      "区间内的采样时钟不连续，请重新选择采样点。",
      "Observation timestamps are inconsistent. Choose another interval.",
    ],
    insufficient_delta: [
      "消耗不足 1 个百分点，请扩大同一窗口内的采样范围。",
      "Consumption is below one percentage point. Choose a longer interval in the same window.",
    ],
    no_links: [
      "请先在账号编辑中关联用量路由，再重新采样。",
      "Link usage routes in Edit, then collect new samples.",
    ],
    unsupported_bucket: [
      "此额度桶未关联模型计价规则，无法可靠归集同期消耗。",
      "This quota bucket is not mapped to model pricing rules.",
    ],
    legacy_usage: [
      "同期包含旧的按日汇总，无法精确分配到两个采样点之间。",
      "Legacy daily aggregates cannot be assigned to this exact interval.",
    ],
    unpriced_usage: [
      "同期有未定价请求，请补全价表后重试。",
      "Some concurrent requests are unpriced. Complete their rates and retry.",
    ],
    no_priced_usage: [
      "同期没有可计价的关联请求，无法反推额度。",
      "No attributable priced requests are available in this interval.",
    ],
    precision_limit: [
      "结果超出可计算范围，请检查单价和样本。",
      "The result exceeds the supported range. Check rates and samples.",
    ],
  };
  return reason ? (messages[reason]?.[zh ? 0 : 1] ?? reason) : "";
}
