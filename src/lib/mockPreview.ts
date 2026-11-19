import type {
  AddRouteRequest,
  AppSettings,
  AudioDeviceInfo,
  EngineSnapshot,
  LatencyBreakdown,
  LatencyMode,
  Profile,
  RouteConfig,
  RouteTelemetry,
  TransportKind,
} from "./ipc";

const MOCK_DEVICES: AudioDeviceInfo[] = [
  {
    id: "mock-cap-blackstar",
    name: "Blackstar ID:Core V4 (USB Audio)",
    flow: "capture",
    channels: 4,
    sampleRate: 48000,
    minPeriodFrames: 144,
    defaultPeriodFrames: 480,
    supportsRawMode: true,
    transport: "usb",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-cap-shure",
    name: "Shure MV7 (USB Microphone)",
    flow: "capture",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 128,
    defaultPeriodFrames: 480,
    supportsRawMode: true,
    transport: "usb",
    isDefault: true,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-cap-linein",
    name: "Line In (Realtek Audio)",
    flow: "capture",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 160,
    defaultPeriodFrames: 480,
    supportsRawMode: false,
    transport: "wired",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-loop-desktop",
    name: "Desktop Audio — Speakers (Loopback)",
    flow: "loopback",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 160,
    defaultPeriodFrames: 480,
    supportsRawMode: false,
    transport: "wired",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-loop-virtual",
    name: "CABLE Output — Backing Track (Loopback)",
    flow: "loopback",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 240,
    defaultPeriodFrames: 480,
    supportsRawMode: false,
    transport: "virtualCable",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-ren-xm5-a2dp",
    name: "WH-1000XM5 Stereo (Bluetooth)",
    flow: "render",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 160,
    defaultPeriodFrames: 480,
    supportsRawMode: false,
    transport: "bluetoothA2dp",
    isDefault: true,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-ren-xm5-hfp",
    name: "WH-1000XM5 Hands-Free AG Audio",
    flow: "render",
    channels: 1,
    sampleRate: 16000,
    minPeriodFrames: 160,
    defaultPeriodFrames: 320,
    supportsRawMode: false,
    transport: "bluetoothHfp",
    isDefault: false,
    isBluetoothHfpWarning: true,
  },
  {
    id: "mock-ren-nova-24g",
    name: "Arctis Nova Pro (2.4 GHz Wireless)",
    flow: "render",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 144,
    defaultPeriodFrames: 480,
    supportsRawMode: true,
    transport: "wireless24Ghz",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-ren-monitors",
    name: "Studio Monitors — Focusrite Scarlett",
    flow: "render",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 128,
    defaultPeriodFrames: 480,
    supportsRawMode: true,
    transport: "wired",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
  {
    id: "mock-ren-vbcable",
    name: "CABLE Input (VB-Audio Virtual Cable)",
    flow: "render",
    channels: 2,
    sampleRate: 48000,
    minPeriodFrames: 240,
    defaultPeriodFrames: 480,
    supportsRawMode: false,
    transport: "virtualCable",
    isDefault: false,
    isBluetoothHfpWarning: false,
  },
];

const DEFAULT_ROUTES: RouteConfig[] = [
  {
    id: "route-amp-a2dp",
    name: "Blackstar Amp → WH-1000XM5",
    sourceDeviceId: "mock-cap-blackstar",
    sourceIsLoopback: false,
    channelMode: { kind: "stereoPair", left: 0, right: 1 },
    targetDeviceId: "mock-ren-xm5-a2dp",
    gainDb: 0.0,
    muted: false,
    enabled: true,
    exclusiveOutput: false,
    latencyMode: "ultraLow",
  },
  {
    id: "route-backing-a2dp",
    name: "Desktop Backing → WH-1000XM5",
    sourceDeviceId: "mock-loop-desktop",
    sourceIsLoopback: true,
    channelMode: { kind: "stereoPair", left: 0, right: 1 },
    targetDeviceId: "mock-ren-xm5-a2dp",
    gainDb: -4.5,
    muted: false,
    enabled: true,
    exclusiveOutput: false,
    latencyMode: "low",
  },
];

const DEFAULT_PROFILES: Profile[] = [
  {
    id: "prof-guitar-bt",
    name: "Late-Night Amp + Backing",
    routes: DEFAULT_ROUTES.map((r) => ({ ...r })),
  },
  {
    id: "prof-reamp-monitors",
    name: "Dry DI → Studio Monitors",
    routes: [
      {
        id: "route-di-monitors",
        name: "Blackstar Ch 3–4 DI → Monitors",
        sourceDeviceId: "mock-cap-blackstar",
        sourceIsLoopback: false,
        channelMode: { kind: "stereoPair", left: 2, right: 3 },
        targetDeviceId: "mock-ren-monitors",
        gainDb: 0.0,
        muted: false,
        enabled: true,
        exclusiveOutput: true,
        latencyMode: "ultraLow",
      },
    ],
  },
];

function transportFloorMs(transport: TransportKind): number {
  switch (transport) {
    case "bluetoothA2dp":
      return 140.0;
    case "bluetoothHfp":
      return 96.0;
    case "wireless24Ghz":
      return 14.0;
    case "wired":
      return 4.0;
    case "usb":
      return 5.0;
    case "virtualCable":
      return 2.0;
    default:
      return 10.0;
  }
}

function ringTargetMs(mode: LatencyMode): number {
  switch (mode) {
    case "ultraLow":
      return 2.4;
    case "low":
      return 3.6;
    case "balanced":
      return 9.5;
    case "safe":
      return 19.0;
  }
}

function computeMockLatency(
  route: RouteConfig,
  source: AudioDeviceInfo | undefined,
  target: AudioDeviceInfo | undefined,
): LatencyBreakdown {
  const srcRate = source?.sampleRate ?? 48000;
  const dstRate = target?.sampleRate ?? 48000;
  const srcFrames =
    route.latencyMode === "ultraLow"
      ? (source?.minPeriodFrames ?? 144)
      : route.latencyMode === "low"
        ? Math.min(192, source?.defaultPeriodFrames ?? 192)
        : (source?.defaultPeriodFrames ?? 480);
  const dstFrames =
    route.latencyMode === "ultraLow"
      ? (target?.minPeriodFrames ?? 144)
      : route.latencyMode === "low"
        ? Math.min(192, target?.defaultPeriodFrames ?? 192)
        : (target?.defaultPeriodFrames ?? 480);

  const capturePeriodMs = Math.min(10, (srcFrames / srcRate) * 1000);
  const renderPeriodMs = Math.min(10, (dstFrames / dstRate) * 1000);
  const ringBufferMs = ringTargetMs(route.latencyMode);
  const softwareTotalMs = capturePeriodMs + ringBufferMs + renderPeriodMs;

  return {
    capturePeriodMs,
    ringBufferMs,
    renderPeriodMs,
    softwareTotalMs,
    estimatedTransportFloorMs: transportFloorMs(target?.transport ?? "unknown"),
    rawModeActive: Boolean(source?.supportsRawMode),
    exclusiveActive: route.exclusiveOutput,
  };
}

class MockPreviewEngine {
  private devices: AudioDeviceInfo[] = MOCK_DEVICES.map((d) => ({ ...d }));
  private routes: RouteConfig[] = DEFAULT_ROUTES.map((r) => ({ ...r }));
  private profiles: Profile[] = DEFAULT_PROFILES.map((p) => ({
    ...p,
    routes: p.routes.map((r) => ({ ...r })),
  }));
  private settings: AppSettings = {
    minimizeToTrayOnClose: true,
    startMinimized: false,
    autoReconnect: true,
    activeProfileId: "prof-guitar-bt",
    theme: "dark",
  };
  private nextId = 1;
  private startTs = performance.now();

  private snapshot(): EngineSnapshot {
    return {
      version: "0.1.0",
      devices: this.devices.map((d) => ({ ...d })),
      routes: this.routes.map((r) => ({ ...r })),
      profiles: this.profiles.map((p) => ({
        ...p,
        routes: p.routes.map((r) => ({ ...r })),
      })),
      settings: { ...this.settings },
    };
  }

  async getSnapshot(): Promise<EngineSnapshot> {
    return this.snapshot();
  }

  async listDevices(): Promise<AudioDeviceInfo[]> {
    return this.devices.map((d) => ({ ...d }));
  }

  async addRoute(req: AddRouteRequest): Promise<RouteConfig> {
    const id = `route-preview-${this.nextId++}`;
    const created: RouteConfig = {
      id,
      name: req.name,
      sourceDeviceId: req.sourceDeviceId,
      sourceIsLoopback: req.sourceIsLoopback,
      channelMode: req.channelMode,
      targetDeviceId: req.targetDeviceId,
      gainDb: req.gainDb,
      muted: false,
      enabled: true,
      exclusiveOutput: req.exclusiveOutput,
      latencyMode: req.latencyMode,
    };
    this.routes = [...this.routes, created];
    return { ...created };
  }

  async updateRoute(route: RouteConfig): Promise<RouteConfig> {
    const idx = this.routes.findIndex((r) => r.id === route.id);
    if (idx === -1) {
      throw new Error(`Route not found: ${route.id}`);
    }
    const updated = { ...route };
    this.routes[idx] = updated;
    return { ...updated };
  }

  async removeRoute(routeId: string): Promise<void> {
    this.routes = this.routes.filter((r) => r.id !== routeId);
  }

  async setRouteGainMute(routeId: string, gainDb: number, muted: boolean): Promise<void> {
    const route = this.routes.find((r) => r.id === routeId);
    if (route) {
      route.gainDb = gainDb;
      route.muted = muted;
    }
  }

  async listProfiles(): Promise<Profile[]> {
    return this.profiles.map((p) => ({
      ...p,
      routes: p.routes.map((r) => ({ ...r })),
    }));
  }

  async saveProfile(name: string): Promise<Profile> {
    const trimmed = name.trim() || `Preset ${this.profiles.length + 1}`;
    const created: Profile = {
      id: `prof-${Date.now().toString(36)}`,
      name: trimmed,
      routes: this.routes.map((r) => ({ ...r })),
    };
    this.profiles = [...this.profiles, created];
    this.settings.activeProfileId = created.id;
    return {
      ...created,
      routes: created.routes.map((r) => ({ ...r })),
    };
  }

  async loadProfile(profileId: string): Promise<EngineSnapshot> {
    const profile = this.profiles.find((p) => p.id === profileId);
    if (!profile) {
      throw new Error(`Profile not found: ${profileId}`);
    }
    this.routes = profile.routes.map((r) => ({ ...r }));
    this.settings.activeProfileId = profile.id;
    return this.snapshot();
  }

  async deleteProfile(profileId: string): Promise<void> {
    this.profiles = this.profiles.filter((p) => p.id !== profileId);
    if (this.settings.activeProfileId === profileId) {
      this.settings.activeProfileId = null;
    }
  }

  async updateSettings(settings: AppSettings): Promise<AppSettings> {
    this.settings = { ...settings };
    return { ...this.settings };
  }

  getTelemetrySync(): RouteTelemetry[] {
    const t = (performance.now() - this.startTs) / 1000;
    return this.routes.map((route, idx) => {
      const source = this.devices.find((d) => d.id === route.sourceDeviceId);
      const target = this.devices.find((d) => d.id === route.targetDeviceId);
      const latency = computeMockLatency(route, source, target);

      if (!route.enabled) {
        return {
          routeId: route.id,
          status: { state: "stopped" },
          peakLeft: 0,
          peakRight: 0,
          rmsLeft: 0,
          rmsRight: 0,
          clipping: false,
          bufferFillRatio: 0,
          driftPpm: 0,
          xruns: 0,
          latency,
        };
      }

      if (route.muted || route.gainDb <= -59.5) {
        return {
          routeId: route.id,
          status: { state: "running" },
          peakLeft: 0,
          peakRight: 0,
          rmsLeft: 0,
          rmsRight: 0,
          clipping: false,
          bufferFillRatio: 0.49 + Math.sin(t * 1.4 + idx) * 0.02,
          driftPpm: idx % 2 === 0 ? 14.2 : -6.4,
          xruns: 0,
          latency,
        };
      }

      const gainLin = Math.pow(10, route.gainDb / 20);
      const phase = t * (2.6 + idx * 0.7) + idx * 1.7;
      const strum = Math.pow(0.5 + 0.5 * Math.sin(phase * 1.9), 2.1);
      const pulse = 0.22 + 0.38 * strum + 0.12 * Math.sin(phase * 4.3);
      const stereoBias = route.channelMode.kind === "monoToStereo" ? 0 : Math.sin(phase * 1.1) * 0.05;

      const rawPeakL = Math.max(0.02, pulse + stereoBias) * gainLin;
      const rawPeakR = Math.max(0.02, pulse - stereoBias) * gainLin;
      const clipping = rawPeakL > 0.995 || rawPeakR > 0.995;

      const peakLeft = Math.min(1.0, rawPeakL);
      const peakRight = Math.min(1.0, rawPeakR);
      const rmsLeft = peakLeft * 0.62;
      const rmsRight = peakRight * 0.61;

      const driftBase =
        target?.transport === "bluetoothA2dp"
          ? 18.4
          : target?.transport === "bluetoothHfp"
            ? -42.0
            : 4.8;
      const driftPpm = Number((driftBase + Math.sin(t * 0.6 + idx) * 2.1).toFixed(1));

      return {
        routeId: route.id,
        status: { state: "running" },
        peakLeft,
        peakRight,
        rmsLeft,
        rmsRight,
        clipping,
        bufferFillRatio: 0.5 + Math.sin(t * 1.8 + idx) * 0.03,
        driftPpm,
        xruns: 0,
        latency,
      };
    });
  }

  async getTelemetry(): Promise<RouteTelemetry[]> {
    return this.getTelemetrySync();
  }
}

export const mockPreviewEngine = new MockPreviewEngine();
