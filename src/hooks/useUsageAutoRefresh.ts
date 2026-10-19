import { useCallback, useEffect, useRef } from "react";
import { focusManager, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import type { SessionSyncResult } from "@/types/usage";

interface UsageAutoRefreshOptions {
  refreshIntervalMs: number;
  sessionAutoSyncEnabled: boolean;
  paused?: boolean;
  /** 时间范围随「现在」移动（今天、最近 24 小时等） */
  rollingRange?: boolean;
  backgroundCollection?: boolean;
}

/** 随时间移动的范围多久重算一次：跨过午夜、滚动窗口移出旧记录都靠它 */
const ROLLING_RANGE_REFRESH_MS = 60_000;

/**
 * backgroundCollection 时由后端统一扫描，避免页面启动和后台重复采集。
 * 独立嵌入时保留前端定时扫描；手动同步仍可随时触发。
 * 原生页面通过 snapshot 的 usageVersion 刷新实际变化的数据。
 * 范围随时间移动时另有每分钟一次的重查，否则没有新用量时跨过午夜「今天」还停在昨天。
 */
export function useUsageAutoRefresh({
  refreshIntervalMs,
  sessionAutoSyncEnabled,
  paused = false,
  rollingRange = false,
  backgroundCollection = false,
}: UsageAutoRefreshOptions) {
  const queryClient = useQueryClient();
  const pendingSync = useRef<Promise<SessionSyncResult> | null>(null);

  // 手动同步复用尚未完成的静默扫描；慢扫描不会在后端互斥锁前排队累积。
  const syncSessionUsage = useCallback(() => {
    if (!pendingSync.current) {
      pendingSync.current = usageApi.syncSessionUsage().finally(() => {
        pendingSync.current = null;
      });
    }
    return pendingSync.current;
  }, []);

  useEffect(() => {
    if (
      backgroundCollection ||
      !sessionAutoSyncEnabled ||
      refreshIntervalMs <= 0 ||
      paused
    )
      return;
    let disposed = false;

    const scan = async () => {
      if (!focusManager.isFocused() || pendingSync.current) return;
      try {
        const result = await syncSessionUsage();
        if (result.errors.length > 0) {
          console.warn(
            "[UsageDashboard] Session auto-sync errors",
            result.errors,
          );
        }
      } catch (error) {
        // 静默扫描失败不弹提示，下一轮继续重试
        console.warn("[UsageDashboard] Session auto-sync failed", error);
      }
      if (!disposed) {
        await queryClient.invalidateQueries({
          queryKey: [...usageKeys.all, "session-last-sync"],
        });
      }
    };

    void scan();
    const timer = window.setInterval(() => void scan(), refreshIntervalMs);
    const unsubscribe = focusManager.subscribe((focused) => {
      if (focused) void scan();
    });
    return () => {
      disposed = true;
      window.clearInterval(timer);
      unsubscribe();
    };
  }, [
    paused,
    queryClient,
    refreshIntervalMs,
    sessionAutoSyncEnabled,
    syncSessionUsage,
    backgroundCollection,
  ]);

  useEffect(() => {
    if (!rollingRange) return;
    const timer = window.setInterval(() => {
      if (focusManager.isFocused()) {
        void queryClient.invalidateQueries({ queryKey: usageKeys.all });
      }
    }, ROLLING_RANGE_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [queryClient, rollingRange]);

  return { syncSessionUsage };
}
