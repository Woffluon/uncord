import { createPortal } from "react-dom";
import { Moon, Sun, X } from "lucide-react";
import type { AppSettings } from "../lib/ipc";

interface SettingsDrawerProps {
  open: boolean;
  version: string;
  settings: AppSettings;
  onClose: () => void;
  onUpdateSettings: (next: AppSettings) => void;
  onReplayTour: () => void;
}

function SettingSwitchRow({
  title,
  description,
  checked,
  onToggle,
}: {
  title: string;
  description: string;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      className="flex items-start justify-between gap-3 p-3 rounded-[8px] border"
      style={{
        backgroundColor: "var(--bg-well)",
        borderColor: "var(--border-subtle)",
      }}
    >
      <div className="min-w-0">
        <div className="text-[12px] font-semibold" style={{ color: "var(--text-primary)" }}>
          {title}
        </div>
        <p className="mt-0.5 text-[11px] leading-[1.45]" style={{ color: "var(--text-secondary)" }}>
          {description}
        </p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={title}
        onClick={onToggle}
        className="hw-switch shrink-0 relative w-9 h-5 rounded-full border p-[2px] flex items-center mt-0.5"
        style={{
          backgroundColor: checked ? "var(--accent-signal)" : "var(--bg-panel)",
          borderColor: checked ? "var(--accent-signal)" : "var(--border-strong)",
        }}
      >
        <span
          className="hw-switch-thumb block w-3.5 h-3.5 rounded-full"
          style={{
            backgroundColor: checked ? "#ffffff" : "var(--text-secondary)",
            transform: checked ? "translateX(16px)" : "translateX(0px)",
          }}
        />
      </button>
    </div>
  );
}

