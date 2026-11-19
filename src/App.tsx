import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, X } from "lucide-react";
import type {
  AppSettings,
  EngineSnapshot,
  RouteConfig,
  RouteTelemetry,
  UpdateStateSnapshot,
} from "./lib/ipc";
import {
  audioClient,
  subscribeDeviceChanges,
  subscribeTelemetry,
  subscribeUpdateState,
} from "./lib/audioClient";
import {
  computeAggregateSoftwareLatency,
  pickSmartRouteDefaults,
} from "./lib/audioUtils";
import { EmptyState } from "./components/EmptyState";
import { HeaderBar } from "./components/HeaderBar";
import { OnboardingTour } from "./components/OnboardingTour";
import { RouteCard } from "./components/RouteCard";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { UpdateBanner } from "./components/UpdateBanner";

const TOUR_STORAGE_KEY = "uncord.tour.v1";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

function toErrorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  return "Unexpected audio engine error";
}

export function App() {
  const [snapshot, setSnapshot] = useState<EngineSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [errorBanner, setErrorBanner] = useState<string | null>(null);

  const [selectedRouteId, setSelectedRouteId] = useState<string | null>(null);
  const [openMenuKey, setOpenMenuKey] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [tourOpen, setTourOpen] = useState<boolean>(() => {
    try {
      return window.localStorage.getItem(TOUR_STORAGE_KEY) !== "done";
    } catch {
      return false;
    }
  });

  const closeTour = useCallback(() => {
    setTourOpen(false);
    try {
      window.localStorage.setItem(TOUR_STORAGE_KEY, "done");
    } catch {
      // Ignore storage errors in restricted WebView contexts
    }
  }, []);

  // Instant 0ms flags for keyboard-initiated actions (N, M, Esc)
  const [instantRouteIds, setInstantRouteIds] = useState<Set<string>>(() => new Set());
  const [instantMuteRouteId, setInstantMuteRouteId] = useState<string | null>(null);
  const [instantUiFlag, setInstantUiFlag] = useState(false);
  const [updateState, setUpdateState] = useState<UpdateStateSnapshot | null>(null);
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const [dismissedUpdateError, setDismissedUpdateError] = useState<string | null>(null);

  // 60fps Canvas VU Meters read directly from telemetryMapRef (ZERO React re-render per frame)
  const telemetryMapRef = useRef<Map<string, RouteTelemetry>>(new Map());
  // Throttled summary state for textual readouts & status badges (~280ms)
  const [summaryTelemetryMap, setSummaryTelemetryMap] = useState<Map<string, RouteTelemetry>>(
    () => new Map(),
  );
  const lastSummarySyncRef = useRef<number>(0);

  const refreshSnapshot = useCallback(async () => {
    try {
      const next = await audioClient.getSnapshot();
      setSnapshot(next);
      setSelectedRouteId((prev) => {
        if (prev && next.routes.some((r) => r.id === prev)) return prev;
        return next.routes[0]?.id ?? null;
      });
    } catch (err) {
      setErrorBanner(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshSnapshot();
    void audioClient.getUpdateState().then(setUpdateState).catch(() => {});
    void audioClient
      .getTelemetry()
      .then((items) => {
        const nextMap = new Map<string, RouteTelemetry>();
        for (const item of items) {
          telemetryMapRef.current.set(item.routeId, item);
          nextMap.set(item.routeId, item);
        }
        setSummaryTelemetryMap(nextMap);
      })
      .catch(() => {});
  }, [refreshSnapshot]);

  // Apply theme to <html> immediately
  const theme = snapshot?.settings.theme === "light" ? "light" : "dark";
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Telemetry subscription: writes to mutable ref at 30Hz, throttles text summary updates
  useEffect(() => {
    const unsubscribe = subscribeTelemetry((items) => {
      let statusChanged = items.length !== summaryTelemetryMap.size;
      for (const item of items) {
        const prev = telemetryMapRef.current.get(item.routeId);
        telemetryMapRef.current.set(item.routeId, item);
        if (!prev || prev.status.state !== item.status.state || prev.xruns !== item.xruns) {
          statusChanged = true;
        }
      }

      const now = performance.now();
      if (statusChanged || now - lastSummarySyncRef.current >= 280) {
        lastSummarySyncRef.current = now;
        const copy = new Map<string, RouteTelemetry>();
        for (const item of items) {
          copy.set(item.routeId, item);
        }
        setSummaryTelemetryMap(copy);
      }
    });
    return unsubscribe;
  }, [summaryTelemetryMap.size]);

  // Hot-plug device updates + window focus sync (e.g. tray Quick Mute / Profile switch)
  useEffect(() => {
    const unsubDevices = subscribeDeviceChanges((nextDevices) => {
      if (nextDevices) {
        setSnapshot((prev) => (prev ? { ...prev, devices: nextDevices } : prev));
      } else {
        void refreshSnapshot();
      }
    });

    const handleFocus = () => {
      void refreshSnapshot();
    };
    window.addEventListener("focus", handleFocus);
    return () => {
      unsubDevices();
      window.removeEventListener("focus", handleFocus);
    };
  }, [refreshSnapshot]);

  useEffect(() => {
    return subscribeUpdateState((next) => {
      setUpdateState(next);
    });
  }, []);

  const handleCheckUpdates = useCallback(() => {
    setDismissedUpdateVersion(null);
    setDismissedUpdateError(null);
    void audioClient.checkForUpdates().then(setUpdateState).catch((err) => {
      setErrorBanner(toErrorMessage(err));
    });
  }, []);

  const handleCancelUpdate = useCallback(() => {
    void audioClient.cancelUpdateCheck().then(setUpdateState).catch((err) => {
      setErrorBanner(toErrorMessage(err));
    });
  }, []);

  const handleDownloadUpdate = useCallback(() => {
    setDismissedUpdateError(null);
    void audioClient.downloadUpdate().then(setUpdateState).catch((err) => {
      setErrorBanner(toErrorMessage(err));
    });
  }, []);

  const handleInstallUpdate = useCallback(() => {
    void audioClient.installUpdate().catch((err) => {
      setErrorBanner(toErrorMessage(err));
    });
  }, []);

  const handleOpenReleasePage = useCallback(() => {
    void audioClient.openReleasePage().catch((err) => {
      setErrorBanner(toErrorMessage(err));
    });
  }, []);

  const handleAddRoute = useCallback(
    async (opts?: { instant?: boolean }) => {
      if (!snapshot) return;
      const defaults = pickSmartRouteDefaults(snapshot.devices, snapshot.routes);
      if (!defaults.source || !defaults.target) {
        setErrorBanner("Connect at least one capture/loopback input and one output device.");
        return;
      }

      try {
        const created = await audioClient.addRoute({
          name: defaults.routeName,
          sourceDeviceId: defaults.source.id,
          sourceIsLoopback: defaults.source.flow === "loopback",
          channelMode: defaults.channelMode,
          targetDeviceId: defaults.target.id,
          gainDb: 0.0,
          exclusiveOutput: false,
          latencyMode: "ultraLow",
        });

        if (opts?.instant) {
          setInstantRouteIds((prev) => {
            const next = new Set(prev);
            next.add(created.id);
            return next;
          });
        }

        setSnapshot((prev) =>
          prev
            ? {
                ...prev,
                routes: [...prev.routes, created],
                settings: { ...prev.settings, activeProfileId: null },
              }
            : prev,
        );
        setSelectedRouteId(created.id);
        setOpenMenuKey(null);
      } catch (err) {
        setErrorBanner(toErrorMessage(err));
      }
    },
    [snapshot],
  );

  const handleUpdateRoute = useCallback(
    async (nextRoute: RouteConfig) => {
      if (!snapshot) return;
      const previousRoute = snapshot.routes.find((r) => r.id === nextRoute.id);

      setSnapshot((prev) =>
        prev
          ? {
              ...prev,
              routes: prev.routes.map((r) => (r.id === nextRoute.id ? nextRoute : r)),
            }
          : prev,
      );

      try {
        const confirmed = await audioClient.updateRoute(nextRoute);
        setSnapshot((prev) =>
          prev
            ? {
                ...prev,
                routes: prev.routes.map((r) => (r.id === confirmed.id ? confirmed : r)),
              }
            : prev,
        );
      } catch (err) {
        if (previousRoute) {
          setSnapshot((prev) =>
            prev
              ? {
                  ...prev,
                  routes: prev.routes.map((r) =>
                    r.id === previousRoute.id ? previousRoute : r,
                  ),
                }
              : prev,
          );
        }
        setErrorBanner(toErrorMessage(err));
      }
    },
    [snapshot],
  );

  const handleGainMuteChange = useCallback(
    (routeId: string, gainDb: number, muted: boolean, instant = false) => {
      if (instant) {
        setInstantMuteRouteId(routeId);
        window.requestAnimationFrame(() => {
          setInstantMuteRouteId((curr) => (curr === routeId ? null : curr));
        });
      }

      setSnapshot((prev) =>
        prev
          ? {
              ...prev,
              routes: prev.routes.map((r) =>
                r.id === routeId ? { ...r, gainDb, muted } : r,
              ),
            }
          : prev,
      );

      void audioClient.setRouteGainMute(routeId, gainDb, muted).catch((err) => {
        setErrorBanner(toErrorMessage(err));
      });
    },
    [],
  );

  const handleRemoveRoute = useCallback(async (routeId: string) => {
    try {
      await audioClient.removeRoute(routeId);
      telemetryMapRef.current.delete(routeId);
      setSnapshot((prev) => {
        if (!prev) return prev;
        const remaining = prev.routes.filter((r) => r.id !== routeId);
        setSelectedRouteId((sel) =>
          sel === routeId ? (remaining[0]?.id ?? null) : sel,
        );
        return { ...prev, routes: remaining };
      });
    } catch (err) {
      setErrorBanner(toErrorMessage(err));
    }
  }, []);

  const handleUpdateSettings = useCallback(
    async (nextSettings: AppSettings) => {
      const prevSettings = snapshot?.settings;
      setSnapshot((prev) => (prev ? { ...prev, settings: nextSettings } : prev));
      try {
        const confirmed = await audioClient.updateSettings(nextSettings);
        setSnapshot((prev) => (prev ? { ...prev, settings: confirmed } : prev));
      } catch (err) {
        if (prevSettings) {
          setSnapshot((prev) => (prev ? { ...prev, settings: prevSettings } : prev));
        }
        setErrorBanner(toErrorMessage(err));
      }
    },
    [snapshot?.settings],
  );

  const handleToggleTheme = useCallback(() => {
    if (!snapshot) return;
    const nextTheme = theme === "dark" ? "light" : "dark";
    void handleUpdateSettings({ ...snapshot.settings, theme: nextTheme });
  }, [snapshot, theme, handleUpdateSettings]);

  const handleLoadProfile = useCallback(async (profileId: string) => {
    try {
      const nextSnap = await audioClient.loadProfile(profileId);
      telemetryMapRef.current.clear();
      setSnapshot(nextSnap);
      setSelectedRouteId(nextSnap.routes[0]?.id ?? null);
    } catch (err) {
      setErrorBanner(toErrorMessage(err));
    }
  }, []);

  const handleSaveProfile = useCallback(async (name: string) => {
    try {
      const created = await audioClient.saveProfile(name);
      setSnapshot((prev) =>
        prev
          ? {
              ...prev,
              profiles: [...prev.profiles, created],
              settings: { ...prev.settings, activeProfileId: created.id },
            }
          : prev,
      );
    } catch (err) {
      setErrorBanner(toErrorMessage(err));
    }
  }, []);

  const handleDeleteProfile = useCallback(async (profileId: string) => {
    try {
      await audioClient.deleteProfile(profileId);
      setSnapshot((prev) =>
        prev
          ? {
              ...prev,
              profiles: prev.profiles.filter((p) => p.id !== profileId),
              settings: {
                ...prev.settings,
                activeProfileId:
                  prev.settings.activeProfileId === profileId
                    ? null
                    : prev.settings.activeProfileId,
              },
            }
          : prev,
      );
    } catch (err) {
      setErrorBanner(toErrorMessage(err));
    }
  }, []);

  // Global keyboard shortcuts: N (new route), M (mute selected), Esc (close) — all 0ms instant!
  useEffect(() => {
    const handleKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") {
        if (openMenuKey !== null || settingsOpen || tourOpen || errorBanner !== null) {
          e.preventDefault();
          setInstantUiFlag(true);
          setOpenMenuKey(null);
          setSettingsOpen(false);
          if (tourOpen) closeTour();
          setErrorBanner(null);
          window.requestAnimationFrame(() => setInstantUiFlag(false));
        }
        return;
      }

      if (e.ctrlKey || e.metaKey || e.altKey || isEditableTarget(e.target)) {
        return;
      }

      const lower = e.key.toLowerCase();
      if (lower === "n") {
        e.preventDefault();
        void handleAddRoute({ instant: true });
      } else if (lower === "m") {
        if (!snapshot || snapshot.routes.length === 0) return;
        e.preventDefault();
        const targetRoute =
          snapshot.routes.find((r) => r.id === selectedRouteId) ?? snapshot.routes[0];
        if (targetRoute) {
          handleGainMuteChange(
            targetRoute.id,
            targetRoute.gainDb,
            !targetRoute.muted,
            true,
          );
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    snapshot,
    selectedRouteId,
    openMenuKey,
    settingsOpen,
    tourOpen,
    closeTour,
    errorBanner,
    handleAddRoute,
    handleGainMuteChange,
  ]);

  const devices = snapshot?.devices ?? [];
  const routes = snapshot?.routes ?? [];
  const profiles = snapshot?.profiles ?? [];

  const canAddRoute = useMemo(() => {
    const hasIn = devices.some((d) => d.flow === "capture" || d.flow === "loopback");
    const hasOut = devices.some((d) => d.flow === "render");
    return hasIn && hasOut;
  }, [devices]);

  const aggregateLatency = useMemo(
    () => computeAggregateSoftwareLatency(routes, summaryTelemetryMap),
    [routes, summaryTelemetryMap],
  );

  const inputCount = devices.filter(
    (d) => d.flow === "capture" || d.flow === "loopback",
  ).length;
  const outputCount = devices.filter((d) => d.flow === "render").length;

  return (
    <div
      data-instant={instantUiFlag ? "true" : undefined}
      className="min-h-full flex flex-col"
      style={{ backgroundColor: "var(--bg-app)", color: "var(--text-primary)" }}
    >
      <HeaderBar
        profiles={profiles}
        activeProfileId={snapshot?.settings.activeProfileId ?? null}
        routeCount={routes.length}
        maxSoftwareMs={aggregateLatency.maxSoftwareMs}
        activeCount={aggregateLatency.activeCount}
        hasWarning={aggregateLatency.hasWarning}
        theme={theme}
        canAddRoute={canAddRoute}
        openMenuKey={openMenuKey}
        onSetOpenMenu={setOpenMenuKey}
        onLoadProfile={handleLoadProfile}
        onSaveProfile={handleSaveProfile}
        onDeleteProfile={handleDeleteProfile}
        onAddRoute={() => void handleAddRoute()}
        onToggleTheme={handleToggleTheme}
        onOpenSettings={() => {
          setOpenMenuKey(null);
          setSettingsOpen(true);
        }}
      />

      <main className="flex-1 w-full max-w-[1120px] mx-auto px-4 sm:px-5 py-4 flex flex-col gap-3.5">
        {updateState && (
          <UpdateBanner
            state={updateState}
            dismissedVersion={dismissedUpdateVersion}
            dismissedError={dismissedUpdateError}
            onDismiss={() => {
              if (updateState.status === "error") {
                setDismissedUpdateError(updateState.errorMessage ?? "Update check failed");
              } else {
                setDismissedUpdateVersion(updateState.availableVersion);
              }
            }}
            onDownload={handleDownloadUpdate}
            onInstall={handleInstallUpdate}
            onCancel={handleCancelUpdate}
            onOpenRelease={handleOpenReleasePage}
          />
        )}

        {errorBanner && (
          <div
            role="alert"
            className="px-3.5 py-2.5 rounded-[8px] border flex items-center justify-between gap-3 text-[12px]"
            style={{
              backgroundColor: "var(--status-error-soft)",
              borderColor: "var(--status-error-border)",
              color: "var(--text-primary)",
            }}
          >
            <div className="flex items-center gap-2 min-w-0">
              <AlertCircle
                size={15}
                className="shrink-0"
                style={{ color: "var(--status-error)" }}
              />
              <span className="truncate">{errorBanner}</span>
            </div>
            <button
              type="button"
              aria-label="Dismiss error"
              onClick={() => setErrorBanner(null)}
              className="tactile-btn p-1 rounded-[4px] shrink-0"
              style={{ color: "var(--text-secondary)" }}
            >
              <X size={14} />
            </button>
          </div>
        )}

        {loading && !snapshot ? (
          <div
            className="rounded-[10px] border p-6 text-[13px] font-mono-num"
            style={{
              backgroundColor: "var(--bg-panel)",
              borderColor: "var(--border-subtle)",
              color: "var(--text-secondary)",
            }}
          >
            Initializing WASAPI audio matrix...
          </div>
        ) : routes.length === 0 ? (
          <EmptyState
            devices={devices}
            onQuickStart={() => void handleAddRoute()}
          />
        ) : (
          <div className="flex flex-col gap-3">
            {routes.map((route) => (
              <RouteCard
                key={route.id}
                route={route}
                devices={devices}
                summaryTelemetry={summaryTelemetryMap.get(route.id)}
                telemetryMapRef={telemetryMapRef}
                theme={theme}
                selected={route.id === selectedRouteId}
                instantCreate={instantRouteIds.has(route.id)}
                instantMute={instantMuteRouteId === route.id}
                openMenuKey={openMenuKey}
                onSetOpenMenu={setOpenMenuKey}
                onSelectRoute={() => setSelectedRouteId(route.id)}
                onUpdateRoute={(next) => void handleUpdateRoute(next)}
                onGainMuteChange={(id, g, m) => handleGainMuteChange(id, g, m, false)}
                onRemoveRoute={(id) => void handleRemoveRoute(id)}
              />
            ))}
          </div>
        )}
      </main>

      {/* Minimal Status Footer */}
      <footer
        className="border-t px-4 sm:px-5 py-2 flex items-center justify-between gap-2 text-[11px] font-mono-num"
        style={{
          backgroundColor: "var(--bg-app)",
          borderColor: "var(--border-subtle)",
          color: "var(--text-muted)",
        }}
      >
        <span>
          {inputCount} in · {outputCount} out
        </span>
        <span>
          {routes.filter((r) => r.enabled).length}/{routes.length} active
        </span>
      </footer>

      {snapshot && (
        <SettingsDrawer
          open={settingsOpen}
          version={snapshot.version}
          settings={snapshot.settings}
          updateState={updateState}
          onClose={() => setSettingsOpen(false)}
          onUpdateSettings={(next) => void handleUpdateSettings(next)}
          onReplayTour={() => setTourOpen(true)}
          onCheckUpdates={handleCheckUpdates}
          onCancelUpdate={handleCancelUpdate}
          onDownloadUpdate={handleDownloadUpdate}
          onInstallUpdate={handleInstallUpdate}
          onOpenReleasePage={handleOpenReleasePage}
        />
      )}

      <OnboardingTour open={tourOpen} onClose={closeTour} />
    </div>
  );
}

export default App;
