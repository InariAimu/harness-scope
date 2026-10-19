export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string | null;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath?: string;
  resumeCommand?: string;
  canDelete?: boolean;
}

// ─── 会话阅读页数据模型（对应 Rust session_manager/model.rs） ───

export type ToolKind =
  | "shell"
  | "read"
  | "search"
  | "edit"
  | "write"
  | "web"
  | "mcp"
  | "agent"
  | "ask"
  | "todo"
  | "other";

export type ToolStatus =
  | "success"
  | "error"
  | "interrupted"
  | "pending"
  | "unknown";

export type EventKind =
  | "aborted"
  | "compaction"
  | "model_change"
  | "thinking_level"
  | "hook"
  | "pr_link"
  | "slash_command"
  | "info"
  | "error"
  | "sub_agent"
  | "other";

/** 大内容的「按需取」引用：前端不解析，原样回传给后端 */
export type ContentRef =
  | { kind: "jsonl"; offset: number; len: number; pointer: string }
  | {
      kind: "sqlite";
      table: string;
      id: string;
      column: string;
      pointer: string;
    }
  | { kind: "file"; relPath: string; pointer: string }
  | { kind: "sidecar"; relPath: string };

export type ImageSource =
  | { kind: "inline"; content: ContentRef }
  | { kind: "local_file"; path: string };

export interface ImageRef {
  source: ImageSource;
  mediaType: string;
  /** 解码后的字节数估算 */
  size: number;
  alt?: string;
}

export interface DiffFile {
  path: string;
  op: "add" | "update" | "delete" | "rename";
  added: number;
  removed: number;
}

export interface DiffSummary {
  files: DiffFile[];
  added: number;
  removed: number;
  full?: ContentRef;
}

export interface TextBlock {
  type: "text";
  /** 正文；带 full 时只是预览（超长注入文本） */
  text: string;
  full?: ContentRef;
}

export interface ThinkingBlock {
  type: "thinking";
  /** 可见正文的预览；可能为空（加密/只有签名） */
  text: string;
  summary?: string;
  redacted?: boolean;
  durationMs?: number;
  full?: ContentRef;
}

export interface ToolCallBlock {
  type: "tool_call";
  /** 与 ToolResultBlock.callId 配对 */
  id: string;
  rawName: string;
  kind: ToolKind;
  title: string;
  detail?: string;
  server?: string;
  inputPreview: string;
  inputTotalLen: number;
  inputFull?: ContentRef;
  diff?: DiffSummary;
  byUser?: boolean;
}

export interface ToolResultBlock {
  type: "tool_result";
  /** 为空串表示源数据没有配对信息，显示为通用「工具输出」 */
  callId: string;
  status: ToolStatus;
  preview: string;
  totalLen: number;
  lineCount: number;
  truncated: boolean;
  full?: ContentRef;
  exitCode?: number;
  durationMs?: number;
  images?: ImageRef[];
  savedPath?: string;
}

export interface ImageBlock {
  type: "image";
  image: ImageRef;
}

export interface EventBlock {
  type: "event";
  kind: EventKind;
  /** 说明；带 full 时只是预览（压缩摘要等长文本） */
  text?: string;
  url?: string;
  full?: ContentRef;
}

export interface StepBlock {
  type: "step";
  phase: "start" | "finish";
  tokens?: number;
  costUsd?: number;
  reason?: string;
}

export type SessionBlock =
  | TextBlock
  | ThinkingBlock
  | ToolCallBlock
  | ToolResultBlock
  | ImageBlock
  | EventBlock
  | StepBlock;

export interface MessageMeta {
  model?: string;
  inputTokens?: number;
  outputTokens?: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  reasoningTokens?: number;
  costUsd?: number;
  durationMs?: number;
  stopReason?: string;
}

export interface SessionMessage {
  role: string;
  /**
   * 旧格式的纯文本内容：只有 blocks 缺失时（旧后端）才下发，
   * 有 blocks 时前端一律从 blocks 推导（见 reader/turns.ts `messageText`）
   */
  content?: string;
  ts?: number;
  id?: string;
  turnId?: string;
  injected?: boolean;
  /** 旧后端 / 尚未迁移的解析器会省略 → 前端按 content 兜底 */
  blocks?: SessionBlock[];
  meta?: MessageMeta;
}

export interface TurnIndex {
  turnId: string;
  firstMessageIndex: number;
  lastMessageIndex: number;
  questionPreview: string;
  ts?: number;
  stepCount: number;
  errorCount: number;
  hasFinalReply: boolean;
  aborted: boolean;
}

export type TranscriptChunk =
  | {
      type: "header";
      total: number;
      turns: TurnIndex[];
      cached: boolean;
      parseMs: number;
    }
  | { type: "messages"; start: number; messages: SessionMessage[] }
  | { type: "done"; payloadBytes: number }
  | { type: "error"; message: string };

/** Device-local usage presentation; persisted independently from USD facts and sync. */
export interface UsageDisplaySettings {
  currency?: string;
  exchangeRates?: Record<string, number>;
  sidebarTodayItems?: string[];
  requestLogColumns?: Record<string, boolean>;
  trendChartType?: string;
}
