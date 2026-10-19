import { command, saveText } from "@/lib/client";
import type {
  SessionMeta,
  SessionMessage,
  ContentRef,
  ImageRef,
  TranscriptChunk,
} from "@/types";
export interface TranscriptPage {
  header: Extract<TranscriptChunk, { type: "header" }>;
  messages: SessionMessage[];
  start: number;
  nextOffset: number | null;
  fingerprint: string;
  payloadBytes: number;
}
export interface BlockContentPage {
  text: string;
  totalLen: number;
  truncated: boolean;
  nextOffset: number | null;
}
export const sessionsApi = {
  list: () => command<SessionMeta[]>("sessions"),
  getMessages: (providerId: string, sourcePath: string, sessionId?: string) =>
    command<SessionMessage[]>("session_messages", {
      providerId,
      sourcePath,
      sessionId,
    }),
  getTranscript: (providerId: string, sourcePath: string, sessionId?: string, offset = 0, fingerprint?: string) =>
    command<TranscriptPage>("session_transcript", { providerId, sourcePath, sessionId, offset, fingerprint }),
  delete: (args: {
    providerId: string;
    sourcePath: string;
    sessionId: string;
  }) => command<boolean>("delete_session", args),
  exportMarkdown: (defaultName: string, content: string) =>
    saveText(defaultName, content),
  getBlockContent: (
    providerId: string,
    sourcePath: string,
    reference: ContentRef,
    offset = 0,
  ) => command<BlockContentPage>("session_block", { providerId, sourcePath, reference, offset }),
  getImage: (providerId: string, sourcePath: string, image: ImageRef) =>
    command<string>("session_image", { providerId, sourcePath, image }),
  revealPath: (path: string) =>
    command<boolean>("reveal_session_path", { path }),
};
