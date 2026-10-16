import { command, saveText } from "@/lib/client";
import type {
  SessionMeta,
  SessionMessage,
  ContentRef,
  ImageRef,
} from "@/types";
export const sessionsApi = {
  list: () => command<SessionMeta[]>("sessions"),
  getMessages: (providerId: string, sourcePath: string, sessionId?: string) =>
    command<SessionMessage[]>("session_messages", {
      providerId,
      sourcePath,
      sessionId,
    }),
  delete: (args: {
    providerId: string;
    sourcePath: string;
    sessionId: string;
  }) => command<boolean>("delete_session", args),
  exportMarkdown: (defaultName: string, content: string) =>
    saveText(defaultName, content),
  getBlockContent: (
    _providerId: string,
    _sourcePath: string,
    _reference: ContentRef,
  ) => Promise.reject(new Error("This reader supplies content inline")),
  getImage: (providerId: string, sourcePath: string, image: ImageRef) =>
    command<string>("session_image", { providerId, sourcePath, image }),
  revealPath: (path: string) =>
    command<boolean>("reveal_session_path", { path }),
};
