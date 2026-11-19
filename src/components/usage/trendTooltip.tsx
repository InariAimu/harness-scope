import { useLayoutEffect, useRef, type MutableRefObject, type ReactNode, type RefObject } from "react";
import type { TrendBarRect } from "./trendBarShape";

export interface TrendPointer { x: number; y: number }
export const TREND_TOOLTIP_GAP = 18;

/** Keep a pointer-following tooltip clear of the complete magnified block. */
export function placeTrendTooltip(
  pointer: TrendPointer, size: { width: number; height: number }, block: TrendBarRect,
  viewport: { width: number; height: number },
) {
  const gap = TREND_TOOLTIP_GAP;
  const padding = 8;
  const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(n, max));
  const right = Math.max(pointer.x + gap, block.x + block.width + gap);
  const left = Math.min(pointer.x - gap - size.width, block.x - gap - size.width);
  const sides = pointer.x >= block.x + block.width / 2 ? ["right", "left"] as const : ["left", "right"] as const;
  for (const side of sides) {
    const x = side === "right" ? right : left;
    if (x >= padding && x + size.width <= viewport.width - padding) {
      return { x, y: clamp(pointer.y - size.height / 2, padding, viewport.height - size.height - padding), side };
    }
  }
  const above = Math.min(pointer.y - gap - size.height, block.y - gap - size.height);
  const below = Math.max(pointer.y + gap, block.y + block.height + gap);
  const x = clamp(pointer.x - size.width / 2, padding, viewport.width - size.width - padding);
  if (above >= padding) return { x, y: above, side: "above" as const };
  if (below + size.height <= viewport.height - padding) return { x, y: below, side: "below" as const };
  return { x, y: clamp(below, padding, viewport.height - size.height - padding), side: "below" as const };
}

/** Pointer updates reposition just this frame, rather than rerendering every bar. */
export function TrendTooltipFrame({ plotRef, pointerRef, getBounds, children }: {
  plotRef: RefObject<HTMLDivElement>;
  pointerRef: MutableRefObject<TrendPointer | null>;
  getBounds: () => TrendBarRect | undefined;
  children: ReactNode;
}) {
  const frameRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const plot = plotRef.current;
    const frame = frameRef.current;
    if (!plot || !frame) return;
    let pending = 0;
    const position = () => {
      pending = 0;
      const bounds = getBounds();
      if (!bounds) return;
      const plotBox = plot.getBoundingClientRect();
      const frameBox = frame.getBoundingClientRect();
      const block = { ...bounds, x: plotBox.left + bounds.x, y: plotBox.top + bounds.y };
      const pointer = pointerRef.current ?? { x: block.x + block.width / 2, y: block.y + block.height / 2 };
      const target = placeTrendTooltip(pointer, frameBox, block, { width: window.innerWidth, height: window.innerHeight });
      frame.style.transform = `translate(${target.x - plotBox.left}px, ${target.y - plotBox.top}px)`;
      frame.style.visibility = "visible";
      frame.dataset.side = target.side;
    };
    const schedule = () => { if (!pending) pending = requestAnimationFrame(position); };
    const move = (event: PointerEvent) => { pointerRef.current = { x: event.clientX, y: event.clientY }; schedule(); };
    position();
    schedule();
    const observer = new ResizeObserver(schedule);
    observer.observe(frame);
    plot.addEventListener("pointermove", move);
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, true);
    return () => {
      cancelAnimationFrame(pending);
      observer.disconnect();
      plot.removeEventListener("pointermove", move);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
    };
  }, [plotRef, pointerRef, getBounds]);
  return <div ref={frameRef} data-testid="usage-trend-tooltip-frame"
    style={{ visibility: "hidden", maxWidth: "calc(100vw - 16px)" }}>{children}</div>;
}
