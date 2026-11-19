import { Download, ExternalLink, RefreshCw, ShieldCheck, X } from "lucide-react";
import type { UpdateStateSnapshot } from "../lib/ipc";

interface UpdateBannerProps {
  state: UpdateStateSnapshot;
  dismissedVersion: string | null;
  dismissedError: string | null;
  onDismiss: () => void;
  onDownload: () => void;
  onInstall: () => void;
  onCancel: () => void;
  onOpenRelease: () => void;
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  const kb = bytes / 1024;
  return `${Math.max(1, Math.round(kb))} KB`;
}

export function UpdateBanner({
  state,
  dismissedVersion,
  dismissedError,
  onDismiss,
  onDownload,
  onInstall,
  onCancel,
  onOpenRelease,
}: UpdateBannerProps) {
  const { status, availableVersion, downloadedBytes, totalBytes, verifying, errorMessage, releaseUrl } =
    state;

  const isVisibleStatus =
    status === "available" ||
    status === "downloading" ||
    status === "verified" ||
    status === "installing" ||
    status === "error";

  if (!isVisibleStatus) return null;
  if (status === "available" && availableVersion && dismissedVersion === availableVersion) {
    return null;
  }
  if (status === "error" && dismissedError !== null && dismissedError === (errorMessage ?? "Update check failed")) {
    return null;
  }

  const pct =
    totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : 0;

  let summary = "";
  if (status === "available") {
    const sizeText = totalBytes > 0 ? ` (${formatBytes(totalBytes)})` : "";
    summary = `v${availableVersion ?? "new"} available${sizeText}`;
  } else if (status === "downloading") {
    summary = verifying
      ? "Verifying SHA-256 & PE signature..."
      : `Downloading v${availableVersion ?? ""} · ${pct}% (${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)})`;
  } else if (status === "verified") {
    summary = `v${availableVersion ?? ""} verified and ready to install`;
  } else if (status === "installing") {
    summary = "Launching verified installer...";
  } else if (status === "error") {
    summary = errorMessage ? `Update error: ${errorMessage}` : "Update check failed";
  }

  const isError = status === "error";

  return (
    <div
      role="status"
      aria-live="polite"
      className="px-3.5 py-2.5 rounded-[8px] border flex flex-col gap-2 text-[12px]"
      style={{
        backgroundColor: isError ? "var(--status-error-soft)" : "var(--bg-panel)",
        borderColor: isError ? "var(--status-error-border)" : "var(--border-strong)",
        color: "var(--text-primary)",
      }}
    >
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        <div className="flex items-center gap-2 min-w-0">
          {status === "verified" ? (
            <ShieldCheck size={14} className="shrink-0" style={{ color: "var(--status-live)" }} />
          ) : (
            <RefreshCw
              size={14}
              className={`shrink-0 ${status === "downloading" || status === "installing" ? "animate-spin" : ""}`}
              style={{ color: isError ? "var(--status-error)" : "var(--accent-signal)" }}
            />
          )}
          <span className="font-mono-num truncate">{summary}</span>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          {status === "available" && (
            <button
              type="button"
              onClick={onDownload}
              className="tactile-btn hover-accent h-7 px-2.5 rounded-[6px] text-[11px] font-semibold inline-flex items-center gap-1.5"
              style={{ backgroundColor: "var(--accent-signal)", color: "#ffffff" }}
            >
              <Download size={12} />
              <span>Download</span>
            </button>
          )}

          {status === "downloading" && !verifying && (
            <button
              type="button"
              onClick={onCancel}
              className="tactile-btn hover-surface h-7 px-2.5 rounded-[6px] border text-[11px] font-medium"
              style={{
                backgroundColor: "var(--bg-well)",
                borderColor: "var(--border-strong)",
                color: "var(--text-secondary)",
              }}
            >
              Cancel
            </button>
          )}

          {status === "verified" && (
            <button
              type="button"
              onClick={onInstall}
              className="tactile-btn hover-accent h-7 px-2.5 rounded-[6px] text-[11px] font-semibold inline-flex items-center gap-1.5"
              style={{ backgroundColor: "var(--accent-signal)", color: "#ffffff" }}
            >
              <ShieldCheck size={12} />
              <span>Install &amp; Restart</span>
            </button>
          )}

          {releaseUrl && status !== "installing" && (
            <button
              type="button"
              onClick={onOpenRelease}
              className="tactile-btn hover-surface h-7 px-2.5 rounded-[6px] border text-[11px] font-medium inline-flex items-center gap-1"
              style={{
                backgroundColor: "var(--bg-well)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              <ExternalLink size={11} />
              <span>Release</span>
            </button>
          )}

          {(status === "available" || status === "error") && (
            <button
              type="button"
              aria-label="Dismiss update notification"
              onClick={onDismiss}
              className="tactile-btn p-1 rounded-[4px]"
              style={{ color: "var(--text-secondary)" }}
            >
              <X size={13} />
            </button>
          )}
        </div>
      </div>

      {status === "downloading" && (
        <div
          className="h-1 w-full rounded-full overflow-hidden"
          style={{ backgroundColor: "var(--bg-well)" }}
        >
          <div
            className="h-full transition-[width] duration-150 ease-out"
            style={{
              width: verifying ? "100%" : `${pct}%`,
              backgroundColor: "var(--accent-signal)",
            }}
          />
        </div>
      )}
    </div>
  );
}
