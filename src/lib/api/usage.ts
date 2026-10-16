import { command, getScope } from "@/lib/client";
import type {
  UsageSummary,
  UsageSummaryByApp,
  DailyStats,
  ProviderStats,
  ModelStats,
  RequestLog,
  LogFilters,
  ModelPricing,
  ModelsDevSyncConfig,
  ModelsDevSyncState,
  PaginatedLogs,
  SessionSyncResult,
  ProviderLimitStatus,
} from "@/types/usage";
const query = <T>(kind: string, args: Record<string, unknown> = {}) =>
  command<T>("usage_query", { kind, scope: getScope(), ...args });
const filters = (
  startDate?: number,
  endDate?: number,
  appType?: string,
  providerName?: string,
  model?: string,
) => ({ startDate, endDate, appType, providerName, model });
export const usageApi = {
  getUsageSummary: (...args: Parameters<typeof filters>) =>
    query<UsageSummary>("summary", filters(...args)),
  getUsageSummaryByApp: (
    startDate?: number,
    endDate?: number,
    providerName?: string,
    model?: string,
  ) =>
    query<UsageSummaryByApp[]>("summaryByApp", {
      startDate,
      endDate,
      providerName,
      model,
    }),
  getUsageTrends: (...args: Parameters<typeof filters>) =>
    query<DailyStats[]>("trends", filters(...args)),
  getUsageFirstDate: (
    appType?: string,
    providerName?: string,
    model?: string,
  ) => query<string | null>("firstDate", { appType, providerName, model }),
  getModelStats: (...args: Parameters<typeof filters>) =>
    query<ModelStats[]>("models", filters(...args)),
  getProviderStats: (...args: Parameters<typeof filters>) =>
    query<ProviderStats[]>("providers", filters(...args)),
  getRequestLogs: (f: LogFilters, page = 0, pageSize = 20) =>
    query<PaginatedLogs>("logs", { ...f, filters: f, page, pageSize }),
  getRequestDetail: (requestId: string) =>
    query<RequestLog>("detail", { requestId }),
  getSessionUsageSummary: (appType: string, sessionId: string) =>
    query<UsageSummary>("summary", { scope: "local", appType, sessionId }),
  getSessionUsageLastSync: () => command<number | null>("last_scan"),
  syncSessionUsage: () => command<SessionSyncResult>("scan"),
  rebuildCodexUsage: () => command<SessionSyncResult>("rescan"),
  getModelPricing: () => command<ModelPricing[]>("prices"),
  updateModelPricing: (
    modelId: string,
    displayName: string,
    inputCost: string,
    outputCost: string,
    cacheReadCost: string,
    cacheCreationCost: string,
    source = "manual",
    sourceKey?: string,
  ) =>
    command<number>("save_prices", {
      entries: [
        {
          modelId,
          displayName,
          inputCostPerMillion: inputCost,
          outputCostPerMillion: outputCost,
          cacheReadCostPerMillion: cacheReadCost,
          cacheCreationCostPerMillion: cacheCreationCost,
          source,
          sourceKey,
        },
      ],
    }),
  updateModelPricingBatch: (entries: ModelPricing[]) =>
    command<number>("save_prices", { entries }),
  deleteModelPricing: (modelId: string) =>
    command<void>("delete_price", { modelId }),
  getModelsDevSyncConfig: () => command<ModelsDevSyncState>("models_config"),
  saveModelsDevSyncConfig: (config: ModelsDevSyncConfig) =>
    command<ModelsDevSyncState>("save_models_config", { config }),
  recordModelsDevSyncResult: (syncedAt: number | null, error: string | null) =>
    command<void>("record_models_sync", { syncedAt, error }),
  checkProviderLimits: (
    _providerId: string,
    _appType: string,
  ): Promise<ProviderLimitStatus> =>
    Promise.reject(
      new Error("Provider limits are not collected from session logs"),
    ),
};
