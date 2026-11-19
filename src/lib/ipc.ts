import { invoke } from "@tauri-apps/api/core";

export const EVENT_TELEMETRY = "uncord://telemetry";
export const EVENT_DEVICES_CHANGED = "uncord://devices-changed";
export const EVENT_ROUTE_STATUS = "uncord://route-status";
export const EVENT_UPDATE_STATE = "uncord://update-state";

export type UpdateStatusKind =
  | "ready"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "verified"
  | "installing"
  | "error";

export interface UpdateStateSnapshot {
  status: UpdateStatusKind;
  currentVersion: string;
  availableVersion: string | null;
  releaseUrl: string | null;
  releaseNotes: string | null;
  downloadedBytes: number;
  totalBytes: number;
  verifying: boolean;
  errorMessage: string | null;
}

export type DeviceFlow = "capture" | "render" | "loopback";

export type TransportKind =
  | "usb"
  | "bluetoothA2dp"
  | "bluetoothHfp"
  | "wireless24Ghz"
  | "wired"
  | "virtualCable"
  | "unknown";

export interface AudioDeviceInfo {
  id: string;
  name: string;
  flow: DeviceFlow;
  channels: number;
  sampleRate: number;
  minPeriodFrames: number;
  defaultPeriodFrames: number;
  supportsRawMode: boolean;
  transport: TransportKind;
  isDefault: boolean;
  isBluetoothHfpWarning: boolean;
}

export type ChannelMode =
  | { kind: "stereoPair"; left: number; right: number }
  | { kind: "monoToStereo"; channel: number }
  | { kind: "mixAllToStereo" };

export type LatencyMode = "ultraLow" | "low" | "balanced" | "safe";

export interface RouteConfig {
  id: string;
  name: string;
  sourceDeviceId: string;
  sourceIsLoopback: boolean;
  channelMode: ChannelMode;
  targetDeviceId: string;
  gainDb: number;
  muted: boolean;
  enabled: boolean;
  exclusiveOutput: boolean;
  latencyMode: LatencyMode;
}

export type RouteStatus =
  | { state: "stopped" }
  | { state: "running" }
  | { state: "reconnecting" }
  | { state: "error"; message: string };

export interface LatencyBreakdown {
  capturePeriodMs: number;
  ringBufferMs: number;
  renderPeriodMs: number;
  softwareTotalMs: number;
  estimatedTransportFloorMs: number;
  rawModeActive: boolean;
  exclusiveActive: boolean;
}

export interface RouteTelemetry {
  routeId: string;
  status: RouteStatus;
  peakLeft: number;
  peakRight: number;
  rmsLeft: number;
  rmsRight: number;
  clipping: boolean;
  bufferFillRatio: number;
  driftPpm: number;
  xruns: number;
  latency: LatencyBreakdown;
}

export interface Profile {
  id: string;
  name: string;
  routes: RouteConfig[];
}

export interface AppSettings {
  minimizeToTrayOnClose: boolean;
  startMinimized: boolean;
  autoReconnect: boolean;
  activeProfileId: string | null;
  theme: string;
  updateChecksEnabled: boolean;
}

export interface EngineSnapshot {
  version: string;
  devices: AudioDeviceInfo[];
  routes: RouteConfig[];
  profiles: Profile[];
  settings: AppSettings;
}

export type DeviceChangeEvent =
  | { kind: "added"; deviceId: string }
  | { kind: "removed"; deviceId: string }
  | { kind: "stateChanged"; deviceId: string }
  | { kind: "defaultChanged" };

export interface AddRouteRequest {
  name: string;
  sourceDeviceId: string;
  sourceIsLoopback: boolean;
  channelMode: ChannelMode;
  targetDeviceId: string;
  gainDb: number;
  exclusiveOutput: boolean;
  latencyMode: LatencyMode;
}

export const uncordIpc = {
  getSnapshot: (): Promise<EngineSnapshot> => invoke("get_snapshot"),
  listDevices: (): Promise<AudioDeviceInfo[]> => invoke("list_devices"),
  addRoute: (req: AddRouteRequest): Promise<RouteConfig> => invoke("add_route", { req }),
  updateRoute: (route: RouteConfig): Promise<RouteConfig> => invoke("update_route", { route }),
  removeRoute: (routeId: string): Promise<void> => invoke("remove_route", { routeId }),
  setRouteGainMute: (routeId: string, gainDb: number, muted: boolean): Promise<void> =>
    invoke("set_route_gain_mute", { routeId, gainDb, muted }),
  listProfiles: (): Promise<Profile[]> => invoke("list_profiles"),
  saveProfile: (name: string): Promise<Profile> => invoke("save_profile", { name }),
  loadProfile: (profileId: string): Promise<EngineSnapshot> =>
    invoke("load_profile", { profileId }),
  deleteProfile: (profileId: string): Promise<void> => invoke("delete_profile", { profileId }),
  updateSettings: (settings: AppSettings): Promise<AppSettings> =>
    invoke("update_settings", { settings }),
  getTelemetry: (): Promise<RouteTelemetry[]> => invoke("get_telemetry"),
  getUpdateState: (): Promise<UpdateStateSnapshot> => invoke("get_update_state"),
  checkForUpdates: (): Promise<UpdateStateSnapshot> => invoke("check_for_updates"),
  cancelUpdateCheck: (): Promise<UpdateStateSnapshot> => invoke("cancel_update_check"),
  downloadUpdate: (): Promise<UpdateStateSnapshot> => invoke("download_update"),
  installUpdate: (): Promise<void> => invoke("install_update"),
  openReleasePage: (): Promise<void> => invoke("open_release_page"),
};
