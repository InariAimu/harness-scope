import { useQuery } from "@tanstack/react-query";
import { command } from "@/lib/client";
import type { Snapshot } from "@/App";

/** Share the application's snapshot so filters, labels and the sidebar use one zone. */
export function useUsageTimezone() {
  const { data } = useQuery({
    queryKey: ["snapshot"],
    queryFn: () => command<Snapshot>("snapshot"),
    staleTime: 10000,
  });
  return (
    data?.config.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone
  );
}
