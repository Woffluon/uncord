import { useEffect, useState, type CSSProperties, type RefObject } from "react";

export interface PopoverLayout {
  style: CSSProperties;
  ready: boolean;
}

interface PlacementOptions {
  open: boolean;
  triggerRef: RefObject<HTMLElement | null>;
  align?: "left" | "right";
  minWidth?: number;
  maxWidth?: number;
  estimatedHeight?: number;
  matchTriggerWidth?: boolean;
}

export function usePopoverPlacement({
  open,
  triggerRef,
  align = "left",
  minWidth = 240,
  maxWidth = 380,
  estimatedHeight = 280,
  matchTriggerWidth = false,
}: PlacementOptions): PopoverLayout {
  const [style, setStyle] = useState<CSSProperties>({});
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!open) {
      setReady(false);
      return;
    }

    const update = () => {
      const el = triggerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      const gap = 6;
      const margin = 12;

      const desiredWidth = matchTriggerWidth
        ? Math.max(minWidth, Math.min(maxWidth, rect.width))
        : Math.min(maxWidth, Math.max(minWidth, rect.width));

      let left = align === "right" ? rect.right - desiredWidth : rect.left;
      left = Math.max(margin, Math.min(vw - desiredWidth - margin, left));

      const spaceBelow = vh - rect.bottom - margin;
      const spaceAbove = rect.top - margin;
      const placeAbove = spaceBelow < Math.min(estimatedHeight, 220) && spaceAbove > spaceBelow;

      const triggerCenterX = rect.left + rect.width / 2;
      const originX = Math.max(12, Math.min(desiredWidth - 12, triggerCenterX - left));

      if (placeAbove) {
        const bottom = vh - rect.top + gap;
        setStyle({
          position: "fixed",
          bottom: `${bottom}px`,
          left: `${left}px`,
          width: `${desiredWidth}px`,
          maxHeight: `${Math.max(160, spaceAbove - gap)}px`,
          ["--transform-origin" as string]: `${Math.round(originX)}px 100%`,
        });
      } else {
        const top = rect.bottom + gap;
        setStyle({
          position: "fixed",
          top: `${top}px`,
          left: `${left}px`,
          width: `${desiredWidth}px`,
          maxHeight: `${Math.max(160, spaceBelow - gap)}px`,
          ["--transform-origin" as string]: `${Math.round(originX)}px 0px`,
        });
      }
      setReady(true);
    };

    update();
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [open, triggerRef, align, minWidth, maxWidth, estimatedHeight, matchTriggerWidth]);

  return { style, ready };
}
