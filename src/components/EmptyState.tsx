import { ArrowRight, Zap } from "lucide-react";
import type { AudioDeviceInfo } from "../lib/ipc";
import {
  formatSampleRate,
  getInputBadge,
  getOutputBadge,
  pickSmartRouteDefaults,
} from "../lib/audioUtils";

interface EmptyStateProps {
  devices: AudioDeviceInfo[];
  onQuickStart: () => void;
}

export function EmptyState({ devices, onQuickStart }: EmptyStateProps) {
  const suggestion = pickSmartRouteDefaults(devices, []);
  const srcBadge = suggestion.source ? getInputBadge(suggestion.source) : null;
  const dstBadge = suggestion.target ? getOutputBadge(suggestion.target) : null;
  const canQuickStart = Boolean(suggestion.source && suggestion.target);

  return (
    <section
      className="rounded-[12px] border p-6 sm:p-8 flex flex-col items-start gap-5"
      style={{
        backgroundColor: "var(--bg-panel)",
        borderColor: "var(--border-subtle)",
      }}
    >
      <div className="flex flex-col gap-1.5 max-w-[520px]">
        <h2
          className="text-[18px] font-semibold tracking-[-0.02em]"
          style={{ color: "var(--text-primary)" }}
        >
          No routes
        </h2>
        <p className="text-[13px] leading-[1.55]" style={{ color: "var(--text-secondary)" }}>
          Route any capture or loopback input to one or more outputs.
        </p>
      </div>

      {canQuickStart && suggestion.source && suggestion.target ? (
        <div
          className="w-full rounded-[8px] border p-3.5 flex flex-col sm:flex-row sm:items-center justify-between gap-3"
          style={{
            backgroundColor: "var(--bg-well)",
            borderColor: "var(--border-subtle)",
          }}
        >
          <div className="flex flex-wrap items-center gap-2 text-[12px] min-w-0">
            <span
              className="font-mono-num text-[10px] font-semibold px-1.5 py-0.5 rounded-[3px] border"
              style={{
                backgroundColor: "var(--accent-signal-soft)",
                borderColor: "var(--accent-signal-border)",
                color: "var(--accent-signal)",
              }}
            >
              {srcBadge?.label ?? "INPUT"}
            </span>
            <span className="font-medium truncate" style={{ color: "var(--text-primary)" }}>
              {suggestion.source.name}
            </span>
            <span className="font-mono-num text-[11px]" style={{ color: "var(--text-muted)" }}>
              ({suggestion.source.channels}ch · {formatSampleRate(suggestion.source.sampleRate)})
            </span>

            <ArrowRight size={14} style={{ color: "var(--accent-signal)" }} />

            <span
              className="font-mono-num text-[10px] font-semibold px-1.5 py-0.5 rounded-[3px] border"
              style={{
                backgroundColor: "var(--bg-panel)",
                borderColor: "var(--border-strong)",
                color: "var(--text-secondary)",
              }}
            >
              {dstBadge?.label ?? "OUTPUT"}
            </span>
            <span className="font-medium truncate" style={{ color: "var(--text-primary)" }}>
              {suggestion.target.name}
            </span>
          </div>

          <button
            type="button"
            onClick={onQuickStart}
            className="tactile-btn hover-accent shrink-0 inline-flex items-center gap-1.5 px-3.5 py-2 rounded-[6px] text-[12px] font-semibold"
            style={{
              backgroundColor: "var(--accent-signal)",
              color: "#ffffff",
            }}
          >
            <Zap size={13} />
            <span>Quick Start</span>
          </button>
        </div>
      ) : (
        <div
          className="px-3.5 py-2.5 rounded-[8px] border text-[12px]"
          style={{
            backgroundColor: "var(--status-warning-soft)",
            borderColor: "var(--status-warning-border)",
            color: "var(--text-primary)",
          }}
        >
          Connect at least one capture/loopback input and one render output device.
        </div>
      )}
    </section>
  );
}
