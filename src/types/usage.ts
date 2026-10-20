// 使用统计相关类型定义
import type { GoTariff } from "./goCatalog";

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
}

export interface RequestLog {
  nativeProvider?: string | null;
  billingBasis?: "subscription_usage" | "model_reference" | null;
  costConverted?: boolean;
  costConversion?: {
    currency: string;
    originalAmount: string;
    unitsPerUsd: string | null;
    configuredAt: number;
  } | null;
  pricingContext?: {
    period: "peak" | "off_peak";
    timestamp: number;
    timeBasis: string;
    calendarVersion: string;
  } | null;
  pricingRuleVersion?: string;
  deviceId?: string;
  costSource?: string;
  priceSource?: string;
  recordSource?: string;
  pricingSnapshot?: unknown;
  requestId: string;
  providerId: string;
  providerName?: string;
  appType: string;
  model: string;
  requestModel?: string;
  /** 写入时实际用于计价的模型名；路由接管 + request 计价模式下可能与 model 不同 */
  pricingModel?: string;
  costMultiplier: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  inputCostUsd: string;
  outputCostUsd: string;
  cacheReadCostUsd: string;
  cacheCreationCostUsd: string;
  totalCostUsd: string;
  isStreaming: boolean;
  latencyMs: number | null;
  /** 路由服务记的是精确首字；会话日志导入的是按时间戳推出来的估算值（看 dataSource） */
  firstTokenMs?: number | null;
  durationMs?: number | null;
  timingMethod?: "estimated";
  timingSource?: string;
  firstTokenSource?: string | null;
  /** Query-time speed policy; absent on older local services. */
  estimatedSpeed?: SessionSpeedEstimate;
  statusCode: number | null;
  errorMessage?: string;
  createdAt: number;
  dataSource?: string;
}

export interface SessionSyncResult {
  imported: number;
  skipped: number;
  filesScanned: number;
  suspectedDuplicates: number;
  deferredFiles: number;
  errors: string[];
}

export interface SessionSpeedEstimate {
  basis: "decode" | "end_to_end";
  durationMs: number | null;
  eligible: boolean;
  minOutputTokens: number;
  minDurationMs: number;
}

export interface DataSourceSummary {
  dataSource: string;
  requestCount: number;
  totalCostUsd: string;
}

