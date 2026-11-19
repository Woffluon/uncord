import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { createPortal } from "react-dom";
import { Activity, Info } from "lucide-react";
import type { RouteTelemetry } from "../lib/ipc";
import { formatMs } from "../lib/audioUtils";
import { usePopoverPlacement } from "./popoverPlacement";

interface LatencyPopoverProps {
  routeId: string;
  enabled: boolean;
  summaryTelemetry?: RouteTelemetry;
  telemetryMapRef: MutableRefObject<Map<string, RouteTelemetry>>;
  isOpen: boolean;
  onToggleOpen: () => void;
  onClose: () => void;
}

export function LatencyPopover({
  routeId,
  enabled,
  summaryTelemetry,
  telemetryMapRef,
  isOpen,
  onToggleOpen,
  onClose,
}: LatencyPopoverProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [liveTelemetry, setLiveTelemetry] = useState<RouteTelemetry | undefined>(summaryTelemetry);

  useEffect(() => {
    if (!isOpen) {
      setLiveTelemetry(summaryTelemetry);
      return;
    }
    setLiveTelemetry(telemetryMapRef.current.get(routeId) ?? summaryTelemetry);
    const id = window.setInterval(() => {
      setLiveTelemetry(telemetryMapRef.current.get(routeId));
    }, 180);
    return () => window.clearInterval(id);
  }, [isOpen, routeId, summaryTelemetry, telemetryMapRef]);

  const { style, ready } = usePopoverPlacement({
    open: isOpen,
    triggerRef,
    align: "right",
    minWidth: 296,
    maxWidth: 332,
    estimatedHeight: 280,
  });

  useEffect(() => {
    if (!isOpen) return;
    const handlePointerDown = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (!target) return;
      if (triggerRef.current?.contains(target)) return;
      if (popoverRef.current?.contains(target)) return;
      onClose();
    };
    window.addEventListener("pointerdown", handlePointerDown);
    return () => window.removeEventListener("pointerdown", handlePointerDown);
  }, [isOpen, onClose]);

  const t = liveTelemetry ?? summaryTelemetry;
  const swMs = t?.latency.softwareTotalMs;
  const hwFloorMs = t?.latency.estimatedTransportFloorMs;
  const healthySw = enabled && swMs !== undefined && swMs <= 12.0;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        onClick={onToggleOpen}
        title="Inspect Software Pipeline vs Hardware Transport Latency"
        className="tactile-btn hover-surface inline-flex items-center gap-1.5 px-2 py-1 rounded-[5px] border font-mono-num text-[11px]"
        style={{
          backgroundColor: "var(--bg-well)",
          borderColor: isOpen ? "var(--accent-signal)" : "var(--border-subtle)",
          color: "var(--text-primary)",
        }}
      >
        <Activity
          size={12}
          style={{
            color: !enabled
              ? "var(--text-muted)"
              : healthySw
                ? "var(--status-healthy)"
                : "var(--status-warning)",
          }}
        />
        <span>SW {enabled ? formatMs(swMs) : "-- ms"}</span>
        {hwFloorMs !== undefined && enabled && (
          <span style={{ color: "var(--text-muted)" }}>
            + ~{Math.round(hwFloorMs)}ms HW
          </span>
        )}
        <Info size={11} style={{ color: "var(--text-secondary)" }} />
      </button>

      {isOpen &&
        ready &&
        createPortal(
          <div
            ref={popoverRef}
            role="dialog"
            aria-label="Latency breakdown diagnostics"
            className="popover-surface z-50 rounded-[8px] border p-3"
            style={{
              ...style,
              backgroundColor: "var(--bg-popover)",
              borderColor: "var(--border-strong)",
            }}
          >
            <div className="flex items-center justify-between mb-2.5">
              <span
                className="text-[11px] font-semibold uppercase tracking-[0.08em]"
                style={{ color: "var(--text-primary)" }}
              >
                Latency
              </span>
              <span
                className="font-mono-num text-[10px] px-1.5 py-0.5 rounded-[3px] font-semibold"
                style={{
                  backgroundColor: t?.latency.rawModeActive
                    ? "var(--status-healthy-soft)"
                    : "var(--bg-well)",
                  color: t?.latency.rawModeActive
                    ? "var(--status-healthy)"
                    : "var(--text-secondary)",
                }}
              >
                RAW APO: {t?.latency.rawModeActive ? "ON" : "OFF"}
              </span>
            </div>

            <div className="flex flex-col gap-1.5 font-mono-num text-[11px]">
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>Capture Period</span>
                <span>{t ? `${t.latency.capturePeriodMs.toFixed(2)} ms` : "--"}</span>
              </div>
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>Ring Buffer</span>
                <span>{t ? `${t.latency.ringBufferMs.toFixed(2)} ms` : "--"}</span>
              </div>
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>Render Period</span>
                <span>{t ? `${t.latency.renderPeriodMs.toFixed(2)} ms` : "--"}</span>
              </div>

              <div
                className="my-1 h-[1px]"
                style={{ backgroundColor: "var(--border-subtle)" }}
              />

              <div className="flex items-center justify-between font-semibold">
                <span style={{ color: "var(--text-primary)" }}>SW Pipeline Total</span>
                <span
                  style={{
                    color:
                      t && t.latency.softwareTotalMs <= 12.0
                        ? "var(--status-healthy)"
                        : "var(--status-warning)",
                  }}
                >
                  {t ? `${t.latency.softwareTotalMs.toFixed(2)} ms` : "--"}
                </span>
              </div>

              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>HW Transport Floor</span>
                <span style={{ color: "var(--text-secondary)" }}>
                  {t ? `~${Math.round(t.latency.estimatedTransportFloorMs)} ms` : "--"}
                </span>
              </div>
            </div>

            <p
              className="mt-2 text-[10px] leading-[1.4]"
              style={{ color: "var(--text-muted)" }}
            >
              Transport floor is downstream of WASAPI.
            </p>

            <div
              className="mt-2.5 pt-2 border-t grid grid-cols-3 gap-2 font-mono-num text-[10px]"
              style={{ borderColor: "var(--border-subtle)" }}
            >
              <div>
                <div style={{ color: "var(--text-muted)" }}>DRIFT</div>
                <div
                  style={{
                    color:
                      t && Math.abs(t.driftPpm) > 500
                        ? "var(--status-warning)"
                        : "var(--text-primary)",
                  }}
                >
                  {t ? `${t.driftPpm >= 0 ? "+" : ""}${t.driftPpm.toFixed(1)} ppm` : "--"}
                </div>
              </div>
              <div>
                <div style={{ color: "var(--text-muted)" }}>XRUNS</div>
                <div
                  style={{
                    color:
                      t && t.xruns > 0 ? "var(--status-warning)" : "var(--text-primary)",
                  }}
                >
                  {t ? t.xruns : 0}
                </div>
              </div>
              <div>
                <div style={{ color: "var(--text-muted)" }}>RING FILL</div>
                <div style={{ color: "var(--text-primary)" }}>
                  {t ? `${Math.round(t.bufferFillRatio * 100)}%` : "--"}
                </div>
              </div>
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
