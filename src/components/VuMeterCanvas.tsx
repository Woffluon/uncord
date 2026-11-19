import { useEffect, useRef, type MutableRefObject } from "react";
import type { RouteTelemetry } from "../lib/ipc";
import { linearToDb } from "../lib/audioUtils";

interface VuMeterCanvasProps {
  routeId: string;
  enabled: boolean;
  muted: boolean;
  theme: string;
  telemetryMapRef: MutableRefObject<Map<string, RouteTelemetry>>;
}

const MIN_DB = -60;
const MAX_DB = 0;

function dbToNorm(db: number): number {
  const clamped = Math.max(MIN_DB, Math.min(MAX_DB, db));
  return (clamped - MIN_DB) / (MAX_DB - MIN_DB);
}

export function VuMeterCanvas({
  routeId,
  enabled,
  muted,
  theme,
  telemetryMapRef,
}: VuMeterCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const stateFlagsRef = useRef({ enabled, muted, theme });
  stateFlagsRef.current = { enabled, muted, theme };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    let width = canvas.clientWidth || 220;
    let height = canvas.clientHeight || 40;
    let dpr = Math.min(window.devicePixelRatio || 1, 2);

    const syncCanvasSize = () => {
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      width = Math.max(120, canvas.clientWidth || 220);
      height = Math.max(36, canvas.clientHeight || 40);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
    };

    syncCanvasSize();

    const resizeObserver = new ResizeObserver(() => {
      syncCanvasSize();
    });
    resizeObserver.observe(canvas);

    // Ballistic meter state kept outside React — zero re-renders per frame
    let peakDispL = 0;
    let peakDispR = 0;
    let rmsDispL = 0;
    let rmsDispR = 0;
    let holdL = 0;
    let holdR = 0;
    let holdUntilL = 0;
    let holdUntilR = 0;
    let clipUntil = 0;
    let lastTs = performance.now();
    let rafId = 0;

    // Resolve palette tokens when theme changes
    let cachedTheme = "";
    let colors = {
      bg: "#0b0c0e",
      tick: "#383d47",
      textMuted: "#626873",
      healthy: "#22c55e",
      warning: "#f59e0b",
      signal: "#f25c2a",
      error: "#ef4444",
      mutedBar: "#343942",
      peakHold: "#f5f5f3",
    };

    const refreshPalette = (currentTheme: string) => {
      if (cachedTheme === currentTheme) return;
      cachedTheme = currentTheme;
      const styles = getComputedStyle(document.documentElement);
      const pick = (name: string, fallback: string) =>
        styles.getPropertyValue(name).trim() || fallback;
      colors = {
        bg: pick("--meter-bg", "#0b0c0e"),
        tick: pick("--meter-tick", "#383d47"),
        textMuted: pick("--text-muted", "#626873"),
        healthy: pick("--status-healthy", "#22c55e"),
        warning: pick("--status-warning", "#f59e0b"),
        signal: pick("--accent-signal", "#f25c2a"),
        error: pick("--status-error", "#ef4444"),
        mutedBar: pick("--border-strong", "#343942"),
        peakHold: pick("--text-primary", "#f5f5f3"),
      };
    };

    const drawChannel = (
      x: number,
      y: number,
      barW: number,
      barH: number,
      label: string,
      rmsNorm: number,
      peakNorm: number,
      holdNorm: number,
      isMutedOrStopped: boolean,
    ) => {
      // Channel label ("L" / "R")
      ctx.fillStyle = colors.textMuted;
      ctx.font = '600 9px "JetBrains Mono", Consolas, monospace';
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.fillText(label, 0, y + barH / 2 + 0.5);

      // Well track
      ctx.fillStyle = colors.bg;
      ctx.fillRect(x, y, barW, barH);

      if (isMutedOrStopped) {
        if (peakNorm > 0.004) {
          ctx.fillStyle = colors.mutedBar;
          ctx.fillRect(x, y, barW * peakNorm, barH);
        }
        return;
      }

      // RMS body with zoned colors (-60..-12 green, -12..-3 amber, -3..0 signal)
      const zoneAmber = dbToNorm(-12);
      const zoneHot = dbToNorm(-3);

      const drawZonedFill = (norm: number, alpha: number) => {
        if (norm <= 0.002) return;
        ctx.save();
        ctx.globalAlpha = alpha;
        const pxEnd = barW * norm;

        const w1 = Math.min(pxEnd, barW * zoneAmber);
        if (w1 > 0) {
          ctx.fillStyle = colors.healthy;
          ctx.fillRect(x, y, w1, barH);
        }

        if (pxEnd > barW * zoneAmber) {
          const x2 = x + barW * zoneAmber;
          const w2 = Math.min(pxEnd, barW * zoneHot) - barW * zoneAmber;
          if (w2 > 0) {
            ctx.fillStyle = colors.warning;
            ctx.fillRect(x2, y, w2, barH);
          }
        }

        if (pxEnd > barW * zoneHot) {
          const x3 = x + barW * zoneHot;
          const w3 = pxEnd - barW * zoneHot;
          if (w3 > 0) {
            ctx.fillStyle = colors.signal;
            ctx.fillRect(x3, y, w3, barH);
          }
        }
        ctx.restore();
      };

      // Peak ghost envelope + brighter RMS core
      drawZonedFill(peakNorm, 0.34);
      drawZonedFill(rmsNorm, 0.92);

      // Instant peak frontier line
      if (peakNorm > 0.01) {
        const px = x + Math.min(barW - 1.5, barW * peakNorm);
        ctx.fillStyle = peakNorm > zoneHot ? colors.signal : colors.healthy;
        ctx.fillRect(px, y, 1.5, barH);
      }

      // Peak-hold marker
      if (holdNorm > 0.01) {
        const hx = x + Math.min(barW - 1.5, barW * holdNorm);
        ctx.fillStyle = holdNorm > 0.985 ? colors.error : colors.peakHold;
        ctx.fillRect(hx, y, 1.5, barH);
      }
    };

    const tick = (now: number) => {
      const dt = Math.min(0.1, Math.max(0.001, (now - lastTs) / 1000));
      lastTs = now;

      const flags = stateFlagsRef.current;
      refreshPalette(flags.theme);

      const t = telemetryMapRef.current.get(routeId);
      const isStopped = !flags.enabled || (t !== undefined && t.status.state === "stopped");
      const isMutedOrStopped = flags.muted || isStopped;

      const targetPeakL = isMutedOrStopped ? 0 : dbToNorm(linearToDb(t?.peakLeft ?? 0));
      const targetPeakR = isMutedOrStopped ? 0 : dbToNorm(linearToDb(t?.peakRight ?? 0));
      const targetRmsL = isMutedOrStopped ? 0 : dbToNorm(linearToDb(t?.rmsLeft ?? 0));
      const targetRmsR = isMutedOrStopped ? 0 : dbToNorm(linearToDb(t?.rmsRight ?? 0));

      // Fast attack, smooth ballistic release (~1.2 normalized units/sec)
      const decay = dt * 1.15;
      const rmsDecay = dt * 0.85;

      peakDispL = targetPeakL >= peakDispL ? targetPeakL : Math.max(targetPeakL, peakDispL - decay);
      peakDispR = targetPeakR >= peakDispR ? targetPeakR : Math.max(targetPeakR, peakDispR - decay);
      rmsDispL = targetRmsL >= rmsDispL ? targetRmsL : Math.max(targetRmsL, rmsDispL - rmsDecay);
      rmsDispR = targetRmsR >= rmsDispR ? targetRmsR : Math.max(targetRmsR, rmsDispR - rmsDecay);

      if (targetPeakL >= holdL) {
        holdL = targetPeakL;
        holdUntilL = now + 700;
      } else if (now > holdUntilL) {
        holdL = Math.max(peakDispL, holdL - dt * 0.45);
      }

      if (targetPeakR >= holdR) {
        holdR = targetPeakR;
        holdUntilR = now + 700;
      } else if (now > holdUntilR) {
        holdR = Math.max(peakDispR, holdR - dt * 0.45);
      }

      if (!isMutedOrStopped && t && (t.clipping || t.peakLeft > 0.995 || t.peakRight > 0.995)) {
        clipUntil = now + 1150;
      } else if (isStopped) {
        clipUntil = 0;
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);

      const labelPad = 14;
      const clipLedW = 14;
      const gapBeforeClip = 6;
      const barW = Math.max(40, width - labelPad - clipLedW - gapBeforeClip);
      const barH = 7;
      const topY = 2;
      const bottomY = topY + barH + 4;

      drawChannel(labelPad, topY, barW, barH, "L", rmsDispL, peakDispL, holdL, isMutedOrStopped);
      drawChannel(labelPad, bottomY, barW, barH, "R", rmsDispR, peakDispR, holdR, isMutedOrStopped);

      // Clipping LED on right end
      const clipX = labelPad + barW + gapBeforeClip;
      const clipH = barH * 2 + 4;
      const clippingActive = now < clipUntil;
      ctx.fillStyle = clippingActive ? colors.error : colors.bg;
      ctx.fillRect(clipX, topY, clipLedW, clipH);
      ctx.strokeStyle = clippingActive ? colors.error : colors.tick;
      ctx.lineWidth = 1;
      ctx.strokeRect(clipX + 0.5, topY + 0.5, clipLedW - 1, clipH - 1);

      // Tiny "PK" inside clip LED
      ctx.fillStyle = clippingActive ? "#ffffff" : colors.textMuted;
      ctx.font = '700 7px "JetBrains Mono", Consolas, monospace';
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText("PK", clipX + clipLedW / 2, topY + clipH / 2 + 0.5);

      // dB scale markings underneath
      const scaleY = bottomY + barH + 5;
      const ticks = [-48, -24, -12, -6, 0];
      ctx.font = '500 8px "JetBrains Mono", Consolas, monospace';
      ctx.textBaseline = "top";

      for (const db of ticks) {
        const norm = dbToNorm(db);
        const tx = Math.round(labelPad + barW * norm);
        ctx.fillStyle = colors.tick;
        ctx.fillRect(tx, scaleY - 3, 1, 3);
        ctx.fillStyle = colors.textMuted;
        ctx.textAlign = db === 0 ? "right" : "center";
        ctx.fillText(db === 0 ? "0" : `${db}`, db === 0 ? labelPad + barW : tx, scaleY + 1);
      }

      rafId = window.requestAnimationFrame(tick);
    };

    rafId = window.requestAnimationFrame(tick);

    return () => {
      window.cancelAnimationFrame(rafId);
      resizeObserver.disconnect();
    };
  }, [routeId, telemetryMapRef]);

  return (
    <canvas
      ref={canvasRef}
      aria-label="Stereo Peak and RMS level meter"
      className="block w-full h-[38px]"
    />
  );
}
