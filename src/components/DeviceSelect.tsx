import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";
import type { AudioDeviceInfo } from "../lib/ipc";
import { formatSampleRate, getInputBadge, getOutputBadge } from "../lib/audioUtils";
import { usePopoverPlacement } from "./popoverPlacement";

interface DeviceSelectProps {
  mode: "input" | "output";
  label: string;
  devices: AudioDeviceInfo[];
  selectedDeviceId: string;
  isOpen: boolean;
  onToggleOpen: () => void;
  onClose: () => void;
  onSelect: (device: AudioDeviceInfo) => void;
}

function Badge({
  label,
  tone,
}: {
  label: string;
  tone: "signal" | "loopback" | "healthy" | "warning" | "neutral";
}) {
  const palette =
    tone === "signal"
      ? {
          bg: "var(--accent-signal-soft)",
          fg: "var(--accent-signal)",
          border: "var(--accent-signal-border)",
        }
      : tone === "loopback"
        ? {
            bg: "rgba(99, 102, 241, 0.14)",
            fg: "#818cf8",
            border: "rgba(129, 140, 248, 0.38)",
          }
        : tone === "healthy"
          ? {
              bg: "var(--status-healthy-soft)",
              fg: "var(--status-healthy)",
              border: "rgba(34, 197, 94, 0.36)",
            }
          : tone === "warning"
            ? {
                bg: "var(--status-warning-soft)",
                fg: "var(--status-warning)",
                border: "var(--status-warning-border)",
              }
            : {
                bg: "var(--bg-well)",
                fg: "var(--text-secondary)",
                border: "var(--border-subtle)",
              };

  return (
    <span
      className="font-mono-num shrink-0 inline-flex items-center px-1.5 py-[1px] rounded-[3px] text-[10px] font-semibold tracking-[0.04em] border"
      style={{
        backgroundColor: palette.bg,
        color: palette.fg,
        borderColor: palette.border,
      }}
    >
      {label}
    </span>
  );
}

