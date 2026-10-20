import { OptionSelect } from "@/components/ui/option-select";
import { useState } from "react";
import { useHarnessDescriptors } from "@/lib/harnessRegistry";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw, Folder } from "lucide-react";
import { sessionsApi } from "@/lib/api";
import type { SessionMeta } from "@/types";
import { useSessionTranscript } from "@/lib/query/sessions";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { SessionReader } from "@/components/sessions/reader/SessionReader";
import { SessionItem } from "@/components/sessions/SessionItem";
import { Button } from "@/components/ui/button";
import { SearchField } from "@/components/ui/search-field";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { toast } from "@/lib/toast";
import { getSessionKey } from "@/components/sessions/utils";
export function SessionsPage({ onSettings }: { onSettings: () => void }) {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) =>
    i18n.language.startsWith("zh") ? zh : en;
  const client = useQueryClient();
  const list = useQuery({
    queryKey: ["sessions"],
    queryFn: sessionsApi.list,
    refetchInterval: 60000,
    staleTime: 30000,
  });
  const [selected, setSelected] = useState<SessionMeta | null>(null);
  const [search, setSearch] = useState("");
  const [app, setApp] = useState("all");
  const [group, setGroup] = useState("project");
  const [deleting, setDeleting] = useState<SessionMeta | null>(null);
  const [busy, setBusy] = useState(false);
  const [selection, setSelection] = useState(false);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const harnesses = useHarnessDescriptors();
  const transcript = useSessionTranscript(
    selected?.providerId,
    selected?.sourcePath,
    selected?.sessionId,
  );
  const copy = (s: string) =>
    void navigator.clipboard
      .writeText(s)
      .then(() => toast.success(l("已复制", "Copied")))
      .catch((e) => toast.error(String(e)));
  const rows = (list.data ?? []).filter(
    (s) =>
      (app === "all" || s.providerId === app) &&
      `${s.title} ${s.summary} ${s.projectDir} ${s.sessionId}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  const index = rows.findIndex(
    (s) =>
      s.sourcePath === selected?.sourcePath &&
      s.sessionId === selected?.sessionId &&
      s.providerId === selected?.providerId,
  );
  const remove = async () => {
    if (!deleting?.sourcePath) return;
    setBusy(true);
    try {
      await sessionsApi.delete({
        providerId: deleting.providerId,
        sessionId: deleting.sessionId,
        sourcePath: deleting.sourcePath,
      });
      if (selected && getSessionKey(selected) === getSessionKey(deleting)) setSelected(null);
      setDeleting(null);
      await client.invalidateQueries();
      toast.success(
        l(
          "会话已删除，已采集用量继续保留",
          "Session deleted; collected usage retained",
        ),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };
  const groups = new Map<string, SessionMeta[]>();
  for (const s of rows) {
    const key =
      group === "project"
        ? s.projectDir || l("其他会话", "Other sessions")
        : new Date(s.lastActiveAt || s.createdAt || 0).toLocaleDateString();
    const entries = groups.get(key);
    if (entries) entries.push(s);
    else groups.set(key, [s]);
  }
  return (
    <>
      {selected ? (
        <SessionReader
          session={selected}
          appName={harnesses.find((h) => h.id === selected.providerId)?.name ?? selected.providerId}
          transcript={transcript}
          listQuery={search}
          launchTerminal={null}
          hasPrev={index > 0}
          hasNext={index >= 0 && index < rows.length - 1}
          onPrev={() => setSelected(rows[index - 1])}
          onNext={() => setSelected(rows[index + 1])}
          onBack={() => setSelected(null)}
          onLaunch={() => copy(selected.resumeCommand || "")}
          onCopy={(text) => copy(text)}
          onOpenTerminalSettings={onSettings}
          onReload={() => void transcript.refetch()}
          onDelete={() => setDeleting(selected)}
        />
      ) : (
        <>
          <AppPageHeader
            title={l("会话", "Sessions")}
            titleExtra={
              <span className="text-caption text-fg-3">{rows.length}</span>
            }
            actions={
              <Button
                variant="neutral"
                size="compact"
                onClick={() => void list.refetch()}
              >
                <RefreshCw size={14} />
                {l("刷新", "Refresh")}
              </Button>
            }
          />
          <div className="scope-page">
            <div className="flex items-center gap-3 mb-6">
              <SearchField
                className="max-w-sm flex-1"
                placeholder={l(
                  "搜索会话、项目、ID…",
                  "Search sessions, projects, IDs…",
                )}
                value={search}
                onValueChange={setSearch}
                clearLabel={l("清除搜索", "Clear search")}
              />
              <OptionSelect
                aria-label="Session harness"
                className="scope-select"
                value={app}
                onValueChange={(value) => setApp(value)}
              >
                <option value="all">{l("全部应用", "All applications")}</option>
                {harnesses.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.name}
                  </option>
                ))}
              </OptionSelect>
              <OptionSelect
                aria-label="Session grouping"
                className="scope-select"
                value={group}
                onValueChange={(value) => setGroup(value)}
              >
                <option value="project">{l("按项目分组", "By project")}</option>
                <option value="time">{l("按时间分组", "By date")}</option>
              </OptionSelect>
            </div>
            {list.isLoading && <p>{l("读取会话中…", "Loading sessions…")}</p>}
            {list.isError && (
              <p className="text-danger-text">{String(list.error)}</p>
            )}
            {!list.isLoading && rows.length === 0 && (
              <div className="scope-card text-center text-fg-2 py-12">
                {l(
                  "没有找到会话。可在「应用」页配置日志目录。",
                  "No sessions found. Configure log directories in Applications.",
                )}
              </div>
            )}
            {Array.from(groups, ([key, sessions]) => (
              <section key={key} className="mb-6">
                <h2 className="flex items-center gap-2 text-caption text-fg-2 mb-2">
                  <Folder size={13} />
                  {key}
                </h2>
                <div className="rounded-panel border border-border overflow-hidden">
                  {sessions.map((s, i) => (
                    <SessionItem
                      key={`${s.providerId}:${s.sourcePath}:${s.sessionId}`}
                      session={s}
                      showAppIcon
                      showDir={group !== "project"}
                      selectionMode={selection}
                      isChecked={checked.has(getSessionKey(s))}
                      searchQuery={search}
                      launchTerminal={null}
                      bordered={i > 0}
                      openButtonId={`open-${s.sessionId}`}
                      onOpen={() => setSelected(s)}
                      onToggleChecked={(value) =>
                        setChecked((old) => {
                          const next = new Set(old);
                          value
                            ? next.add(getSessionKey(s))
                            : next.delete(getSessionKey(s));
                          return next;
                        })
                      }
                      onStartSelect={() => setSelection(true)}
                      onLaunch={() => copy(s.resumeCommand || "")}
                      onCopyResume={() => copy(s.resumeCommand || "")}
                      onCopyId={() => copy(s.sessionId)}
                      onCopySource={() => copy(s.sourcePath || "")}
                      onDelete={() => setDeleting(s)}
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        </>
      )}
      <ConfirmDialog
        isOpen={!!deleting}
        title={l("删除本机会话", "Delete local session")}
        message={l(
          "删除原始会话文件；删除前会采集并保存用量，历史统计继续保留。最近仍在写入的会话不能删除。",
          "Delete the original session file after collecting its usage. Historical usage is retained. Recently active sessions cannot be deleted.",
        )}
        pending={busy}
        onCancel={() => setDeleting(null)}
        onConfirm={() => void remove()}
      />
    </>
  );
}
