import type { RequestLog } from "@/types/usage";

export function codexRequestMode(
  request: Pick<RequestLog, "appType" | "requestMetadata">,
) {
  if (request.appType !== "codex") return null;
  switch (request.requestMetadata?.serviceTier) {
    case "default":
      return "Standard";
    case "priority":
    case "fast":
      return "Fast";
    default:
      return null;
  }
}
export function codexRequestHint(
  request: Pick<RequestLog, "appType" | "requestMetadata">,
  zh: boolean,
) {
  if (request.appType !== "codex") return "";
  const mode =
    codexRequestMode(request) ??
    request.requestMetadata?.serviceTier ??
    (zh ? "未知" : "Unknown");
  const effort =
    request.requestMetadata?.reasoningEffort ?? (zh ? "未知" : "Unknown");
  return zh
    ? `请求档位：${mode}（日志设置，非服务端确认）；推理强度：${effort}`
    : `Requested tier: ${mode} (log setting, not server-confirmed); reasoning effort: ${effort}`;
}
