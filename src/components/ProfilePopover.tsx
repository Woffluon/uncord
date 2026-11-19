import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { Bookmark, Check, ChevronDown, Plus, Trash2 } from "lucide-react";
import type { Profile } from "../lib/ipc";
import { usePopoverPlacement } from "./popoverPlacement";

interface ProfilePopoverProps {
  profiles: Profile[];
  activeProfileId: string | null;
  routeCount: number;
  isOpen: boolean;
  onToggleOpen: () => void;
  onClose: () => void;
  onLoadProfile: (profileId: string) => void;
  onSaveProfile: (name: string) => void;
  onDeleteProfile: (profileId: string) => void;
}

export function ProfilePopover({
  profiles,
  activeProfileId,
  routeCount,
  isOpen,
  onToggleOpen,
  onClose,
  onLoadProfile,
  onSaveProfile,
  onDeleteProfile,
}: ProfilePopoverProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [newName, setNewName] = useState("");

  const activeProfile = profiles.find((p) => p.id === activeProfileId);

  const { style, ready } = usePopoverPlacement({
    open: isOpen,
    triggerRef,
    align: "right",
    minWidth: 290,
    maxWidth: 340,
    estimatedHeight: 320,
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

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    const trimmed = newName.trim();
    if (!trimmed) return;
    onSaveProfile(trimmed);
    setNewName("");
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        onClick={onToggleOpen}
        className="tactile-btn hover-surface inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-[6px] border text-[12px] font-medium max-w-[190px]"
        style={{
          backgroundColor: "var(--bg-panel)",
          borderColor: isOpen ? "var(--accent-signal)" : "var(--border-subtle)",
          color: "var(--text-primary)",
        }}
      >
        <Bookmark size={13} className="shrink-0" style={{ color: "var(--text-secondary)" }} />
        <span className="truncate">
          {activeProfile ? activeProfile.name : "Profiles"}
        </span>
        <ChevronDown
          size={13}
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
            ref={popoverRef}
            role="dialog"
            aria-label="Routing profiles"
            className="popover-surface z-50 rounded-[8px] border p-2.5 flex flex-col gap-2.5"
            style={{
              ...style,
              backgroundColor: "var(--bg-popover)",
              borderColor: "var(--border-strong)",
            }}
          >
            <div className="flex items-center justify-between px-1">
              <span
                className="text-[10px] font-semibold uppercase tracking-[0.08em]"
                style={{ color: "var(--text-secondary)" }}
              >
                Saved Profiles
              </span>
              <span className="font-mono-num text-[10px]" style={{ color: "var(--text-muted)" }}>
                {profiles.length} saved
              </span>
            </div>

            <div className="flex flex-col gap-1 max-h-[180px] overflow-y-auto">
              {profiles.length === 0 ? (
                <div
                  className="px-2 py-3 text-[12px] text-center rounded-[5px]"
                  style={{
                    backgroundColor: "var(--bg-well)",
                    color: "var(--text-muted)",
                  }}
                >
                  No saved routing profiles yet.
                </div>
              ) : (
                profiles.map((profile) => {
                  const active = profile.id === activeProfileId;
                  return (
                    <div
                      key={profile.id}
                      className="flex items-center gap-1 rounded-[5px]"
                      style={{
                        backgroundColor: active ? "var(--bg-active-row)" : "transparent",
                      }}
                    >
                      <button
                        type="button"
                        onClick={() => {
                          onLoadProfile(profile.id);
                          onClose();
                        }}
                        className="tactile-btn hover-option flex-1 min-w-0 flex items-center justify-between gap-2 px-2 py-1.5 rounded-[5px] text-left"
                      >
                        <div className="min-w-0 flex items-center gap-1.5">
                          <span className="w-3.5 shrink-0 flex justify-center">
                            {active && (
                              <Check size={13} style={{ color: "var(--accent-signal)" }} />
                            )}
                          </span>
                          <span
                            className="text-[12px] font-medium truncate"
                            style={{ color: "var(--text-primary)" }}
                          >
                            {profile.name}
                          </span>
                        </div>
                        <span
                          className="font-mono-num text-[10px] shrink-0"
                          style={{ color: "var(--text-muted)" }}
                        >
                          {profile.routes.length}r
                        </span>
                      </button>

                      <button
                        type="button"
                        aria-label={`Delete profile ${profile.name}`}
                        onClick={() => onDeleteProfile(profile.id)}
                        className="tactile-btn hover-danger p-1.5 rounded-[5px] border border-transparent shrink-0"
                        style={{ color: "var(--text-muted)" }}
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  );
                })
              )}
            </div>

            <form
              onSubmit={handleSubmit}
              className="pt-2 border-t flex items-center gap-1.5"
              style={{ borderColor: "var(--border-subtle)" }}
            >
              <input
                type="text"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder={routeCount > 0 ? "Save current routing as..." : "Profile name..."}
                className="flex-1 min-w-0 px-2 py-1.5 rounded-[5px] border text-[12px]"
                style={{
                  backgroundColor: "var(--bg-well)",
                  borderColor: "var(--border-subtle)",
                  color: "var(--text-primary)",
                }}
              />
              <button
                type="submit"
                disabled={!newName.trim()}
                className="tactile-btn hover-accent shrink-0 inline-flex items-center gap-1 px-2.5 py-1.5 rounded-[5px] text-[11px] font-semibold"
                style={{
                  backgroundColor: "var(--accent-signal)",
                  color: "#ffffff",
                }}
              >
                <Plus size={12} />
                <span>Save</span>
              </button>
            </form>
          </div>,
          document.body,
        )}
    </>
  );
}
