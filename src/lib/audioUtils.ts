import type {
  AudioDeviceInfo,
  ChannelMode,
  LatencyMode,
  RouteConfig,
  RouteTelemetry,
} from "./ipc";

export interface ChannelModeOption {
  id: string;
  label: string;
  shortLabel: string;
  mode: ChannelMode;
}

export function formatSampleRate(hz: number): string {
  if (!Number.isFinite(hz) || hz <= 0) return "-- kHz";
  const khz = hz / 1000;
  return Number.isInteger(khz) ? `${khz} kHz` : `${khz.toFixed(1)} kHz`;
}

export function formatGainDb(gainDb: number): string {
  if (gainDb <= -59.5) return "-∞ dB";
  const sign = gainDb > 0.04 ? "+" : "";
  return `${sign}${gainDb.toFixed(1)} dB`;
}

export function formatMs(ms: number | undefined | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return "-- ms";
  return `${ms.toFixed(1)} ms`;
}

export function getInputBadge(device: AudioDeviceInfo): {
  label: string;
  tone: "signal" | "loopback" | "neutral";
} {
  if (device.flow === "loopback") {
    return { label: "LOOPBACK", tone: "loopback" };
  }
  switch (device.transport) {
    case "usb":
      return { label: "USB", tone: "signal" };
    case "virtualCable":
      return { label: "VIRTUAL", tone: "neutral" };
    default:
      return { label: "MIC", tone: "neutral" };
  }
}

export function getOutputBadge(device: AudioDeviceInfo): {
  label: string;
  tone: "healthy" | "warning" | "neutral" | "signal";
} {
  if (device.isBluetoothHfpWarning || device.transport === "bluetoothHfp") {
    return { label: "BT HFP ⚠", tone: "warning" };
  }
  switch (device.transport) {
    case "bluetoothA2dp":
      return { label: "BT A2DP", tone: "signal" };
    case "wireless24Ghz":
      return { label: "2.4 GHz", tone: "healthy" };
    case "wired":
      return { label: "WIRED", tone: "neutral" };
    case "virtualCable":
      return { label: "VIRTUAL", tone: "neutral" };
    case "usb":
      return { label: "USB", tone: "neutral" };
    default:
      return { label: "OUTPUT", tone: "neutral" };
  }
}

export function channelModeKey(mode: ChannelMode): string {
  switch (mode.kind) {
    case "stereoPair":
      return `stereo:${mode.left}-${mode.right}`;
    case "monoToStereo":
      return `mono:${mode.channel}`;
    case "mixAllToStereo":
      return "mixAll";
  }
}

export function channelModeEquals(a: ChannelMode, b: ChannelMode): boolean {
  return channelModeKey(a) === channelModeKey(b);
}

