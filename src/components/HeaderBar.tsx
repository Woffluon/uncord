import { Moon, Plus, Settings, Sun } from "lucide-react";
import type { Profile } from "../lib/ipc";
import { BrandMark } from "./BrandMark";
import { ProfilePopover } from "./ProfilePopover";

interface HeaderBarProps {
  profiles: Profile[];
  activeProfileId: string | null;
  routeCount: number;
  maxSoftwareMs: number | null;
  activeCount: number;
  hasWarning: boolean;
  theme: string;
  canAddRoute: boolean;
  openMenuKey: string | null;
  onSetOpenMenu: (key: string | null) => void;
  onLoadProfile: (profileId: string) => void;
  onSaveProfile: (name: string) => void;
  onDeleteProfile: (profileId: string) => void;
  onAddRoute: () => void;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
}

export function HeaderBar({
  profiles,
  activeProfileId,
  routeCount,
  maxSoftwareMs,
  activeCount,
  hasWarning,
  theme,
  canAddRoute,
  openMenuKey,
  onSetOpenMenu,
  onLoadProfile,
  onSaveProfile,
  onDeleteProfile,
  onAddRoute,
  onToggleTheme,
  onOpenSettings,
}: HeaderBarProps) {
  const isDark = theme !== "light";

  const swLabel =
    activeCount === 0
      ? "Standby"
      : maxSoftwareMs !== null
        ? `${maxSoftwareMs.toFixed(1)} ms`
        : "-- ms";

  const dotColor =
    activeCount === 0
      ? "var(--text-muted)"
      : hasWarning
        ? "var(--status-warning)"
        : "var(--status-healthy)";

  return (
    <header
      className="sticky top-0 z-30 border-b px-4 sm:px-5 py-2.5 flex items-center justify-between gap-3"
      style={{
        backgroundColor: "var(--bg-app)",
        borderColor: "var(--border-subtle)",
      }}
    >
      <div className="flex items-center gap-3 min-w-0">
        <BrandMark />

        <div
          className="hidden sm:inline-flex items-center gap-1.5 px-2 py-1 rounded-[5px] border font-mono-num text-[11px]"
          title="Aggregate software routing pipeline latency (excluding physical wireless/DAC floor)"
          style={{
            backgroundColor: "var(--bg-panel)",
            borderColor: "var(--border-subtle)",
            color: "var(--text-primary)",
          }}
        >
          <span
            className="w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: dotColor }}
          />
          <span>{swLabel}</span>
        </div>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <ProfilePopover
          profiles={profiles}
          activeProfileId={activeProfileId}
          routeCount={routeCount}
          isOpen={openMenuKey === "profiles"}
          onToggleOpen={() =>
            onSetOpenMenu(openMenuKey === "profiles" ? null : "profiles")
          }
          onClose={() => onSetOpenMenu(null)}
          onLoadProfile={onLoadProfile}
          onSaveProfile={onSaveProfile}
          onDeleteProfile={onDeleteProfile}
        />

        <button
          type="button"
          disabled={!canAddRoute}
          onClick={onAddRoute}
          title="Add new audio route (Shortcut: N)"
          className="tactile-btn hover-accent inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-[6px] text-[12px] font-semibold"
          style={{
            backgroundColor: "var(--accent-signal)",
            color: "#ffffff",
          }}
        >
          <Plus size={14} />
          <span>Route</span>
        </button>

        <button
          type="button"
          onClick={onToggleTheme}
          aria-label={isDark ? "Switch to light theme" : "Switch to dark theme"}
          title={isDark ? "Light" : "Dark"}
          className="tactile-btn hover-surface p-1.5 rounded-[6px] border"
          style={{
            backgroundColor: "var(--bg-panel)",
            borderColor: "var(--border-subtle)",
            color: "var(--text-secondary)",
          }}
        >
          {isDark ? <Sun size={15} /> : <Moon size={15} />}
        </button>

        <button
          type="button"
          onClick={onOpenSettings}
          aria-label="Open console settings"
          title="Settings"
          className="tactile-btn hover-surface p-1.5 rounded-[6px] border"
          style={{
            backgroundColor: "var(--bg-panel)",
            borderColor: "var(--border-subtle)",
            color: "var(--text-secondary)",
          }}
        >
          <Settings size={15} />
        </button>
      </div>
    </header>
  );
}