export interface PaginatedLogs {
  data: RequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface ModelPricing {
  goTariff?: GoTariff;
  provider?: string | null;
  billingBasis?: "subscription_usage" | null;
  currency?: string;
  pricingRule?: string;
  source?: string;
  sourceKey?: string;
  fetchedAt?: number;
  modelId: string;
  displayName: string;
  inputCostPerMillion: string;
  outputCostPerMillion: string;
  cacheReadCostPerMillion: string;
  cacheCreationCostPerMillion: string;
}

export interface PricingSettings {
  exchangeRates: Record<string, string>;
  holidayCalendars: Record<string, string[]>;
  updatedAt: number;
}

export interface ModelsDevSyncConfig {
  autoSyncEnabled: boolean;
  includeCommonModels: boolean;
  selectedModelKeys: string[];
  excludedCommonModelKeys: string[];
  lastSyncAt: number | null;
  lastSyncError: string | null;
}

export interface ModelsDevSyncState {
  config: ModelsDevSyncConfig;
  configPath: string;
}

export interface UsageSummary {
  subscriptionUsageCount?: number;
  costConverted?: boolean;
  unpricedCount?: number;
  knownStatusCount?: number;
  totalRequests: number;
  totalCost: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  successRate: number | null;
  /** input + output + cache_creation + cache_read, all cache-normalized */
  realTotalTokens: number;
  /** cache_read / (input + cache_creation + cache_read), range 0–1 */
  cacheHitRate: number;
}

export interface UsageSummaryByApp {
  appType: string;
  summary: UsageSummary;
}

export interface DailyStats {
  costConverted?: boolean;
  date: string;
  requestCount: number;
  totalCost: string;
  totalTokens: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
}

export interface ProviderStats {
  costConverted?: boolean;
  providerId: string;
  appType: string;
  providerName: string;
  requestCount: number;
  /** 真实消耗 Tokens（新增输入 + 输出 + 缓存写入 + 缓存命中），与指标卡同口径 */
  totalTokens: number;
  totalCost: string;
  successRate: number | null;
  avgLatencyMs: number | null;
  /** 速度分子：有首字、输出 ≥ 100 token 的明细请求的输出之和（日汇总不计） */
  speedOutputTokens?: number;
  /** 速度分母：同一批请求的（耗时 − 首字）之和，毫秒 */
  speedGenerationMs?: number;
  /** 输出阶段估算，独立于含等待的端到端样本，不能混合相加。 */
  estDecodeSpeedOutputTokens?: number;
  estDecodeSpeedGenerationMs?: number;
  estDecodeSpeedSampleCount?: number;
  /** 端到端估算分子：输出 ≥ 200 token、总耗时 ≥ 1s 的回退样本 */
  estSpeedOutputTokens?: number;
  /** 估算速度分母：同一批请求的估算耗时（含首字等待）之和，毫秒 */
  estSpeedDurationMs?: number;
  estSpeedSampleCount?: number;
  /** 首字时间中位数（毫秒）：经路由服务、成功的流式明细请求；没有时为 null */
  firstTokenMedianMs?: number | null;
  /** 估算首字中位数（毫秒）：会话日志导入、按时间戳推出了首字的请求；没有精确值时才显示，带 ≈ */
  estFirstTokenMedianMs?: number | null;
}

export interface ModelStats {
  costConverted?: boolean;
  model: string;
  requestCount: number;
  /** 真实消耗 Tokens（新增输入 + 输出 + 缓存写入 + 缓存命中），与指标卡同口径 */
  totalTokens: number;
  totalCost: string;
  avgCostPerRequest: string;
  successRate: number | null;
  /** 速度分子分母与样本数，口径同 ProviderStats */
  speedOutputTokens?: number;
  speedGenerationMs?: number;
  estDecodeSpeedOutputTokens?: number;
  estDecodeSpeedGenerationMs?: number;
  estDecodeSpeedSampleCount?: number;
  estSpeedOutputTokens?: number;
  estSpeedDurationMs?: number;
  estSpeedSampleCount?: number;
  /** 首字时间中位数，口径同 ProviderStats 的那两个字段 */
  firstTokenMedianMs?: number | null;
  estFirstTokenMedianMs?: number | null;
}

export interface LogFilters {
  appType?: string;
  providerName?: string;
  model?: string;
  statusCode?: number;
  startDate?: number;
  endDate?: number;
}

/**
 * Dashboard 顶栏的全局筛选维度，作用于 Hero / 趋势图 / 三个统计 Tab。
 *
 * - `providerName` 按原始供应商 ID 或会话占位名匹配，翻译标签只用于显示；
 * - `model` 按日志模型原文匹配，与模型统计的分组口径一致。
 */
export interface UsageScopeFilters {
  appType?: string;
  providerName?: string;
  model?: string;
}

export interface ProviderLimitStatus {
  providerId: string;
  dailyUsage: string;
  dailyLimit?: string;
  dailyExceeded: boolean;
  monthlyUsage: string;
  monthlyLimit?: string;
  monthlyExceeded: boolean;
}

export type UsageRangePreset =
  "today" | "1d" | "7d" | "14d" | "30d" | "all" | "custom";

export interface UsageRangeSelection {
  preset: UsageRangePreset;
  customStartDate?: number;
  customEndDate?: number;
  /** When true (custom mode only), endDate resolves to "now" instead of the
   *  fixed customEndDate snapshot, and the end-time field becomes read-only. */
  liveEndTime?: boolean;
}

/**
 * App types surfaced as dashboard filter buttons.
 *
 * `claude-desktop` is intentionally NOT listed: the Desktop gateway's proxy
 * traffic is still recorded under its own `app_type` (preserving route-takeover
 * billing audit — the request detail panel shows the real value), but the
 * dashboard folds it into `claude` for display. It is the embedded Claude Code
 * runtime running inside the Desktop shell, and Desktop *chat* usage never
 * passes through this app at all, so a separate "Claude Desktop" bucket would
 * only ever show a partial number and mislead users into reading it as the
 * Desktop's full usage. The backend collapses `claude-desktop → claude` in
 * every dashboard query (see `folded_app_type_sql`).
 * `opencode` and `pi` have no proxy handler; their usage reaches this
 * dashboard through session importers. `openclaw` / `hermes` appear only as
 * managed apps elsewhere.
 */
export type AppType =
  "claude" | "codex" | "gemini" | "grokbuild" | "opencode" | "pi" | "mcode";

export type AppTypeFilter = "all" | AppType;

export const KNOWN_APP_TYPES: ReadonlyArray<AppType> = ["claude", "codex"];

/**
 * App types whose proxy uses an OpenAI-style protocol. Two consequences:
 *
 * 1. `inputTokens` already includes the cached portion (must subtract
 *    `cacheReadTokens` to get fresh-input semantics — see
 *    [getFreshInputTokens]).
 * 2. The protocol does not report cache _creation_ separately, only cache
 *    _reads_. So `cacheCreationTokens` is always 0 for these app types and
 *    the UI should label it as N/A rather than 0.
 *
 * Mirror of the Rust `CACHE_INCLUSIVE_APP_TYPES` whitelist.
 */
export const CACHE_INCLUSIVE_APP_TYPES: ReadonlySet<string> = new Set([
  "codex",
  "gemini",
  "grokbuild",
]);

// Pi sessions can mix Anthropic and OpenAI APIs, but the dashboard aggregates
// only by app type. Treat cache-write coverage as partial without changing
// Pi's fresh-input token semantics.
const PARTIAL_CACHE_WRITE_APP_TYPES: ReadonlySet<string> = new Set([
  "pi",
  "mcode",
]);

export type CacheWriteAvailability = "ok" | "partial" | "na";

export function getCacheWriteAvailability(
  appTypes: readonly string[],
): CacheWriteAvailability {
  if (appTypes.length === 0) return "ok";
  const unavailable = appTypes.filter((appType) =>
    CACHE_INCLUSIVE_APP_TYPES.has(appType),
  ).length;
  if (unavailable === appTypes.length) return "na";
  const partial = appTypes.some((appType) =>
    PARTIAL_CACHE_WRITE_APP_TYPES.has(appType),
  );
  return unavailable === 0 && !partial ? "ok" : "partial";
}

/** Subset of request-log fields needed to derive cache-normalized input. */
export interface CacheNormalizableLog {
  appType: string;
  inputTokens: number;
  cacheReadTokens: number;
}

/**
 * For a single request log, return the input token count with cache reads
 * removed. Anthropic-style providers already report `inputTokens` without
 * cache, so they pass through unchanged.
 */
// Harness Scope APIs normalize all input to fresh semantics in the core.
export function getFreshInputTokens(log: CacheNormalizableLog): number {
  return log.inputTokens;
}

export const NON_NEGATIVE_DECIMAL_REGEX = /^\d+(?:\.\d+)?$/;

export function isNonNegativeDecimalString(value: string): boolean {
  const trimmed = value.trim();
  if (!NON_NEGATIVE_DECIMAL_REGEX.test(trimmed)) return false;
  return Number.isFinite(Number(trimmed));
}

type UsageCostLog = Pick<
  RequestLog,
  | "inputTokens"
  | "outputTokens"
  | "cacheReadTokens"
  | "cacheCreationTokens"
  | "totalCostUsd"
  | "statusCode"
> &
  Partial<Pick<RequestLog, "costMultiplier">>;

export function hasUsageTokens(log: UsageCostLog): boolean {
  return (
    log.inputTokens > 0 ||
    log.outputTokens > 0 ||
    log.cacheReadTokens > 0 ||
    log.cacheCreationTokens > 0
  );
}

export function isUnpricedUsage(log: UsageCostLog): boolean {
  return log.totalCostUsd == null && hasUsageTokens(log);
}

// Test the decimal text directly: a tiny paid amount must not underflow to zero.
function isExactZero(value: unknown): boolean {
  return (
    value === 0 ||
    (typeof value === "string" && /^0+(?:\.0+)?$/.test(value.trim()))
  );
}

/** Free needs model/rate evidence; reference prices retain their billing-basis hint. */
export function isFreeUsage(log: RequestLog): boolean {
  if (!hasUsageTokens(log) || !isExactZero(log.totalCostUsd)) return false;

  if (log.costSource === "reported") {
    return (
      log.billingBasis !== "model_reference" &&
      ["opencode", "opencode-go"].includes(log.nativeProvider ?? "") &&
      log.model.replace(/^opencode(?:-go)?\//, "").endsWith("-free")
    );
  }

  const snapshot = log.pricingSnapshot;
  if (
    log.costSource !== "estimated" ||
    !snapshot ||
    typeof snapshot !== "object"
  )
    return false;
  const price = snapshot as Record<string, unknown>;
  return (
    !price.pricingRule &&
    [
      price.inputCostPerMillion,
      price.outputCostPerMillion,
      price.cacheReadCostPerMillion,
      price.cacheCreationCostPerMillion,
    ].every(isExactZero)
  );
}

export interface StatsFilters {
  timeRange: UsageRangePreset;
  providerId?: string;
  appType?: string;
}