export function getChannelModeOptions(channels: number): ChannelModeOption[] {
  const chCount = Math.max(1, channels);
  if (chCount >= 4) {
    const options: ChannelModeOption[] = [
      {
        id: "stereo:0-1",
        label: "Ch 1–2 Stereo (Amp Tone)",
        shortLabel: "Ch 1–2 Stereo",
        mode: { kind: "stereoPair", left: 0, right: 1 },
      },
      {
        id: "stereo:2-3",
        label: "Ch 3–4 (Reamp / DI)",
        shortLabel: "Ch 3–4 DI",
        mode: { kind: "stereoPair", left: 2, right: 3 },
      },
    ];

    for (let ch = 4; ch + 1 < chCount; ch += 2) {
      options.push({
        id: `stereo:${ch}-${ch + 1}`,
        label: `Ch ${ch + 1}–${ch + 2} Stereo`,
        shortLabel: `Ch ${ch + 1}–${ch + 2}`,
        mode: { kind: "stereoPair", left: ch, right: ch + 1 },
      });
    }

    options.push(
      {
        id: "mono:0",
        label: "Ch 1 Mono → Stereo",
        shortLabel: "Ch 1 Mono",
        mode: { kind: "monoToStereo", channel: 0 },
      },
      {
        id: "mono:3",
        label: "Ch 4 Dry DI → Stereo",
        shortLabel: "Ch 4 Dry DI",
        mode: { kind: "monoToStereo", channel: 3 },
      },
      {
        id: "mixAll",
        label: "All Mix → Stereo",
        shortLabel: "All Mix",
        mode: { kind: "mixAllToStereo" },
      },
    );

    return options;
  }

  if (chCount === 3) {
    return [
      {
        id: "stereo:0-1",
        label: "Ch 1–2 Stereo",
        shortLabel: "Ch 1–2 Stereo",
        mode: { kind: "stereoPair", left: 0, right: 1 },
      },
      {
        id: "mono:0",
        label: "Ch 1 Mono → Stereo",
        shortLabel: "Ch 1 Mono",
        mode: { kind: "monoToStereo", channel: 0 },
      },
      {
        id: "mono:2",
        label: "Ch 3 Mono → Stereo",
        shortLabel: "Ch 3 Mono",
        mode: { kind: "monoToStereo", channel: 2 },
      },
      {
        id: "mixAll",
        label: "All Mix → Stereo",
        shortLabel: "All Mix",
        mode: { kind: "mixAllToStereo" },
      },
    ];
  }

  if (chCount === 2) {
    return [
      {
        id: "stereo:0-1",
        label: "Ch 1–2 Stereo",
        shortLabel: "Ch 1–2 Stereo",
        mode: { kind: "stereoPair", left: 0, right: 1 },
      },
      {
        id: "mono:0",
        label: "Ch 1 Mono → Stereo",
        shortLabel: "Ch 1 Mono",
        mode: { kind: "monoToStereo", channel: 0 },
      },
      {
        id: "mono:1",
        label: "Ch 2 Mono → Stereo",
        shortLabel: "Ch 2 Mono",
        mode: { kind: "monoToStereo", channel: 1 },
      },
      {
        id: "mixAll",
        label: "All Mix → Stereo",
        shortLabel: "All Mix",
        mode: { kind: "mixAllToStereo" },
      },
    ];
  }

  return [
    {
      id: "mono:0",
      label: "Ch 1 Mono → Stereo",
      shortLabel: "Ch 1 Mono",
      mode: { kind: "monoToStereo", channel: 0 },
    },
  ];
}

export function describeChannelMode(mode: ChannelMode, deviceChannels: number): string {
  const options = getChannelModeOptions(deviceChannels);
  const found = options.find((opt) => channelModeEquals(opt.mode, mode));
  if (found) return found.label;
  switch (mode.kind) {
    case "stereoPair":
      return `Ch ${mode.left + 1}–${mode.right + 1} Stereo`;
    case "monoToStereo":
      return `Ch ${mode.channel + 1} Mono → Stereo`;
    case "mixAllToStereo":
      return "All Mix → Stereo";
  }
}

export function clampChannelModeForDevice(mode: ChannelMode, channels: number): ChannelMode {
  if (channels <= 1) {
    return { kind: "monoToStereo", channel: 0 };
  }
  switch (mode.kind) {
    case "stereoPair":
      if (mode.left < channels && mode.right < channels) return mode;
      return { kind: "stereoPair", left: 0, right: 1 };
    case "monoToStereo":
      if (mode.channel < channels) return mode;
      return { kind: "monoToStereo", channel: 0 };
    case "mixAllToStereo":
      return mode;
  }
}

export function shortDeviceName(name: string): string {
  return name.replace(/\s*\([^)]*\)\s*/g, " ").trim().replace(/\s+/g, " ");
}

