import { usageApi } from "@/lib/api/usage";
import { fetchModelsDev } from "@/lib/modelsDev";
import {
  flattenModels,
  resolveModelsDevSelection,
  resolveUsageModelReferences,
  toModelPricing,
} from "@/lib/modelsDevPricing";
import type { ModelsDevSyncState } from "@/types/usage";

export interface ModelsDevSyncResult {
  skipped: boolean;
  selected: number;
  imported: number;
  changed: number;
  usageChanged: number;
  syncedAt: number | null;
}

export const MODELS_DEV_SYNC_CONFIG_QUERY_KEY = [
  "models-dev-sync-config",
] as const;
export const MODELS_DEV_STARTUP_SYNC_INTERVAL_MS = 6 * 60 * 60 * 1000;

const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
const referenceAttempts = new Map<string, number>();

export async function syncModelsDevPricing(
  state?: ModelsDevSyncState,
  force = false,
): Promise<ModelsDevSyncResult> {
  const initialState = state ?? (await usageApi.getModelsDevSyncConfig());
  const missingModels = (
    initialState.config.autoSyncEnabled || force
      ? await usageApi.getModelsNeedingPrices()
      : []
  ).filter(
    (model) =>
      force ||
      Date.now() - (referenceAttempts.get(model) ?? 0) >=
        MODELS_DEV_STARTUP_SYNC_INTERVAL_MS,
  );
  const recentlySynced =
    initialState.config.lastSyncAt !== null &&
    Date.now() - initialState.config.lastSyncAt <
      MODELS_DEV_STARTUP_SYNC_INTERVAL_MS;
  if (
    !force &&
    (!initialState.config.autoSyncEnabled ||
      (recentlySynced && missingModels.length === 0))
  ) {
    return {
      skipped: true,
      selected: 0,
      imported: 0,
      changed: 0,
      usageChanged: 0,
      syncedAt: initialState.config.lastSyncAt,
    };
  }

  try {
    missingModels.forEach((model) => referenceAttempts.set(model, Date.now()));
    const data = await fetchModelsDev();
    const latestState = await usageApi.getModelsDevSyncConfig();
    if (!force && !latestState.config.autoSyncEnabled) {
      return {
        skipped: true,
        selected: 0,
        imported: 0,
        changed: 0,
        usageChanged: 0,
        syncedAt: latestState.config.lastSyncAt,
      };
    }
    const entries = flattenModels(data);
    const selectedEntries = resolveModelsDevSelection(
      entries,
      latestState.config,
    );
    const pricing = toModelPricing([
      ...resolveUsageModelReferences(entries, missingModels),
      ...selectedEntries,
    ]);
    const result = pricing.length
      ? await usageApi.updateModelPricingBatchDetailed(pricing)
      : { saved: 0, usageChanged: 0 };
    const syncedAt = Date.now();
    await usageApi.recordModelsDevSyncResult(syncedAt, null);
    return {
      skipped: false,
      selected: selectedEntries.length,
      imported: pricing.length,
      changed: result.saved,
      usageChanged: result.usageChanged,
      syncedAt,
    };
  } catch (error) {
    try {
      await usageApi.recordModelsDevSyncResult(null, errorMessage(error));
    } catch (saveError) {
      console.warn(
        "[models.dev] Failed to persist automatic sync error",
        saveError,
      );
    }
    throw error;
  }
}

let startupSync: Promise<ModelsDevSyncResult> | null = null;
let lastScan: number | null | undefined;

/** Check once per completed scan; throttle network sync and unresolved references. */
export function syncModelsDevPricingOnStartup(
  scanAt?: number | null,
): Promise<ModelsDevSyncResult> {
  if (scanAt !== lastScan) {
    lastScan = scanAt;
    startupSync =
      startupSync?.then(
        () => syncModelsDevPricing(),
        () => syncModelsDevPricing(),
      ) ?? null;
  }
  startupSync ??= syncModelsDevPricing();
  return startupSync;
}