export function DeviceSelect({
  mode,
  label,
  devices,
  selectedDeviceId,
  isOpen,
  onToggleOpen,
  onClose,
  onSelect,
}: DeviceSelectProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  const selected = devices.find((d) => d.id === selectedDeviceId);
  const badge = selected
    ? mode === "input"
      ? getInputBadge(selected)
      : getOutputBadge(selected)
    : null;

  const { style, ready } = usePopoverPlacement({
    open: isOpen,
    triggerRef,
    align: "left",
    minWidth: 280,
    maxWidth: 380,
    estimatedHeight: 300,
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

  const captureDevices =
    mode === "input" ? devices.filter((d) => d.flow === "capture") : [];
  const loopbackDevices =
    mode === "input" ? devices.filter((d) => d.flow === "loopback") : [];
  const renderDevices =
    mode === "output" ? devices.filter((d) => d.flow === "render") : [];

  const renderGroup = (groupTitle: string, items: AudioDeviceInfo[]) => {
    if (items.length === 0) return null;
    return (
      <div className="py-1">
        <div
          className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.08em]"
          style={{ color: "var(--text-muted)" }}
        >
          {groupTitle}
        </div>
        <div className="flex flex-col gap-0.5 px-1">
          {items.map((device) => {
            const isSelected = device.id === selectedDeviceId;
            const itemBadge =
              mode === "input" ? getInputBadge(device) : getOutputBadge(device);
            return (
              <button
                key={device.id}
                type="button"
                role="option"
                aria-selected={isSelected}
                onClick={() => {
                  onSelect(device);
                  onClose();
                }}
                className="tactile-btn hover-option w-full flex items-center justify-between gap-2 px-2 py-1.5 rounded-[5px] text-left"
                style={{
                  backgroundColor: isSelected ? "var(--bg-active-row)" : "transparent",
                }}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <Badge label={itemBadge.label} tone={itemBadge.tone} />
                    <span
                      className="text-[12px] font-medium truncate"
                      style={{ color: "var(--text-primary)" }}
                    >
                      {device.name}
                    </span>
                  </div>
                  <div
                    className="font-mono-num flex items-center gap-2 mt-0.5 text-[10px]"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    <span>{device.channels}ch</span>
                    <span>·</span>
                    <span>{formatSampleRate(device.sampleRate)}</span>
                    {device.supportsRawMode && (
                      <>
                        <span>·</span>
                        <span>RAW</span>
                      </>
                    )}
                    {device.isDefault && (
                      <>
                        <span>·</span>
                        <span style={{ color: "var(--accent-signal)" }}>DEFAULT</span>
                      </>
                    )}
                  </div>
                </div>
                <span className="shrink-0 w-4 flex justify-center">
                  {isSelected && (
                    <Check size={14} style={{ color: "var(--accent-signal)" }} />
                  )}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-1 min-w-0">
      <div className="flex items-center justify-between">
        <span
          className="text-[10px] font-semibold uppercase tracking-[0.08em]"
          style={{ color: "var(--text-secondary)" }}
        >
          {label}
        </span>
        {selected && (
          <span
            className="font-mono-num text-[10px]"
            style={{ color: "var(--text-muted)" }}
          >
            {selected.channels}ch · {formatSampleRate(selected.sampleRate)}
          </span>
        )}
      </div>

      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        onClick={onToggleOpen}
        className="tactile-btn hover-surface w-full flex items-center justify-between gap-2 px-2.5 py-2 rounded-[6px] border text-left min-w-0"
        style={{
          backgroundColor: "var(--bg-well)",
          borderColor:
            selected?.isBluetoothHfpWarning
              ? "var(--status-warning-border)"
              : isOpen
                ? "var(--accent-signal)"
                : "var(--border-subtle)",
        }}
      >
        <div className="flex items-center gap-2 min-w-0 flex-1">
          {badge ? (
            <Badge label={badge.label} tone={badge.tone} />
          ) : (
            <Badge label="OFFLINE" tone="warning" />
          )}
          <span
            className="text-[12px] font-medium truncate"
            style={{ color: selected ? "var(--text-primary)" : "var(--text-muted)" }}
          >
            {selected ? selected.name : `Disconnected (${selectedDeviceId})`}
          </span>
        </div>
        <ChevronDown
          size={14}
          className="chevron-rotator shrink-0"
          style={{
            color: "var(--text-secondary)",
            transform: isOpen ? "rotate(180deg)" : "rotate(0deg)",
          }}
        />
      </button>

      {isOpen &&
        ready &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            aria-label={label}
            className="popover-surface z-50 rounded-[8px] border overflow-y-auto"
            style={{
              ...style,
              backgroundColor: "var(--bg-popover)",
              borderColor: "var(--border-strong)",
            }}
          >
            {mode === "input" ? (
              captureDevices.length === 0 && loopbackDevices.length === 0 ? (
                <div
                  className="px-3 py-4 text-[12px]"
                  style={{ color: "var(--text-secondary)" }}
                >
                  No active capture or loopback endpoints detected.
                </div>
              ) : (
                <>
                  {renderGroup("Capture Inputs", captureDevices)}
                  {captureDevices.length > 0 && loopbackDevices.length > 0 && (
                    <div
                      className="mx-2 my-0.5 h-[1px]"
                      style={{ backgroundColor: "var(--border-subtle)" }}
                    />
                  )}
                  {renderGroup("WASAPI Loopback", loopbackDevices)}
                </>
              )
            ) : renderDevices.length === 0 ? (
              <div
                className="px-3 py-4 text-[12px]"
                style={{ color: "var(--text-secondary)" }}
              >
                No active output endpoints detected.
              </div>
            ) : (
              renderGroup("Output Targets", renderDevices)
            )}
          </div>,
          document.body,
        )}
    </div>
  );
}
