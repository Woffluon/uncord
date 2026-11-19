import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

interface OnboardingTourProps {
  open: boolean;
  onClose: () => void;
}

const TOUR_STEPS = [
  {
    title: "Input & channels",
    body: "Pick a capture device or loopback source. Multi-channel USB amps expose stereo pairs and dry DI.",
  },
  {
    title: "Output & latency",
    body: "Route to wired, 2.4 GHz, or Bluetooth A2DP outputs. Open the latency badge to separate software delay from codec floor.",
  },
  {
    title: "Tray & shortcuts",
    body: "Closing the window moves uncord to the tray and frees WebView2 RAM. Press N for a new route, M to mute.",
  },
] as const;

export function OnboardingTour({ open, onClose }: OnboardingTourProps) {
  const [step, setStep] = useState(0);

  useEffect(() => {
    if (open) setStep(0);
  }, [open]);

  if (!open) return null;

  const current = TOUR_STEPS[step];
  const isLast = step === TOUR_STEPS.length - 1;

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      aria-label="uncord quick tour"
    >
      <div
        className="modal-backdrop absolute inset-0"
        style={{ backgroundColor: "rgba(0, 0, 0, 0.56)" }}
        onClick={onClose}
      />

      <div
        className="modal-surface relative z-10 w-full max-w-[380px] rounded-[10px] border p-4 flex flex-col gap-4"
        style={{
          backgroundColor: "var(--bg-panel)",
          borderColor: "var(--border-strong)",
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <span
            className="font-mono-num text-[11px]"
            style={{ color: "var(--text-muted)" }}
          >
            {step + 1} / {TOUR_STEPS.length}
          </span>
          <button
            type="button"
            aria-label="Close tour (Esc)"
            onClick={onClose}
            className="tactile-btn hover-surface p-1 rounded-[5px] border"
            style={{
              backgroundColor: "var(--bg-well)",
              borderColor: "var(--border-subtle)",
              color: "var(--text-secondary)",
            }}
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex flex-col gap-1.5">
          <h2
            className="text-[15px] font-semibold tracking-[-0.01em]"
            style={{ color: "var(--text-primary)" }}
          >
            {current.title}
          </h2>
          <p className="text-[13px] leading-[1.5]" style={{ color: "var(--text-secondary)" }}>
            {current.body}
          </p>
        </div>

        <div className="flex items-center justify-between gap-2 pt-1">
          <button
            type="button"
            onClick={onClose}
            className="tactile-btn hover-surface px-2.5 py-1.5 rounded-[6px] text-[12px]"
            style={{ color: "var(--text-muted)" }}
          >
            Skip
          </button>

          <div className="flex items-center gap-1.5">
            {step > 0 && (
              <button
                type="button"
                onClick={() => setStep((s) => Math.max(0, s - 1))}
                className="tactile-btn hover-surface px-3 py-1.5 rounded-[6px] border text-[12px] font-medium"
                style={{
                  backgroundColor: "var(--bg-well)",
                  borderColor: "var(--border-subtle)",
                  color: "var(--text-primary)",
                }}
              >
                Back
              </button>
            )}
            <button
              type="button"
              onClick={() => {
                if (isLast) {
                  onClose();
                } else {
                  setStep((s) => Math.min(TOUR_STEPS.length - 1, s + 1));
                }
              }}
              className="tactile-btn hover-accent px-3.5 py-1.5 rounded-[6px] text-[12px] font-semibold"
              style={{
                backgroundColor: "var(--accent-signal)",
                color: "#ffffff",
              }}
            >
              {isLast ? "Done" : "Next"}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