export function pickSmartRouteDefaults(
  devices: AudioDeviceInfo[],
  existingRoutes: RouteConfig[],
): {
  source: AudioDeviceInfo | null;
  target: AudioDeviceInfo | null;
  channelMode: ChannelMode;
  routeName: string;
} {
  const inputs = devices.filter((d) => d.flow === "capture" || d.flow === "loopback");
  const outputs = devices.filter((d) => d.flow === "render");

  const usedSourceIds = new Set(existingRoutes.map((r) => r.sourceDeviceId));

  const sortedInputs = [...inputs].sort((a, b) => {
    const aUnused = !usedSourceIds.has(a.id) ? 1 : 0;
    const bUnused = !usedSourceIds.has(b.id) ? 1 : 0;
    if (aUnused !== bUnused) return bUnused - aUnused;

    const aUsb = a.flow === "capture" && a.transport === "usb" ? 1 : 0;
    const bUsb = b.flow === "capture" && b.transport === "usb" ? 1 : 0;
    if (aUsb !== bUsb) return bUsb - aUsb;

    if (a.channels !== b.channels) return b.channels - a.channels;
    if (a.isDefault !== b.isDefault) return a.isDefault ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  const sortedOutputs = [...outputs].sort((a, b) => {
    const aSafe = !a.isBluetoothHfpWarning ? 1 : 0;
    const bSafe = !b.isBluetoothHfpWarning ? 1 : 0;
    if (aSafe !== bSafe) return bSafe - aSafe;

    const aBt = a.transport === "bluetoothA2dp" ? 1 : 0;
    const bBt = b.transport === "bluetoothA2dp" ? 1 : 0;
    if (aBt !== bBt) return bBt - aBt;

    if (a.isDefault !== b.isDefault) return a.isDefault ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  const source = sortedInputs[0] ?? null;
  const target = sortedOutputs[0] ?? null;
  const channelMode: ChannelMode =
    source && source.channels >= 2
      ? { kind: "stereoPair", left: 0, right: 1 }
      : { kind: "monoToStereo", channel: 0 };

  const routeName =
    source && target
      ? `${shortDeviceName(source.name)} → ${shortDeviceName(target.name)}`
      : `Route ${existingRoutes.length + 1}`;

  return { source, target, channelMode, routeName };
}

export function findMatchingA2dpDevice(
  hfpDevice: AudioDeviceInfo,
  devices: AudioDeviceInfo[],
): AudioDeviceInfo | null {
  const candidates = devices.filter(
    (d) => d.flow === "render" && !d.isBluetoothHfpWarning && d.transport === "bluetoothA2dp",
  );
  if (candidates.length === 0) return null;

  const hfpTokens = hfpDevice.name
    .toLowerCase()
    .replace(/hands-free|ag audio|headset|hfp/g, "")
    .split(/[^a-z0-9]+/)
    .filter((t) => t.length >= 2);

  let best: AudioDeviceInfo | null = null;
  let bestScore = 0;

  for (const candidate of candidates) {
    const lower = candidate.name.toLowerCase();
    let score = 0;
    for (const token of hfpTokens) {
      if (lower.includes(token)) score += 1;
    }
    if (score > bestScore) {
      bestScore = score;
      best = candidate;
    }
  }

  return best ?? candidates[0];
}

export const LATENCY_MODES: Array<{
  value: LatencyMode;
  label: string;
  targetHint: string;
}> = [
  { value: "ultraLow", label: "UltraLow", targetHint: "~4 ms" },
  { value: "low", label: "Low", targetHint: "~8 ms" },
  { value: "balanced", label: "Balanced", targetHint: "~16 ms" },
  { value: "safe", label: "Safe", targetHint: "~32 ms" },
];

export function computeAggregateSoftwareLatency(
  routes: RouteConfig[],
  telemetryMap: Map<string, RouteTelemetry>,
): {
  maxSoftwareMs: number | null;
  activeCount: number;
  hasWarning: boolean;
} {
  let maxSoftwareMs: number | null = null;
  let activeCount = 0;
  let hasWarning = false;

  for (const route of routes) {
    if (!route.enabled) continue;
    activeCount += 1;
    const t = telemetryMap.get(route.id);
    if (!t) continue;
    if (t.status.state === "running") {
      if (maxSoftwareMs === null || t.latency.softwareTotalMs > maxSoftwareMs) {
        maxSoftwareMs = t.latency.softwareTotalMs;
      }
      if (t.latency.softwareTotalMs > 12.0 || t.xruns > 0) {
        hasWarning = true;
      }
    } else if (t.status.state === "reconnecting" || t.status.state === "error") {
      hasWarning = true;
    }
  }

  return { maxSoftwareMs, activeCount, hasWarning };
}

export function linearToDb(linear: number): number {
  if (!Number.isFinite(linear) || linear <= 0.0001) return -72;
  return 20 * Math.log10(linear);
}
