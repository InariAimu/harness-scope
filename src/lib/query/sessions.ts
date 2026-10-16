import { useQuery } from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import type {
  SessionMessage,
  ContentRef,
  ImageRef,
  TranscriptChunk,
} from "@/types";
export interface SessionTranscriptResult {
  header: Extract<TranscriptChunk, { type: "header" }> | null;
  messages: SessionMessage[];
  isStreaming: boolean;
  progress: { loaded: number; total: number | null };
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  refetch: () => Promise<unknown>;
}
export function useSessionTranscript(
  providerId?: string,
  sourcePath?: string,
  sessionId?: string,
): SessionTranscriptResult {
  const q = useQuery({
    queryKey: ["session", providerId, sourcePath, sessionId],
    queryFn: () => sessionsApi.getMessages(providerId!, sourcePath!, sessionId),
    enabled: !!providerId && !!sourcePath,
  });
  return {
    ...q,
    header: null,
    messages: q.data ?? [],
    isStreaming: false,
    progress: { loaded: q.data?.length ?? 0, total: q.data?.length ?? null },
  };
}
export function useBlockContent(
  providerId?: string,
  sourcePath?: string,
  reference?: ContentRef,
  options: { enabled?: boolean } = {},
) {
  const q = useQuery({
    queryKey: ["block", providerId, sourcePath, reference],
    queryFn: () =>
      sessionsApi.getBlockContent(providerId!, sourcePath!, reference!),
    enabled:
      !!reference && !!providerId && !!sourcePath && options.enabled !== false,
  });
  return {
    ...q,
    text: null as string | null,
    hasMore: false,
    tooLarge: false,
    isFetchingMore: false,
    loadMore: async () => {},
  };
}
export function useSessionImage(
  providerId: string | undefined,
  sourcePath: string | undefined,
  image: ImageRef,
  options: { enabled?: boolean } = {},
) {
  const q = useQuery({
    queryKey: ["image", providerId, sourcePath, image],
    queryFn: () => sessionsApi.getImage(providerId!, sourcePath!, image),
    enabled: !!providerId && !!sourcePath && options.enabled !== false,
  });
  return { ...q, url: q.data ?? null };
}
