import { CircleHelp } from "lucide-react";
import type { ReactNode } from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

/** Inline explanation that remains available to keyboard and screen-reader users. */
export function ProviderHelp({ label, children }: { label: string; children: ReactNode }) {
  return (
    <TooltipProvider delayDuration={0}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label={label}
            className="inline-flex h-5 w-5 shrink-0 items-center justify-center align-middle text-fg-3 hover:text-fg-1 focus-visible:rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <CircleHelp size={14} aria-hidden="true" />
          </button>
        </TooltipTrigger>
        <TooltipContent
          side="top"
          className="max-h-[60vh] max-w-[min(28rem,calc(100vw-1rem))] overflow-y-auto whitespace-pre-wrap rounded-control leading-relaxed"
        >
          {children}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
