import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, Split } from "lucide-react";
import type { ChannelMode } from "../lib/ipc";
import {
  channelModeEquals,
  describeChannelMode,
  getChannelModeOptions,
} from "../lib/audioUtils";
import { usePopoverPlacement } from "./popoverPlacement";

interface ChannelMapSelectProps {
  channels: number;
  value: ChannelMode;
  isOpen: boolean;
  onToggleOpen: () => void;
  onClose: () => void;
  onChange: (mode: ChannelMode) => void;
}

export function ChannelMapSelect({
  channels,
  value,
  isOpen,
  onToggleOpen,
  onClose,
  onChange,
}: ChannelMapSelectProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  const options = getChannelModeOptions(channels);
  const currentLabel = describeChannelMode(value, channels);
  const isMultiChannel = channels >= 4;

  const { style, ready } = usePopoverPlacement({
    open: isOpen,
    triggerRef,
    align: "left",
    minWidth: 230,
    maxWidth: 320,
    estimatedHeight: 240,
    matchTriggerWidth: true,
  });

  useEffect(() => {
    if (!isOpen) return;
    const handlePointerDown = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (!target) return;
      if (triggerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      onClose();
    };
    window.addEventListener("pointerdown", handlePointerDown);
    return () => window.removeEventListener("pointerdown", handlePointerDown);
  }, [isOpen, onClose]);

  return (
    <div className="flex flex-col gap-1 min-w-0">
      <div className="flex items-center justify-between">
        <span
          className="text-[10px] font-semibold uppercase tracking-[0.08em]"
          style={{ color: "var(--text-secondary)" }}
        >
          Channel Map
        </span>
        {isMultiChannel && (
          <span
            className="font-mono-num text-[10px] font-semibold"
            style={{ color: "var(--accent-signal)" }}
          >
            {channels}-CH MATRIX
          </span>
        )}
      </div>

      <button
        ref={triggerRef}
        type="button"
        disabled={options.length <= 1}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        onClick={onToggleOpen}
        className="tactile-btn hover-surface w-full flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] border text-left"
        style={{
          backgroundColor: "var(--bg-well)",
          borderColor: isOpen
            ? "var(--accent-signal)"
            : isMultiChannel
              ? "var(--border-strong)"
              : "var(--border-subtle)",
        }}
      >
        <div className="flex items-center gap-1.5 min-w-0">
          <Split
            size={12}
            className="shrink-0"
            style={{ color: isMultiChannel ? "var(--accent-signal)" : "var(--text-secondary)" }}
          />
          <span
            className="font-mono-num text-[11px] font-medium truncate"
            style={{ color: "var(--text-primary)" }}
          >
            {currentLabel}
          </span>
        </div>
        {options.length > 1 && (
          <ChevronDown
            size={13}
            className="chevron-rotator shrink-0"
            style={{
              color: "var(--text-secondary)",
              transform: isOpen ? "rotate(180deg)" : "rotate(0deg)",
            }}
          />
        )}
      </button>

      {isOpen &&
        ready &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            aria-label="Channel pair mapping"
            className="popover-surface z-50 rounded-[8px] border p-1 overflow-y-auto"
            style={{
              ...style,
              backgroundColor: "var(--bg-popover)",
              borderColor: "var(--border-strong)",
            }}
          >
            {options.map((opt) => {
              const active = channelModeEquals(opt.mode, value);
              return (
                <button
                  key={opt.id}
                  type="button"
                  role="option"
                  aria-selected={active}
                  onClick={() => {
                    onChange(opt.mode);
                    onClose();
                  }}
                  className="tactile-btn hover-option w-full flex items-center justify-between gap-2 px-2 py-1.5 rounded-[5px] text-left"
                  style={{
                    backgroundColor: active ? "var(--bg-active-row)" : "transparent",
                  }}
                >
                  <span
                    className="font-mono-num text-[11px] font-medium"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {opt.label}
                  </span>
                  {active && <Check size={13} style={{ color: "var(--accent-signal)" }} />}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </div>
  );
}
