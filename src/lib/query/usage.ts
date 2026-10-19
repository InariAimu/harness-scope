import {
  keepPreviousData,
  type QueryClient,
  useQuery,
  useMutation,
  useQueryClient,
} from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { getScope } from "@/lib/client";
import { resolveUsageRange } from "@/lib/usageRange";
import { useUsageTimezone } from "./usageTimezone";
import type {
  LogFilters,
  UsageRangeSelection,
  UsageScopeFilters,
} from "@/types/usage";

const DEFAULT_REFETCH_INTERVAL_MS = 30000;
const USAGE_STALE_TIME_MS = 30000;

/** Warm only the harness the user points to; reuse exactly the visible query keys. */
export function prefetchHarnessUsage(
  client: QueryClient,
  range: UsageRangeSelection,
  appType: string,
  timeZone: string,
  statusCode?: number,
) {
  const effective = normalizeScopeFilters({ appType });
  const bounds = resolveUsageRange(range, Date.now(), timeZone);
  return Promise.all([
    client.prefetchQuery({
      queryKey: [
        ...usageKeys.trends(
          range.preset,
          range.customStartDate,
          range.customEndDate,
          effective,
          range.liveEndTime,
        ),
        timeZone,
      ],
      queryFn: () =>
        usageApi.getUsageTrends(
          bounds.startDate,
          bounds.endDate,
          effective.appType,
        ),
      staleTime: USAGE_STALE_TIME_MS,
    }),
    client.prefetchQuery({
      queryKey: [
        ...usageKeys.logs(
          { ...range, appType: effective.appType, statusCode },
          0,
          20,
        ),
        timeZone,
      ],
      queryFn: () =>
        usageApi.getRequestLogs(
          { ...bounds, appType: effective.appType, statusCode },
          0,
          20,
        ),
      staleTime: USAGE_STALE_TIME_MS,
    }),
  ]);
}

type UsageQueryOptions = {
  enabled?: boolean;
  refetchInterval?: number | false;
  refetchIntervalInBackground?: boolean;
};

type RequestLogsQueryArgs = {
  filters: LogFilters;
  range: UsageRangeSelection;
  page?: number;
  pageSize?: number;
  options?: UsageQueryOptions;
};

type RequestLogsKey = {
  preset: UsageRangeSelection["preset"];
  customStartDate?: number;
  customEndDate?: number;
  liveEndTime?: boolean;
  appType?: string;
  providerName?: string;
  model?: string;
  statusCode?: number;
};

// Query keys
export const usageKeys = {
  all: ["usage"] as const,
  hasRecords: () => ["usage", "has-records", getScope()] as const,
  summary: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  summaryByApp: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: Pick<UsageScopeFilters, "providerName" | "model">,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary-by-app",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  trends: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "trends",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  firstDate: (filters?: UsageScopeFilters) =>
    [
      ...usageKeys.all,
      "first-date",
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  providerStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "provider-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  modelStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "model-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
      getScope(),
    ] as const,
  logs: (key: RequestLogsKey, page: number, pageSize: number) =>
    [
      ...usageKeys.all,
      "logs",
      key.preset,
      key.customStartDate ?? 0,
      key.customEndDate ?? 0,
      key.liveEndTime ?? false,
      key.appType ?? "",
      key.providerName ?? "",
      key.model ?? "",
      key.statusCode ?? -1,
      page,
      pageSize,
      getScope(),
    ] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, "detail", requestId] as const,
  pricing: () => [...usageKeys.all, "pricing"] as const,
  session: (appType: string, sessionId: string) =>
    [...usageKeys.all, "session", appType, sessionId] as const,
  limits: (providerId: string, appType: string) =>
    [...usageKeys.all, "limits", providerId, appType] as const,
  script: (providerId: string, appType: string) =>
    [...usageKeys.all, providerId, appType] as const,
};

/** 把 UI 侧的 "all" 哨兵归一成 undefined（后端语义：不过滤）。 */
function normalizeScopeFilters(filters?: UsageScopeFilters): UsageScopeFilters {
  return {
    appType: filters?.appType === "all" ? undefined : filters?.appType,
    providerName: filters?.providerName,
    model: filters?.model,
  };
}

