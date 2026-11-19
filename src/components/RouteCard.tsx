import { useState, type KeyboardEvent, type MutableRefObject } from "react";
import { AlertTriangle, Check, Pencil, Trash2, Volume2, VolumeX } from "lucide-react";
import type {
  AudioDeviceInfo,
  ChannelMode,
  LatencyMode,
  RouteConfig,
  RouteTelemetry,
} from "../lib/ipc";
import {
  LATENCY_MODES,
  clampChannelModeForDevice,
  findMatchingA2dpDevice,
  shortDeviceName,
} from "../lib/audioUtils";
import { ChannelMapSelect } from "./ChannelMapSelect";
import { DeviceSelect } from "./DeviceSelect";
import { GainFader } from "./GainFader";
import { LatencyPopover } from "./LatencyPopover";
import { VuMeterCanvas } from "./VuMeterCanvas";

interface RouteCardProps {
  route: RouteConfig;
  devices: AudioDeviceInfo[];
  summaryTelemetry?: RouteTelemetry;
  telemetryMapRef: MutableRefObject<Map<string, RouteTelemetry>>;
  theme: string;
  selected: boolean;
  instantCreate: boolean;
  instantMute: boolean;
  openMenuKey: string | null;
  onSetOpenMenu: (key: string | null) => void;
  onSelectRoute: () => void;
  onUpdateRoute: (next: RouteConfig) => void;
  onGainMuteChange: (routeId: string, gainDb: number, muted: boolean) => void;
  onRemoveRoute: (routeId: string) => void;
}

