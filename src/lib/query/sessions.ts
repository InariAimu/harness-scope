import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
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
export interface TranscriptState {
  header: SessionTranscriptResult["header"];
  messages: SessionMessage[];
  done: boolean;
}
export async function loadTranscript(
  providerId: string, sourcePath: string, sessionId: string | undefined,
  onProgress: (state: TranscriptState) => void, signal?: AbortSignal,
): Promise<TranscriptState> {
  let state: TranscriptState = { header: null, messages: [], done: false };
  let offset = 0;
  let fingerprint: string | undefined;
  do {
    signal?.throwIfAborted();
    const page = await sessionsApi.getTranscript(providerId, sourcePath, sessionId, offset, fingerprint);
    signal?.throwIfAborted();
    if (page.start !== offset || (fingerprint && page.fingerprint !== fingerprint)) throw new Error("Session changed; reload transcript");
    fingerprint = page.fingerprint;
    state = { header: state.header ?? page.header, messages: [...state.messages, ...page.messages], done: page.nextOffset === null };
    onProgress(state);
    if (page.nextOffset === null) break;
    if (page.nextOffset <= offset) throw new Error("Invalid transcript continuation");
    offset = page.nextOffset;
  } while (true);
  return state;
}
export function useSessionTranscript(
  providerId?: string, sourcePath?: string, sessionId?: string,
): SessionTranscriptResult {
  const client = useQueryClient();
  const queryKey = ["session", providerId, sourcePath, sessionId];
  const q = useQuery({
    queryKey,
    queryFn: ({ signal }) => {
      const previous = client.getQueryData<TranscriptState>(queryKey);
      return loadTranscript(providerId!, sourcePath!, sessionId, (state) => {
        if (!previous?.done) client.setQueryData(queryKey, state);
      }, signal);
    },
    enabled: !!providerId && !!sourcePath,
    staleTime: 30000,
  });
  return {
    ...q,
    header: q.data?.header ?? null,
    messages: q.data?.messages ?? [],
    isStreaming: !!q.data && !q.data.done && !q.isError,
    progress: { loaded: q.data?.messages.length ?? 0, total: q.data?.header?.total ?? null },
  };
}
export function useBlockContent(
  providerId?: string, sourcePath?: string, reference?: ContentRef,
  options: { enabled?: boolean } = {},
) {
  const q = useInfiniteQuery({
    queryKey: ["block", providerId, sourcePath, reference],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => sessionsApi.getBlockContent(providerId!, sourcePath!, reference!, pageParam),
    getNextPageParam: (page) => page.truncated ? page.nextOffset ?? undefined : undefined,
    enabled: !!reference && !!providerId && !!sourcePath && options.enabled !== false,
    staleTime: 30000,
  });
  return {
    ...q,
    text: q.data ? q.data.pages.map((page) => page.text).join("") : null,
    hasMore: q.hasNextPage,
    tooLarge: false,
    isFetchingMore: q.isFetchingNextPage,
    loadMore: () => q.fetchNextPage(),
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
