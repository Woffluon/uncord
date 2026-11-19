import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EVENT_DEVICES_CHANGED,
  EVENT_ROUTE_STATUS,
  EVENT_TELEMETRY,
  EVENT_UPDATE_STATE,
  uncordIpc,
  type AddRouteRequest,
  type AppSettings,
  type AudioDeviceInfo,
  type EngineSnapshot,
  type Profile,
  type RouteConfig,
  type RouteTelemetry,
  type UpdateStateSnapshot,
} from "./ipc";
import { mockPreviewEngine } from "./mockPreview";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && typeof window.__TAURI_INTERNALS__ !== "undefined";
}

export const audioClient = {
  getSnapshot: (): Promise<EngineSnapshot> =>
    isTauriRuntime() ? uncordIpc.getSnapshot() : mockPreviewEngine.getSnapshot(),

  listDevices: (): Promise<AudioDeviceInfo[]> =>
    isTauriRuntime() ? uncordIpc.listDevices() : mockPreviewEngine.listDevices(),

  addRoute: (req: AddRouteRequest): Promise<RouteConfig> =>
    isTauriRuntime() ? uncordIpc.addRoute(req) : mockPreviewEngine.addRoute(req),

  updateRoute: (route: RouteConfig): Promise<RouteConfig> =>
    isTauriRuntime() ? uncordIpc.updateRoute(route) : mockPreviewEngine.updateRoute(route),

  removeRoute: (routeId: string): Promise<void> =>
    isTauriRuntime() ? uncordIpc.removeRoute(routeId) : mockPreviewEngine.removeRoute(routeId),

  setRouteGainMute: (routeId: string, gainDb: number, muted: boolean): Promise<void> =>
    isTauriRuntime()
      ? uncordIpc.setRouteGainMute(routeId, gainDb, muted)
      : mockPreviewEngine.setRouteGainMute(routeId, gainDb, muted),

  listProfiles: (): Promise<Profile[]> =>
    isTauriRuntime() ? uncordIpc.listProfiles() : mockPreviewEngine.listProfiles(),

  saveProfile: (name: string): Promise<Profile> =>
    isTauriRuntime() ? uncordIpc.saveProfile(name) : mockPreviewEngine.saveProfile(name),

  loadProfile: (profileId: string): Promise<EngineSnapshot> =>
    isTauriRuntime() ? uncordIpc.loadProfile(profileId) : mockPreviewEngine.loadProfile(profileId),

  deleteProfile: (profileId: string): Promise<void> =>
    isTauriRuntime() ? uncordIpc.deleteProfile(profileId) : mockPreviewEngine.deleteProfile(profileId),

  updateSettings: (settings: AppSettings): Promise<AppSettings> =>
    isTauriRuntime()
      ? uncordIpc.updateSettings(settings)
      : mockPreviewEngine.updateSettings(settings),

  getTelemetry: (): Promise<RouteTelemetry[]> =>
    isTauriRuntime() ? uncordIpc.getTelemetry() : mockPreviewEngine.getTelemetry(),

  getUpdateState: (): Promise<UpdateStateSnapshot> =>
    isTauriRuntime() ? uncordIpc.getUpdateState() : mockPreviewEngine.getUpdateState(),

  checkForUpdates: (): Promise<UpdateStateSnapshot> =>
    isTauriRuntime() ? uncordIpc.checkForUpdates() : mockPreviewEngine.checkForUpdates(),

  cancelUpdateCheck: (): Promise<UpdateStateSnapshot> =>
    isTauriRuntime() ? uncordIpc.cancelUpdateCheck() : mockPreviewEngine.cancelUpdateCheck(),

  downloadUpdate: (): Promise<UpdateStateSnapshot> =>
    isTauriRuntime() ? uncordIpc.downloadUpdate() : mockPreviewEngine.downloadUpdate(),

  installUpdate: (): Promise<void> =>
    isTauriRuntime() ? uncordIpc.installUpdate() : mockPreviewEngine.installUpdate(),

  openReleasePage: (): Promise<void> =>
    isTauriRuntime() ? uncordIpc.openReleasePage() : mockPreviewEngine.openReleasePage(),
};

export function subscribeTelemetry(
  onTelemetry: (items: RouteTelemetry[]) => void,
): () => void {
  if (!isTauriRuntime()) {
    onTelemetry(mockPreviewEngine.getTelemetrySync());
    const id = window.setInterval(() => {
      onTelemetry(mockPreviewEngine.getTelemetrySync());
    }, 33);
    return () => window.clearInterval(id);
  }

  let disposed = false;
  let unlisten: UnlistenFn | null = null;

  void listen<RouteTelemetry[]>(EVENT_TELEMETRY, (event) => {
    if (!disposed && Array.isArray(event.payload)) {
      onTelemetry(event.payload);
    }
  }).then((fn) => {
    if (disposed) {
      fn();
    } else {
      unlisten = fn;
    }
  });

  return () => {
    disposed = true;
    if (unlisten) unlisten();
  };
}

export function subscribeDeviceChanges(
  onDevicesChanged: (devices: AudioDeviceInfo[] | null) => void,
): () => void {
  if (!isTauriRuntime()) {
    return () => {};
  }

  let disposed = false;
  let unlistenDevices: UnlistenFn | null = null;
  let unlistenStatus: UnlistenFn | null = null;

  void listen<unknown>(EVENT_DEVICES_CHANGED, (event) => {
    if (disposed) return;
    if (Array.isArray(event.payload)) {
      onDevicesChanged(event.payload as AudioDeviceInfo[]);
    } else {
      onDevicesChanged(null);
    }
  }).then((fn) => {
    if (disposed) {
      fn();
    } else {
      unlistenDevices = fn;
    }
  });

  void listen<unknown>(EVENT_ROUTE_STATUS, () => {
    if (!disposed) {
      onDevicesChanged(null);
    }
  }).then((fn) => {
    if (disposed) {
      fn();
    } else {
      unlistenStatus = fn;
    }
  });

  return () => {
    disposed = true;
    if (unlistenDevices) unlistenDevices();
    if (unlistenStatus) unlistenStatus();
  };
}

export function subscribeUpdateState(
  onState: (state: UpdateStateSnapshot) => void,
): () => void {
  if (!isTauriRuntime()) {
    return mockPreviewEngine.subscribeUpdateState(onState);
  }

  let disposed = false;
  let unlisten: UnlistenFn | null = null;

  void listen<UpdateStateSnapshot>(EVENT_UPDATE_STATE, (event) => {
    if (!disposed && event.payload) {
      onState(event.payload);
    }
  }).then((fn) => {
    if (disposed) {
      fn();
    } else {
      unlisten = fn;
    }
  });

  return () => {
    disposed = true;
    if (unlisten) unlisten();
  };
}
