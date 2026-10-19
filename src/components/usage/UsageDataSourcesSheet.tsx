import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetBody,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  sessionAutoSyncEnabled: boolean;
  onSessionAutoSyncEnabledChange?: (v: boolean) => void;
  syncedLabel?: string;
  syncing: boolean;
  onSyncNow: () => void;
  onOpenRoutingSettings?: () => void;
  rebuildingCodex: boolean;
  onRebuildCodex: () => void;
}
export function UsageDataSourcesSheet(p: Props) {
  const { t } = useTranslation();
  return (
    <Sheet open={p.open} onOpenChange={p.onOpenChange}>
      <SheetContent closeLabel={t("common.close", "关闭")}>
        <SheetHeader>
          <SheetTitle>{t("usage.dataSource", "数据来源")}</SheetTitle>
        </SheetHeader>
        <SheetBody>
          <div className="rounded-panel border border-border p-4 space-y-3">
            <h3>{t("scope.nativeSources", "本机会话日志")}</h3>
            <p className="text-body text-fg-2">
              {t(
                "scope.sourceNote",
                "读取已配置的本机会话日志或数据库。会话正文留在本机，用量与费用来源可同步到自己的服务器。",
              )}
            </p>
            <p className="text-caption text-fg-3">{p.syncedLabel}</p>
            <Button onClick={p.onSyncNow} disabled={p.syncing}>
              {t("scope.scan", "立即采集")}
            </Button>
          </div>
          <p className="mt-4 text-caption text-fg-2">
            {t(
              "scope.sourceSettings",
              "目录和采集开关在「应用」页配置。历史日汇总没有请求明细；部分覆盖一个汇总日的时间范围不包含该日汇总。",
            )}
          </p>
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
