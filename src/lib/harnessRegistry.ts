import { useQuery } from "@tanstack/react-query";
import { command } from "./client";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { Snapshot } from "@/App";
export function useHarnessDescriptors() {
  const q = useQuery({
    queryKey: ["snapshot"],
    queryFn: () => command<Snapshot>("snapshot"),
    staleTime: 10000,
  });
  const descriptors = q.data?.harnesses ?? [];
  for (const d of descriptors) APP_DISPLAY_NAME[d.id] = d.name;
  return descriptors;
}
