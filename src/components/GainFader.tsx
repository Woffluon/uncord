import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { formatGainDb } from "../lib/audioUtils";

interface GainFaderProps {
  gainDb: number;
  muted: boolean;
  disabled?: boolean;
  onChangeGain: (nextGainDb: number) => void;
}

const MIN_GAIN = -60;
const MAX_GAIN = 12;
const ZERO_DB_RATIO = (0 - MIN_GAIN) / (MAX_GAIN - MIN_GAIN); // 60 / 72 = 83.33%

function clampGain(val: number): number {
  const clamped = Math.max(MIN_GAIN, Math.min(MAX_GAIN, val));
  return Math.round(clamped * 10) / 10;
}

function gainToRatio(gainDb: number): number {
  return Math.max(0, Math.min(1, (gainDb - MIN_GAIN) / (MAX_GAIN - MIN_GAIN)));
}

export function GainFader({ gainDb, muted, disabled = false, onChangeGain }: GainFaderProps) {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [dragging, setDragging] = useState(false);
  const activePointerRef = useRef<number | null>(null);
  const lastEmitRef = useRef<number>(0);

  const computeGainFromClientX = (clientX: number): number => {
    const el = trackRef.current;
    if (!el) return gainDb;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return gainDb;
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    const raw = MIN_GAIN + ratio * (MAX_GAIN - MIN_GAIN);
    // Subtle magnetic detent near 0.0 dB (±0.35 dB)
    if (Math.abs(raw) <= 0.35) return 0.0;
    return clampGain(raw);
  };

  const handlePointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    if (activePointerRef.current !== null) return;
    activePointerRef.current = e.pointerId;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
    const next = computeGainFromClientX(e.clientX);
    lastEmitRef.current = performance.now();
    onChangeGain(next);
  };

  const handlePointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!dragging || activePointerRef.current !== e.pointerId) return;
    const now = performance.now();
    if (now - lastEmitRef.current < 16) return;
    lastEmitRef.current = now;
    const next = computeGainFromClientX(e.clientX);
    onChangeGain(next);
  };

  const handlePointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (activePointerRef.current !== e.pointerId) return;
    const next = computeGainFromClientX(e.clientX);
    activePointerRef.current = null;
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    onChangeGain(next);
  };

  const handleDoubleClick = () => {
    if (disabled) return;
    onChangeGain(0.0);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const step = e.shiftKey ? 3.0 : 0.5;
    if (e.key === "ArrowRight" || e.key === "ArrowUp") {
      e.preventDefault();
      onChangeGain(clampGain(gainDb + step));
    } else if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
      e.preventDefault();
      onChangeGain(clampGain(gainDb - step));
    } else if (e.key === "0") {
      e.preventDefault();
      onChangeGain(0.0);
    } else if (e.key === "Home") {
      e.preventDefault();
      onChangeGain(MIN_GAIN);
    } else if (e.key === "End") {
      e.preventDefault();
      onChangeGain(MAX_GAIN);
    }
  };

  const ratio = gainToRatio(gainDb);
  const pct = ratio * 100;

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-[11px]">
        <span
          className="uppercase tracking-[0.08em] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          Gain
        </span>
        <button
          type="button"
          onClick={handleDoubleClick}
          disabled={disabled}
          title="Double-click or click to reset to 0.0 dB"
          className="tactile-btn font-mono-num text-[11px] font-medium px-1.5 py-0.5 rounded-[4px] border border-transparent hover-surface"
          style={{
            color:
              muted
                ? "var(--text-muted)"
                : gainDb > 0.05
                  ? "var(--accent-signal)"
                  : "var(--text-primary)",
          }}
        >
          {formatGainDb(gainDb)}
        </button>
      </div>

      <div
        ref={trackRef}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label="Route gain in decibels"
        aria-valuemin={MIN_GAIN}
        aria-valuemax={MAX_GAIN}
        aria-valuenow={gainDb}
        aria-valuetext={formatGainDb(gainDb)}
        data-dragging={dragging}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onDoubleClick={handleDoubleClick}
        onKeyDown={handleKeyDown}
        className="fader-track-shell relative h-6 flex items-center cursor-pointer touch-none"
      >
        {/* Recessed track */}
        <div
          className="relative w-full h-[6px] rounded-[3px] overflow-hidden"
          style={{
            backgroundColor: "var(--fader-track)",
            border: "1px solid var(--border-subtle)",
          }}
        >
          {/* Active signal fill */}
          <div
            className="h-full"
            style={{
              width: `${pct}%`,
              backgroundColor: muted ? "var(--text-muted)" : "var(--accent-signal)",
              opacity: muted ? 0.45 : 0.9,
            }}
          />
        </div>

        {/* Unity 0.0 dB calibration notch */}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute top-[2px] bottom-[2px] w-[1px]"
          style={{
            left: `${ZERO_DB_RATIO * 100}%`,
            backgroundColor: "var(--border-strong)",
          }}
        />

        {/* Tactile hardware fader cap */}
        <div
          aria-hidden="true"
          className="fader-thumb pointer-events-none absolute top-1/2 w-[14px] h-[18px] rounded-[3px] flex items-center justify-center"
          style={{
            left: `${pct}%`,
            transform: "translate(-50%, -50%)",
            backgroundColor: "var(--fader-thumb)",
            border: "1px solid var(--fader-thumb-ring)",
            boxShadow: "0 1px 4px rgba(0, 0, 0, 0.35)",
          }}
        >
          <span
            className="block w-[1.5px] h-[9px] rounded-[1px]"
            style={{
              backgroundColor: muted ? "var(--text-muted)" : "var(--accent-signal)",
            }}
          />
        </div>
      </div>
    </div>
  );
}