export function SettingsDrawer({
  open,
  version,
  settings,
  onClose,
  onUpdateSettings,
  onReplayTour,
}: SettingsDrawerProps) {
  if (!open) return null;

  const isDark = settings.theme !== "light";

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex justify-end"
      role="dialog"
      aria-modal="true"
      aria-label="uncord settings"
    >
      <div
        className="drawer-backdrop fixed inset-0"
        style={{ backgroundColor: "rgba(0, 0, 0, 0.45)" }}
        onClick={onClose}
      />

      <aside
        className="drawer-surface relative z-10 w-full max-w-[360px] h-full border-l p-4 flex flex-col justify-between overflow-y-auto"
        style={{
          backgroundColor: "var(--bg-panel)",
          borderColor: "var(--border-strong)",
        }}
      >
        <div className="flex flex-col gap-4">
          <div
            className="flex items-center justify-between pb-3 border-b"
            style={{ borderColor: "var(--border-subtle)" }}
          >
            <div>
              <h2 className="text-[14px] font-semibold" style={{ color: "var(--text-primary)" }}>
                Settings
              </h2>
              <p className="font-mono-num text-[10px]" style={{ color: "var(--text-muted)" }}>
                v{version}
              </p>
            </div>
            <button
              type="button"
              aria-label="Close settings (Esc)"
              onClick={onClose}
              className="tactile-btn hover-surface p-1.5 rounded-[6px] border"
              style={{
                backgroundColor: "var(--bg-well)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              <X size={15} />
            </button>
          </div>

          <div className="flex flex-col gap-2.5">
            <SettingSwitchRow
              title="Close to tray"
              description="Terminates WebView2 on close while audio keeps running."
              checked={settings.minimizeToTrayOnClose}
              onToggle={() =>
                onUpdateSettings({
                  ...settings,
                  minimizeToTrayOnClose: !settings.minimizeToTrayOnClose,
                })
              }
            />

            <SettingSwitchRow
              title="Auto-reconnect"
              description="Restores routes when USB or Bluetooth devices return."
              checked={settings.autoReconnect}
              onToggle={() =>
                onUpdateSettings({
                  ...settings,
                  autoReconnect: !settings.autoReconnect,
                })
              }
            />

            <SettingSwitchRow
              title="Start minimized"
              description="Launch directly to the system tray."
              checked={settings.startMinimized}
              onToggle={() =>
                onUpdateSettings({
                  ...settings,
                  startMinimized: !settings.startMinimized,
                })
              }
            />
          </div>

          {/* Theme selector */}
          <div className="flex flex-col gap-1.5">
            <span
              className="text-[10px] font-semibold uppercase tracking-[0.08em]"
              style={{ color: "var(--text-secondary)" }}
            >
              Theme
            </span>
            <div
              className="grid grid-cols-2 gap-1.5 p-1 rounded-[7px] border"
              style={{
                backgroundColor: "var(--bg-well)",
                borderColor: "var(--border-subtle)",
              }}
            >
              <button
                type="button"
                aria-pressed={isDark}
                onClick={() => onUpdateSettings({ ...settings, theme: "dark" })}
                className="tactile-btn flex items-center justify-center gap-1.5 py-1.5 rounded-[5px] text-[12px] font-medium"
                style={{
                  backgroundColor: isDark ? "var(--bg-panel)" : "transparent",
                  color: isDark ? "var(--text-primary)" : "var(--text-secondary)",
                  boxShadow: isDark ? "inset 0 0 0 1px var(--border-strong)" : undefined,
                }}
              >
                <Moon size={13} />
                <span>Dark</span>
              </button>
              <button
                type="button"
                aria-pressed={!isDark}
                onClick={() => onUpdateSettings({ ...settings, theme: "light" })}
                className="tactile-btn flex items-center justify-center gap-1.5 py-1.5 rounded-[5px] text-[12px] font-medium"
                style={{
                  backgroundColor: !isDark ? "var(--bg-panel)" : "transparent",
                  color: !isDark ? "var(--text-primary)" : "var(--text-secondary)",
                  boxShadow: !isDark ? "inset 0 0 0 1px var(--border-strong)" : undefined,
                }}
              >
                <Sun size={13} />
                <span>Light</span>
              </button>
            </div>
          </div>

          {/* Keyboard shortcuts */}
          <div
            className="p-3 rounded-[8px] border flex flex-col gap-2"
            style={{
              backgroundColor: "var(--bg-well)",
              borderColor: "var(--border-subtle)",
            }}
          >
            <span
              className="text-[10px] font-semibold uppercase tracking-[0.08em]"
              style={{ color: "var(--text-secondary)" }}
            >
              Instant Keyboard Controls
            </span>
            <div className="flex flex-col gap-1.5 text-[11px]">
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>New audio route</span>
                <kbd
                  className="font-mono-num px-1.5 py-0.5 rounded-[4px] border text-[10px]"
                  style={{
                    backgroundColor: "var(--bg-panel)",
                    borderColor: "var(--border-strong)",
                    color: "var(--text-primary)",
                  }}
                >
                  N
                </kbd>
              </div>
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>Mute / unmute selected route</span>
                <kbd
                  className="font-mono-num px-1.5 py-0.5 rounded-[4px] border text-[10px]"
                  style={{
                    backgroundColor: "var(--bg-panel)",
                    borderColor: "var(--border-strong)",
                    color: "var(--text-primary)",
                  }}
                >
                  M
                </kbd>
              </div>
              <div className="flex items-center justify-between">
                <span style={{ color: "var(--text-secondary)" }}>Close active popover / drawer</span>
                <kbd
                  className="font-mono-num px-1.5 py-0.5 rounded-[4px] border text-[10px]"
                  style={{
                    backgroundColor: "var(--bg-panel)",
                    borderColor: "var(--border-strong)",
                    color: "var(--text-primary)",
                  }}
                >
                  Esc
                </kbd>
              </div>
            </div>
          </div>
        </div>

        <div className="pt-3 mt-4 border-t" style={{ borderColor: "var(--border-subtle)" }}>
          <button
            type="button"
            onClick={() => {
              onClose();
              onReplayTour();
            }}
            className="tactile-btn hover-surface w-full py-1.5 px-3 rounded-[6px] border text-[12px] font-medium"
            style={{
              backgroundColor: "var(--bg-well)",
              borderColor: "var(--border-subtle)",
              color: "var(--text-secondary)",
            }}
          >
            Show quick tour
          </button>
        </div>
      </aside>
    </div>,
    document.body,
  );
}