// Hooks
// 统计类查询都带 keepPreviousData：换筛选、时间范围、翻页时先留着上一份数据，
// 新数据到了再换，不让指标闪成「…」、请求日志表塌成骨架。
export function useUsageSummary(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: [
      ...usageKeys.summary(
        range.preset,
        range.customStartDate,
        range.customEndDate,
        effective,
        range.liveEndTime,
      ),
      timeZone,
    ],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(
        range,
        Date.now(),
        timeZone,
      );
      return usageApi.getUsageSummary(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useUsageSummaryByApp(
  range: UsageRangeSelection,
  filters?: Pick<UsageScopeFilters, "providerName" | "model">,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  return useQuery({
    queryKey: [
      ...usageKeys.summaryByApp(
        range.preset,
        range.customStartDate,
        range.customEndDate,
        filters,
        range.liveEndTime,
      ),
      timeZone,
    ],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(
        range,
        Date.now(),
        timeZone,
      );
      return usageApi.getUsageSummaryByApp(
        startDate,
        endDate,
        filters?.providerName,
        filters?.model,
      );
    },
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useUsageTrends(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: [
      ...usageKeys.trends(
        range.preset,
        range.customStartDate,
        range.customEndDate,
        effective,
        range.liveEndTime,
      ),
      timeZone,
    ],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(
        range,
        Date.now(),
        timeZone,
      );
      return usageApi.getUsageTrends(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    enabled: options?.enabled ?? true,
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

/** 最早有用量记录的日期，「全部」的按年热力图据此决定从哪一年画起 */
export function useUsageFirstDate(
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: [...usageKeys.firstDate(effective), timeZone],
    queryFn: () =>
      usageApi.getUsageFirstDate(
        effective.appType,
        effective.providerName,
        effective.model,
      ),
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useProviderStats(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: [
      ...usageKeys.providerStats(
        range.preset,
        range.customStartDate,
        range.customEndDate,
        effective,
        range.liveEndTime,
      ),
      timeZone,
    ],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(
        range,
        Date.now(),
        timeZone,
      );
      return usageApi.getProviderStats(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useModelStats(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const timeZone = useUsageTimezone();
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: [
      ...usageKeys.modelStats(
        range.preset,
        range.customStartDate,
        range.customEndDate,
        effective,
        range.liveEndTime,
      ),
      timeZone,
    ],
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(
        range,
        Date.now(),
        timeZone,
      );
      return usageApi.getModelStats(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useRequestLogs({
  filters,
  range,
  page = 0,
  pageSize = 20,
  options,
}: RequestLogsQueryArgs) {
  const timeZone = useUsageTimezone();
  const key: RequestLogsKey = {
    preset: range.preset,
    customStartDate: range.customStartDate,
    customEndDate: range.customEndDate,
    liveEndTime: range.liveEndTime,
    appType: filters.appType,
    providerName: filters.providerName,
    model: filters.model,
    statusCode: filters.statusCode,
  };

  return useQuery({
    queryKey: [...usageKeys.logs(key, page, pageSize), timeZone],
    queryFn: () => {
      const effectiveFilters = {
        ...filters,
        ...resolveUsageRange(range, Date.now(), timeZone),
      };
      return usageApi.getRequestLogs(effectiveFilters, page, pageSize);
    },
    staleTime: USAGE_STALE_TIME_MS,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS, // 每30秒自动刷新
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useRequestDetail(requestId: string) {
  return useQuery({
    queryKey: usageKeys.detail(requestId),
    queryFn: () => usageApi.getRequestDetail(requestId),
    enabled: !!requestId,
  });
}

/**
 * 会话日志扫描（后台定时或手动同步）最近一次完成的时间（毫秒）。
 * 默认 30 秒查询一次；用量页由统一刷新流程在扫描结束后失效重取。
 */
export function useSessionUsageLastSync(options?: UsageQueryOptions) {
  return useQuery({
    queryKey: [...usageKeys.all, "session-last-sync"] as const,
    queryFn: () => usageApi.getSessionUsageLastSync(),
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

/** 单个会话的用量汇总（会话阅读页头部），只数会话日志导入的行。 */
export function useSessionUsageSummary(appType: string, sessionId: string) {
  return useQuery({
    queryKey: usageKeys.session(appType, sessionId),
    queryFn: () => usageApi.getSessionUsageSummary(appType, sessionId),
    refetchInterval: DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });
}

export function useModelPricing() {
  return useQuery({
    queryKey: usageKeys.pricing(),
    queryFn: usageApi.getModelPricing,
  });
}

export function useProviderLimits(providerId: string, appType: string) {
  return useQuery({
    queryKey: usageKeys.limits(providerId, appType),
    queryFn: () => usageApi.checkProviderLimits(providerId, appType),
    enabled: !!providerId && !!appType,
  });
}

export function useUpdateModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: {
      modelId: string;
      displayName: string;
      inputCost: string;
      outputCost: string;
      cacheReadCost: string;
      cacheCreationCost: string;
      source?: string;
      sourceKey?: string;
      currency?: string;
      provider?: string | null;
    }) =>
      usageApi.updateModelPricing(
        params.modelId,
        params.displayName,
        params.inputCost,
        params.outputCost,
        params.cacheReadCost,
        params.cacheCreationCost,
        params.source,
        params.sourceKey,
        params.currency,
        params.provider,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.all });
    },
  });
}

export function useDeleteModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (price: { modelId: string; provider?: string | null }) =>
      usageApi.deleteModelPricing(price.modelId, price.provider),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.all });
    },
  });
}