export function RouteCard({
  route,
  devices,
  summaryTelemetry,
  telemetryMapRef,
  theme,
  selected,
  instantCreate,
  instantMute,
  openMenuKey,
  onSetOpenMenu,
  onSelectRoute,
  onUpdateRoute,
  onGainMuteChange,
  onRemoveRoute,
}: RouteCardProps) {
  const [editingName, setEditingName] = useState(false);
  const [draftName, setDraftName] = useState(route.name);

  const sourceDevice = devices.find((d) => d.id === route.sourceDeviceId);
  const targetDevice = devices.find((d) => d.id === route.targetDeviceId);

  const status = summaryTelemetry?.status ?? (route.enabled ? { state: "running" } : { state: "stopped" });
  const isHfpWarning = Boolean(
    targetDevice && (targetDevice.isBluetoothHfpWarning || targetDevice.transport === "bluetoothHfp"),
  );
  const a2dpCompanion =
    isHfpWarning && targetDevice ? findMatchingA2dpDevice(targetDevice, devices) : null;

  const statusDotColor = !route.enabled
    ? "var(--text-muted)"
    : status.state === "error"
      ? "var(--status-error)"
      : status.state === "reconnecting"
        ? "var(--status-warning)"
        : "var(--status-healthy)";

  const statusText = !route.enabled
    ? "BYPASSED"
    : status.state === "error"
      ? "ERROR"
      : status.state === "reconnecting"
        ? "RECONNECTING"
        : "ACTIVE";

  const srcMenuKey = `src:${route.id}`;
  const chMenuKey = `ch:${route.id}`;
  const dstMenuKey = `dst:${route.id}`;
  const latMenuKey = `lat:${route.id}`;

  const commitName = () => {
    const trimmed = draftName.trim();
    setEditingName(false);
    if (trimmed && trimmed !== route.name) {
      onUpdateRoute({ ...route, name: trimmed });
    } else {
      setDraftName(route.name);
    }
  };

  const handleNameKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commitName();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setDraftName(route.name);
      setEditingName(false);
    }
  };

  const handleSelectInput = (device: AudioDeviceInfo) => {
    const nextChannelMode = clampChannelModeForDevice(route.channelMode, device.channels);
    const autoName =
      targetDevice && route.name.includes("→")
        ? `${shortDeviceName(device.name)} → ${shortDeviceName(targetDevice.name)}`
        : route.name;

    onUpdateRoute({
      ...route,
      name: autoName,
      sourceDeviceId: device.id,
      sourceIsLoopback: device.flow === "loopback",
      channelMode: nextChannelMode,
    });
  };

  const handleSelectOutput = (device: AudioDeviceInfo) => {
    const autoName =
      sourceDevice && route.name.includes("→")
        ? `${shortDeviceName(sourceDevice.name)} → ${shortDeviceName(device.name)}`
        : route.name;

    onUpdateRoute({
      ...route,
      name: autoName,
      targetDeviceId: device.id,
    });
  };

  const handleSelectChannelMode = (channelMode: ChannelMode) => {
    onUpdateRoute({ ...route, channelMode });
  };

  const handleLatencyMode = (latencyMode: LatencyMode) => {
    if (route.latencyMode === latencyMode) return;
    onUpdateRoute({ ...route, latencyMode });
  };

  return (
    <article
      data-instant={instantCreate ? "true" : undefined}
      onPointerDown={onSelectRoute}
      className="route-card hover-card relative rounded-[10px] border p-3.5"
      style={{
        backgroundColor: "var(--bg-panel)",
        borderColor: selected ? "var(--accent-signal-border)" : "var(--border-subtle)",
        boxShadow: selected ? "inset 3px 0 0 var(--accent-signal)" : undefined,
      }}
    >
      {/* Top strip: Status, Name, Enable/Bypass, Delete */}
      <div className="flex items-center justify-between gap-3 pb-3 mb-3 border-b" style={{ borderColor: "var(--border-subtle)" }}>
        <div className="flex items-center gap-2.5 min-w-0 flex-1">
          <span
            className="shrink-0 w-2 h-2 rounded-full"
            style={{ backgroundColor: statusDotColor }}
            title={`Route status: ${statusText}`}
          />

          {editingName ? (
            <div className="flex items-center gap-1.5 min-w-0 flex-1 max-w-[340px]">
              <input
                type="text"
                autoFocus
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                onBlur={commitName}
                onKeyDown={handleNameKeyDown}
                className="w-full px-2 py-0.5 text-[13px] font-semibold rounded-[4px] border"
                style={{
                  backgroundColor: "var(--bg-well)",
                  borderColor: "var(--accent-signal)",
                  color: "var(--text-primary)",
                }}
              />
              <button
                type="button"
                onClick={commitName}
                aria-label="Save route name"
                className="tactile-btn p-1 rounded-[4px]"
                style={{ color: "var(--status-healthy)" }}
              >
                <Check size={14} />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-1.5 min-w-0">
              <h2
                onDoubleClick={() => {
                  setDraftName(route.name);
                  setEditingName(true);
                }}
                className="text-[13px] font-semibold tracking-[-0.01em] truncate"
                style={{ color: route.enabled ? "var(--text-primary)" : "var(--text-secondary)" }}
              >
                {route.name}
              </h2>
              <button
                type="button"
                aria-label="Rename route"
                onClick={() => {
                  setDraftName(route.name);
                  setEditingName(true);
                }}
                className="tactile-btn hover-surface p-1 rounded-[4px] opacity-65"
                style={{ color: "var(--text-secondary)" }}
              >
                <Pencil size={11} />
              </button>
            </div>
          )}

          <span
            className="font-mono-num shrink-0 text-[10px] font-semibold px-1.5 py-0.5 rounded-[3px] tracking-[0.06em]"
            style={{
              backgroundColor:
                status.state === "error"
                  ? "var(--status-error-soft)"
                  : status.state === "reconnecting"
                    ? "var(--status-warning-soft)"
                    : route.enabled
                      ? "var(--status-healthy-soft)"
                      : "var(--bg-well)",
              color:
                status.state === "error"
                  ? "var(--status-error)"
                  : status.state === "reconnecting"
                    ? "var(--status-warning)"
                    : route.enabled
                      ? "var(--status-healthy)"
                      : "var(--text-muted)",
            }}
          >
            {statusText}
          </span>
        </div>

        <div className="flex items-center gap-2.5 shrink-0">
          {/* Enable / Bypass switch */}
          <div className="flex items-center gap-2">
            <span
              className="font-mono-num text-[10px] uppercase tracking-[0.06em]"
              style={{ color: "var(--text-secondary)" }}
            >
              {route.enabled ? "On" : "Bypass"}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={route.enabled}
              aria-label={route.enabled ? "Bypass route" : "Enable route"}
              onClick={() => onUpdateRoute({ ...route, enabled: !route.enabled })}
              className="hw-switch relative w-9 h-5 rounded-full border p-[2px] flex items-center"
              style={{
                backgroundColor: route.enabled ? "var(--accent-signal)" : "var(--bg-well)",
                borderColor: route.enabled ? "var(--accent-signal)" : "var(--border-strong)",
              }}
            >
              <span
                className="hw-switch-thumb block w-3.5 h-3.5 rounded-full"
                style={{
                  backgroundColor: route.enabled ? "#ffffff" : "var(--text-secondary)",
                  transform: route.enabled ? "translateX(16px)" : "translateX(0px)",
                }}
              />
            </button>
          </div>

          {/* Delete route */}
          <button
            type="button"
            aria-label={`Delete ${route.name}`}
            title="Remove route"
            onClick={() => onRemoveRoute(route.id)}
            className="tactile-btn hover-danger p-1.5 rounded-[5px] border"
            style={{
              backgroundColor: "var(--bg-well)",
              borderColor: "var(--border-subtle)",
              color: "var(--text-secondary)",
            }}
          >
            <Trash2 size={13} />
          </button>
        </div>
      </div>

      {/* Main 3-column channel strip */}
      <div className="grid grid-cols-1 md:grid-cols-12 gap-4 items-start">
        {/* Column 1: Source Input + Channel Mapping */}
        <div className="md:col-span-4 flex flex-col gap-2.5 min-w-0">
          <DeviceSelect
            mode="input"
            label="Input"
            devices={devices}
            selectedDeviceId={route.sourceDeviceId}
            isOpen={openMenuKey === srcMenuKey}
            onToggleOpen={() => onSetOpenMenu(openMenuKey === srcMenuKey ? null : srcMenuKey)}
            onClose={() => onSetOpenMenu(null)}
            onSelect={handleSelectInput}
          />
          <ChannelMapSelect
            channels={sourceDevice?.channels ?? 2}
            value={route.channelMode}
            isOpen={openMenuKey === chMenuKey}
            onToggleOpen={() => onSetOpenMenu(openMenuKey === chMenuKey ? null : chMenuKey)}
            onClose={() => onSetOpenMenu(null)}
            onChange={handleSelectChannelMode}
          />
        </div>

        {/* Column 2: Output Target + Latency Mode + Exclusive + Diagnostics */}
        <div className="md:col-span-4 flex flex-col gap-2.5 min-w-0">
          <DeviceSelect
            mode="output"
            label="Output"
            devices={devices}
            selectedDeviceId={route.targetDeviceId}
            isOpen={openMenuKey === dstMenuKey}
            onToggleOpen={() => onSetOpenMenu(openMenuKey === dstMenuKey ? null : dstMenuKey)}
            onClose={() => onSetOpenMenu(null)}
            onSelect={handleSelectOutput}
          />

          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between gap-2">
              <span
                className="text-[10px] font-semibold uppercase tracking-[0.08em]"
                style={{ color: "var(--text-secondary)" }}
              >
                Latency
              </span>
              <button
                type="button"
                role="switch"
                aria-checked={route.exclusiveOutput}
                onClick={() =>
                  onUpdateRoute({ ...route, exclusiveOutput: !route.exclusiveOutput })
                }
                title="Request WASAPI Exclusive Mode on render endpoint (bypasses Windows mixer)"
                className="tactile-btn hover-surface inline-flex items-center gap-1.5 px-1.5 py-0.5 rounded-[4px] border font-mono-num text-[10px]"
                style={{
                  backgroundColor: route.exclusiveOutput
                    ? "var(--accent-signal-soft)"
                    : "var(--bg-well)",
                  borderColor: route.exclusiveOutput
                    ? "var(--accent-signal-border)"
                    : "var(--border-subtle)",
                  color: route.exclusiveOutput
                    ? "var(--accent-signal)"
                    : "var(--text-secondary)",
                }}
              >
                <span
                  className="w-1.5 h-1.5 rounded-full"
                  style={{
                    backgroundColor: route.exclusiveOutput
                      ? "var(--accent-signal)"
                      : "var(--text-muted)",
                  }}
                />
                EXCL
              </button>
            </div>

            {/* Segmented tactile LatencyMode selector */}
            <div
              className="grid grid-cols-4 gap-1 p-0.5 rounded-[6px] border"
              style={{
                backgroundColor: "var(--bg-well)",
                borderColor: "var(--border-subtle)",
              }}
            >
              {LATENCY_MODES.map((item) => {
                const active = route.latencyMode === item.value;
                return (
                  <button
                    key={item.value}
                    type="button"
                    aria-pressed={active}
                    title={`${item.label} (${item.targetHint})`}
                    onClick={() => handleLatencyMode(item.value)}
                    className="tactile-btn py-1 px-1 rounded-[4px] font-mono-num text-[10px] font-medium truncate"
                    style={{
                      backgroundColor: active ? "var(--bg-panel-elevated)" : "transparent",
                      color: active ? "var(--text-primary)" : "var(--text-secondary)",
                      boxShadow: active ? "inset 0 0 0 1px var(--border-strong)" : undefined,
                    }}
                  >
                    {item.label}
                  </button>
                );
              })}
            </div>

            <div className="flex items-center justify-end">
              <LatencyPopover
                routeId={route.id}
                enabled={route.enabled}
                summaryTelemetry={summaryTelemetry}
                telemetryMapRef={telemetryMapRef}
                isOpen={openMenuKey === latMenuKey}
                onToggleOpen={() => onSetOpenMenu(openMenuKey === latMenuKey ? null : latMenuKey)}
                onClose={() => onSetOpenMenu(null)}
              />
            </div>
          </div>
        </div>

        {/* Column 3: 60fps Stereo VU Meter + Gain Fader + Mute */}
        <div
          className="md:col-span-4 flex flex-col gap-2.5 p-2.5 rounded-[8px] border min-w-0"
          style={{
            backgroundColor: "var(--bg-well)",
            borderColor: "var(--border-subtle)",
          }}
        >
          <VuMeterCanvas
            routeId={route.id}
            enabled={route.enabled}
            muted={route.muted}
            theme={theme}
            telemetryMapRef={telemetryMapRef}
          />

          <div className="flex items-end gap-2.5">
            <div className="flex-1 min-w-0">
              <GainFader
                gainDb={route.gainDb}
                muted={route.muted}
                disabled={!route.enabled}
                onChangeGain={(nextGainDb) =>
                  onGainMuteChange(route.id, nextGainDb, route.muted)
                }
              />
            </div>

            <button
              type="button"
              data-instant={instantMute ? "true" : undefined}
              aria-pressed={route.muted}
              aria-label={route.muted ? "Unmute route (M)" : "Mute route (M)"}
              title="Toggle Mute (Shortcut: M)"
              onClick={() => onGainMuteChange(route.id, route.gainDb, !route.muted)}
              className="tactile-btn hover-surface shrink-0 h-8 px-2.5 rounded-[6px] border inline-flex items-center gap-1.5 font-mono-num text-[11px] font-semibold"
              style={{
                backgroundColor: route.muted
                  ? "var(--accent-signal)"
                  : "var(--bg-panel)",
                borderColor: route.muted
                  ? "var(--accent-signal)"
                  : "var(--border-strong)",
                color: route.muted ? "#ffffff" : "var(--text-primary)",
              }}
            >
              {route.muted ? <VolumeX size={13} /> : <Volume2 size={13} />}
              <span>M</span>
            </button>
          </div>
        </div>
      </div>

      {/* Persistent Inline Bluetooth HFP Warning Strip */}
      {isHfpWarning && (
        <div
          role="alert"
          className="mt-3 px-3 py-2 rounded-[6px] border flex flex-wrap items-center justify-between gap-2 text-[11px]"
          style={{
            backgroundColor: "var(--status-warning-soft)",
            borderColor: "var(--status-warning-border)",
            color: "var(--text-primary)",
          }}
        >
          <div className="flex items-center gap-2 min-w-0">
            <AlertTriangle
              size={14}
              className="shrink-0"
              style={{ color: "var(--status-warning)" }}
            />
            <span>
              <strong>Bluetooth HFP</strong> limits audio to 8–16 kHz mono. Switch to Stereo A2DP.
            </span>
          </div>
          {a2dpCompanion && (
            <button
              type="button"
              onClick={() => handleSelectOutput(a2dpCompanion)}
              className="tactile-btn shrink-0 px-2.5 py-1 rounded-[5px] font-mono-num text-[11px] font-semibold"
              style={{
                backgroundColor: "var(--status-warning)",
                color: "#0d0e10",
              }}
            >
              Switch to {shortDeviceName(a2dpCompanion.name)}
            </button>
          )}
        </div>
      )}

      {/* Backend Route Error Strip */}
      {status.state === "error" && (
        <div
          role="alert"
          className="mt-2.5 px-3 py-2 rounded-[6px] border flex items-center gap-2 text-[11px]"
          style={{
            backgroundColor: "var(--status-error-soft)",
            borderColor: "var(--status-error-border)",
            color: "var(--text-primary)",
          }}
        >
          <AlertTriangle
            size={14}
            className="shrink-0"
            style={{ color: "var(--status-error)" }}
          />
          <span className="font-mono-num">{status.message}</span>
        </div>
      )}
    </article>
  );
}
