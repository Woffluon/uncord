use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{addr_of, read_unaligned, slice_from_raw_parts, slice_from_raw_parts_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use windows::core::{implement, w, Interface, HRESULT, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_Device_EnumeratorName, PKEY_Device_FriendlyName,
};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, PROPERTYKEY, RPC_E_CHANGED_MODE, S_FALSE, S_OK, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows::Win32::Media::Audio::{
    eAll, eCapture, eConsole, eRender, AudioCategory_Media, AudioClientProperties,
    IAudioCaptureClient, IAudioClient, IAudioClient2, IAudioClient3, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, IMMEndpoint, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_EXCLUSIVE,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMOPTIONS_RAW, DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::Media::KernelStreaming::{
    KSDATAFORMAT_SUBTYPE_PCM, KSAUDIO_SPEAKER_MONO, WAVE_FORMAT_EXTENSIBLE,
};
use windows::Win32::Media::Multimedia::{
    KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, SetEvent,
    WaitForMultipleObjects,
};
use windows::Win32::System::Variant::VT_LPWSTR;

use crate::backend::{ActiveRouteHandle, AudioBackend, AudioError, RouteTelemetryAtomics};
use crate::dsp::{
    classify_transport, compute_ring_buffer_layout, estimated_transport_floor_ms,
    AdaptiveResampler, AtomicChannelMode, AtomicGainControl, ChannelMapper, GainSmoother,
    LevelMeterAccumulator, RingBufferLayout,
};
use crate::types::{
    AudioDeviceInfo, ChannelMode, DeviceChangeEvent, DeviceFlow, LatencyBreakdown, LatencyMode,
    RouteConfig, RouteStatus, RouteTelemetry, TransportKind,
};

const WAVE_FORMAT_PCM_TAG: u16 = 1;

struct ComGuard {
    should_uninit: bool,
}

impl ComGuard {
    fn init_mta() -> Result<Self, AudioError> {
        // SAFETY: `CoInitializeEx` initializes COM on the current thread; if already initialized
        // in another apartment (e.g. Tauri's STA UI thread), `RPC_E_CHANGED_MODE` is tolerated.
        let hr: HRESULT = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == S_OK || hr == S_FALSE {
            Ok(Self {
                should_uninit: true,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self {
                should_uninit: false,
            })
        } else {
            Err(AudioError::Platform(format!(
                "CoInitializeEx failed: 0x{:08X}",
                hr.0 as u32
            )))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: Paired with a successful `CoInitializeEx` on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

struct MmcssGuard(HANDLE);

impl MmcssGuard {
    fn enter_pro_audio() -> Option<Self> {
        let mut task_index = 0_u32;
        // SAFETY: Registers current thread with MMCSS "Pro Audio" scheduler priority.
        unsafe {
            AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index)
                .ok()
                .map(Self)
        }
    }
}

impl Drop for MmcssGuard {
    fn drop(&mut self) {
        // SAFETY: Reverts MMCSS registration created by `AvSetMmThreadCharacteristicsW`.
        unsafe {
            let _ = AvRevertMmThreadCharacteristics(self.0);
        }
    }
}

#[derive(Clone, Copy)]
struct SendHandle(isize);

impl SendHandle {
    fn new_manual_reset_event() -> Result<Self, AudioError> {
        // SAFETY: Creates an unnamed manual-reset Win32 event.
        let handle = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(|err| {
            AudioError::Platform(format!("CreateEventW (manual) failed: {err}"))
        })?;
        Ok(Self(handle.0 as isize))
    }

    fn new_auto_reset_event() -> Result<Self, AudioError> {
        // SAFETY: Creates an unnamed auto-reset Win32 event.
        let handle =
            unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(|err| {
                AudioError::Platform(format!("CreateEventW (auto) failed: {err}"))
            })?;
        Ok(Self(handle.0 as isize))
    }

    #[inline]
    fn raw(self) -> HANDLE {
        HANDLE(self.0 as *mut c_void)
    }

    fn set(self) {
        // SAFETY: Handle remains valid until owner drops it after threads exit.
        unsafe {
            let _ = SetEvent(self.raw());
        }
    }

    fn close(self) {
        if self.0 != 0 {
            // SAFETY: Closes Win32 event handle once no thread is waiting on it.
            unsafe {
                let _ = CloseHandle(self.raw());
            }
        }
    }
}

struct MixFormatPtr(*mut WAVEFORMATEX);

impl Drop for MixFormatPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `GetMixFormat` allocates via `CoTaskMemAlloc`.
            unsafe { CoTaskMemFree(Some(self.0 as *const c_void)) };
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SampleFormat {
    Pcm16,
    Pcm24Packed,
    Pcm24In32,
    Pcm32,
    Float32,
}

#[derive(Clone, Copy, Debug)]
struct StreamFormat {
    sample_rate: u32,
    channels: u16,
    block_align: u16,
    sample_format: SampleFormat,
}

impl StreamFormat {
    /// Parses `WAVEFORMATEX` / `WAVEFORMATEXTENSIBLE` safely without creating unaligned references.
    fn from_waveformat(ptr: *const WAVEFORMATEX, device_id: &str) -> Result<Self, AudioError> {
        if ptr.is_null() {
            return Err(AudioError::UnsupportedFormat(device_id.to_string()));
        }

        // SAFETY: Pointer comes from `IAudioClient::GetMixFormat` or stack-owned candidate.
        let base = unsafe { read_unaligned(ptr) };
        let sample_rate = base.nSamplesPerSec;
        let channels = base.nChannels;
        let block_align = base.nBlockAlign;
        let bits = base.wBitsPerSample;

        let sample_format = match base.wFormatTag as u32 {
            tag if tag == WAVE_FORMAT_PCM_TAG as u32 => match bits {
                16 => SampleFormat::Pcm16,
                24 => SampleFormat::Pcm24Packed,
                32 => SampleFormat::Pcm32,
                _ => return Err(AudioError::UnsupportedFormat(device_id.to_string())),
            },
            tag if tag == WAVE_FORMAT_IEEE_FLOAT && bits == 32 => SampleFormat::Float32,
            tag if tag == WAVE_FORMAT_EXTENSIBLE => {
                // SAFETY: `WAVE_FORMAT_EXTENSIBLE` always follows `WAVEFORMATEX` when cbSize >= 22.
                let ext = unsafe { read_unaligned(ptr.cast::<WAVEFORMATEXTENSIBLE>()) };
                let valid_bits = unsafe { read_unaligned(addr_of!(ext.Samples.wValidBitsPerSample)) };
                let sub_format = unsafe { read_unaligned(addr_of!(ext.SubFormat)) };
                if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && bits == 32 {
                    SampleFormat::Float32
                } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
                    match (bits, valid_bits) {
                        (16, _) => SampleFormat::Pcm16,
                        (24, _) => SampleFormat::Pcm24Packed,
                        (32, 24) => SampleFormat::Pcm24In32,
                        (32, _) => SampleFormat::Pcm32,
                        _ => return Err(AudioError::UnsupportedFormat(device_id.to_string())),
                    }
                } else {
                    return Err(AudioError::UnsupportedFormat(device_id.to_string()));
                }
            }
            _ => return Err(AudioError::UnsupportedFormat(device_id.to_string())),
        };

        if channels == 0 || sample_rate == 0 || block_align == 0 {
            return Err(AudioError::UnsupportedFormat(device_id.to_string()));
        }

        Ok(Self {
            sample_rate,
            channels,
            block_align,
            sample_format,
        })
    }
}

/// Converts raw `PWSTR` returned by COM (`GetId`) into a Rust `String` and frees via `CoTaskMemFree`.
unsafe fn pwstr_to_string_and_free(ptr: PWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let text = ptr.to_string().unwrap_or_default();
    CoTaskMemFree(Some(ptr.0 as *const c_void));
    text
}

/// Converts borrowed `&PCWSTR` (e.g. from `IMMNotificationClient`) into a Rust `String`.
unsafe fn pcwstr_to_string(ptr: &PCWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    ptr.to_string().unwrap_or_default()
}

struct PropVariantGuard(PROPVARIANT);

impl Drop for PropVariantGuard {
    fn drop(&mut self) {
        // SAFETY: Clears COM-allocated variant storage.
        unsafe {
            let _ = PropVariantClear(&mut self.0);
        }
    }
}

fn read_device_string_property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: Opens read-only property store and reads string variant if present.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let prop = store.GetValue(key).ok()?;
        let guard = PropVariantGuard(prop);
        let vt = addr_of!(guard.0.Anonymous.Anonymous.vt).read_unaligned();
        if vt == VT_LPWSTR {
            let pwsz = addr_of!(guard.0.Anonymous.Anonymous.Anonymous.pwszVal).read_unaligned();
            if !pwsz.is_null() {
                return pwsz.to_string().ok();
            }
        }
        None
    }
}

fn create_enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // SAFETY: Creates the Windows Multimedia Device Enumerator COM object on current thread.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|err| {
        AudioError::BackendInit(format!("failed to create IMMDeviceEnumerator: {err}"))
    })
}

fn try_set_raw_mode(client: &IAudioClient) -> bool {
    let Ok(client2) = client.cast::<IAudioClient2>() else {
        return false;
    };
    let props = AudioClientProperties {
        cbSize: size_of::<AudioClientProperties>() as u32,
        bIsOffload: false.into(),
        eCategory: AudioCategory_Media,
        Options: AUDCLNT_STREAMOPTIONS_RAW,
    };
    // SAFETY: Must be invoked prior to `Initialize` on `IAudioClient2`.
    unsafe { client2.SetClientProperties(&props).is_ok() }
}

fn query_period_and_raw(
    device: &IMMDevice,
    mix_ptr: *const WAVEFORMATEX,
    sample_rate: u32,
    check_raw: bool,
) -> (u32, u32, bool) {
    let fallback_10ms = ((sample_rate as f64 * 0.010).round() as u32).max(1);

    let (min_frames, def_frames) = unsafe {
        if let Ok(client3) = device.Activate::<IAudioClient3>(CLSCTX_ALL, None) {
            let mut default_p = 0_u32;
            let mut fundamental_p = 0_u32;
            let mut min_p = 0_u32;
            let mut max_p = 0_u32;
            if client3
                .GetSharedModeEnginePeriod(
                    mix_ptr,
                    &mut default_p,
                    &mut fundamental_p,
                    &mut min_p,
                    &mut max_p,
                )
                .is_ok()
                && min_p > 0
                && default_p > 0
            {
                (min_p, default_p)
            } else {
                (fallback_10ms, fallback_10ms)
            }
        } else {
            (fallback_10ms, fallback_10ms)
        }
    };

    let supports_raw = if check_raw {
        unsafe {
            device
                .Activate::<IAudioClient>(CLSCTX_ALL, None)
                .ok()
                .map(|c| try_set_raw_mode(&c))
                .unwrap_or(false)
        }
    } else {
        false
    };

    (min_frames, def_frames, supports_raw)
}

fn default_endpoint_id(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    // SAFETY: Queries default console endpoint ID; missing endpoint returns None gracefully.
    unsafe {
        let dev = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
        let id_pwstr = dev.GetId().ok()?;
        Some(pwstr_to_string_and_free(id_pwstr))
    }
}

type DeviceEventListener = Arc<dyn Fn(DeviceChangeEvent) + Send + Sync>;
type SharedDeviceListeners = Arc<Mutex<Vec<DeviceEventListener>>>;

#[implement(IMMNotificationClient)]
struct NotificationClient {
    listeners: SharedDeviceListeners,
}

impl NotificationClient {
    fn broadcast(&self, event: DeviceChangeEvent) {
        let snapshot = self
            .listeners
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for cb in snapshot {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cb(event.clone());
            }));
        }
    }
}

impl IMMNotificationClient_Impl for NotificationClient_Impl {
    fn OnDeviceStateChanged(
        &self,
        pwstrdeviceid: &PCWSTR,
        _dwnewstate: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        let id = unsafe { pcwstr_to_string(pwstrdeviceid) };
        self.broadcast(DeviceChangeEvent::StateChanged(id));
        Ok(())
    }

    fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        let id = unsafe { pcwstr_to_string(pwstrdeviceid) };
        self.broadcast(DeviceChangeEvent::Added(id));
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        let id = unsafe { pcwstr_to_string(pwstrdeviceid) };
        self.broadcast(DeviceChangeEvent::Removed(id));
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        _flow: EDataFlow,
        _role: ERole,
        _pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.broadcast(DeviceChangeEvent::DefaultChanged);
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

struct NotificationWorker {
    stop_event: SendHandle,
    thread: Option<JoinHandle<()>>,
}

impl NotificationWorker {
    fn start(listeners: SharedDeviceListeners) -> Result<Self, AudioError> {
        let stop_event = SendHandle::new_manual_reset_event()?;
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), AudioError>>(1);
        let worker_stop = stop_event;

        let thread = thread::Builder::new()
            .name("uncord-wasapi-notify".to_string())
            .spawn(move || {
                let _com = match ComGuard::init_mta() {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };

                let enumerator = match create_enumerator() {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };

                let client: IMMNotificationClient = NotificationClient { listeners }.into();
                if let Err(err) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
                {
                    let _ = ready_tx.send(Err(AudioError::Platform(format!(
                        "RegisterEndpointNotificationCallback failed: {err}"
                    ))));
                    return;
                }

                let _ = ready_tx.send(Ok(()));

                // Sleep efficiently until `WasapiBackend` drops.
                let handles = [worker_stop.raw()];
                unsafe {
                    let _ = WaitForMultipleObjects(
                        &handles,
                        false,
                        windows::Win32::System::Threading::INFINITE,
                    );
                    let _ = enumerator.UnregisterEndpointNotificationCallback(&client);
                }
            })
            .map_err(|err| {
                stop_event.close();
                AudioError::BackendInit(format!("failed to spawn WASAPI notify thread: {err}"))
            })?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                stop_event,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                stop_event.set();
                let _ = thread.join();
                stop_event.close();
                Err(e)
            }
            Err(_) => {
                stop_event.set();
                let _ = thread.join();
                stop_event.close();
                Err(AudioError::BackendInit(
                    "WASAPI notify thread exited during initialization".into(),
                ))
            }
        }
    }
}

impl Drop for NotificationWorker {
    fn drop(&mut self) {
        self.stop_event.set();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        self.stop_event.close();
    }
}

/// Windows WASAPI low-latency `AudioBackend` implementation (`IAudioClient3`, Raw APO bypass,
/// Loopback, Exclusive mode, and `IMMNotificationClient` hot-plug detection).
pub struct WasapiBackend {
    listeners: SharedDeviceListeners,
    notifier: Mutex<Option<NotificationWorker>>,
}

impl WasapiBackend {
    pub fn new() -> Result<Self, AudioError> {
        // Probe COM & enumerator availability eagerly on startup.
        let _com = ComGuard::init_mta()?;
        let _enum = create_enumerator()?;
        Ok(Self {
            listeners: Arc::new(Mutex::new(Vec::new())),
            notifier: Mutex::new(None),
        })
    }
}

impl AudioBackend for WasapiBackend {
    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError> {
        let _com = ComGuard::init_mta()?;
        let enumerator = create_enumerator()?;

        let default_capture = default_endpoint_id(&enumerator, eCapture);
        let default_render = default_endpoint_id(&enumerator, eRender);

        // SAFETY: Enumerates all active audio endpoints on Windows.
        let collection = unsafe { enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE) }
            .map_err(|err| AudioError::Platform(format!("EnumAudioEndpoints failed: {err}")))?;

        let count = unsafe { collection.GetCount() }
            .map_err(|err| AudioError::Platform(format!("IMMDeviceCollection::GetCount: {err}")))?;

        let mut devices = Vec::with_capacity((count as usize) * 2);
        let mut loopback_candidates = Vec::new();

        for idx in 0..count {
            let Ok(device) = (unsafe { collection.Item(idx) }) else {
                continue;
            };
            let Ok(id_pwstr) = (unsafe { device.GetId() }) else {
                continue;
            };
            let id = unsafe { pwstr_to_string_and_free(id_pwstr) };

            let Ok(endpoint) = device.cast::<IMMEndpoint>() else {
                continue;
            };
            let Ok(data_flow) = (unsafe { endpoint.GetDataFlow() }) else {
                continue;
            };
            let flow = if data_flow == eCapture {
                DeviceFlow::Capture
            } else if data_flow == eRender {
                DeviceFlow::Render
            } else {
                continue;
            };

            let name = read_device_string_property(&device, &PKEY_Device_FriendlyName)
                .unwrap_or_else(|| id.clone());
            let enumerator_name =
                read_device_string_property(&device, &PKEY_Device_EnumeratorName);

            let Ok(audio_client) = (unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) })
            else {
                continue;
            };
            let Ok(mix_raw) = (unsafe { audio_client.GetMixFormat() }) else {
                continue;
            };
            let mix_guard = MixFormatPtr(mix_raw);
            let Ok(fmt) = StreamFormat::from_waveformat(mix_guard.0, &id) else {
                continue;
            };

            let (min_period_frames, default_period_frames, supports_raw_mode) =
                query_period_and_raw(&device, mix_guard.0, fmt.sample_rate, true);

            let (transport, is_bluetooth_hfp_warning) = classify_transport(
                &name,
                enumerator_name.as_deref(),
                fmt.channels,
                fmt.sample_rate,
            );

            let is_default = match flow {
                DeviceFlow::Capture => default_capture.as_deref() == Some(id.as_str()),
                DeviceFlow::Render => default_render.as_deref() == Some(id.as_str()),
                DeviceFlow::Loopback => false,
            };

            let info = AudioDeviceInfo {
                id: id.clone(),
                name: name.clone(),
                flow,
                channels: fmt.channels,
                sample_rate: fmt.sample_rate,
                min_period_frames,
                default_period_frames,
                supports_raw_mode,
                transport,
                is_default,
                is_bluetooth_hfp_warning,
            };

            if flow == DeviceFlow::Render {
                loopback_candidates.push(AudioDeviceInfo {
                    id: format!("loopback::{id}"),
                    name,
                    flow: DeviceFlow::Loopback,
                    channels: fmt.channels,
                    sample_rate: fmt.sample_rate,
                    min_period_frames,
                    default_period_frames,
                    supports_raw_mode: false,
                    transport,
                    is_default: false,
                    is_bluetooth_hfp_warning,
                });
            }

            devices.push(info);
        }

        devices.extend(loopback_candidates);
        Ok(devices)
    }

    fn start_route(&self, config: &RouteConfig) -> Result<Box<dyn ActiveRouteHandle>, AudioError> {
        let is_loopback =
            config.source_is_loopback || config.source_device_id.starts_with("loopback::");
        let canonical_source_id = config
            .source_device_id
            .strip_prefix("loopback::")
            .unwrap_or(&config.source_device_id)
            .to_string();
        let target_id = config.target_device_id.clone();

        // Pre-probe transport metadata quickly before launching threads.
        let (source_transport, target_transport) = {
            let _com = ComGuard::init_mta()?;
            let enumerator = create_enumerator()?;
            let src_dev = open_device_by_id(&enumerator, &canonical_source_id)?;
            let dst_dev = open_device_by_id(&enumerator, &target_id)?;
            (
                probe_device_transport(&src_dev, &canonical_source_id),
                probe_device_transport(&dst_dev, &target_id),
            )
        };

        let stop_event = SendHandle::new_manual_reset_event()?;
        let stop_flag = Arc::new(AtomicBool::new(false));
        let disconnected = Arc::new(AtomicBool::new(false));
        let atomics = Arc::new(RouteTelemetryAtomics::new());
        let gain_ctrl = Arc::new(AtomicGainControl::new(config.gain_db, config.muted));
        let channel_ctrl = Arc::new(AtomicChannelMode::new(config.channel_mode));

        let (cap_init_tx, cap_init_rx) = mpsc::sync_channel::<Result<CaptureInitInfo, AudioError>>(1);
        let (cap_rb_tx, cap_rb_rx) =
            mpsc::sync_channel::<ringbuf::HeapProd<f32>>(1);

        let (ren_init_tx, ren_init_rx) = mpsc::sync_channel::<Result<RenderInitInfo, AudioError>>(1);
        let (ren_rb_tx, ren_rb_rx) =
            mpsc::sync_channel::<(ringbuf::HeapCons<f32>, RingBufferLayout, u32)>(1);

        let latency_mode = config.latency_mode;
        let exclusive_output = config.exclusive_output;
        let cap_stop_flag = Arc::clone(&stop_flag);
        let cap_disconnected = Arc::clone(&disconnected);
        let cap_atomics = Arc::clone(&atomics);
        let cap_channel_ctrl = Arc::clone(&channel_ctrl);
        let cap_src_id = canonical_source_id.clone();

        let capture_thread = thread::Builder::new()
            .name(format!("uncord-cap-{}", config.id))
            .spawn(move || {
                run_capture_thread(CaptureThreadArgs {
                    device_id: cap_src_id,
                    is_loopback,
                    latency_mode,
                    stop_event,
                    stop_flag: cap_stop_flag,
                    disconnected: cap_disconnected,
                    atomics: cap_atomics,
                    channel_ctrl: cap_channel_ctrl,
                    init_tx: cap_init_tx,
                    rb_rx: cap_rb_rx,
                });
            })
            .map_err(|err| {
                stop_event.close();
                AudioError::BackendInit(format!("failed to spawn capture thread: {err}"))
            })?;

        let ren_stop_flag = Arc::clone(&stop_flag);
        let ren_disconnected = Arc::clone(&disconnected);
        let ren_atomics = Arc::clone(&atomics);
        let ren_gain_ctrl = Arc::clone(&gain_ctrl);
        let ren_dst_id = target_id.clone();

        let render_thread = match thread::Builder::new()
            .name(format!("uncord-ren-{}", config.id))
            .spawn(move || {
                run_render_thread(RenderThreadArgs {
                    device_id: ren_dst_id,
                    latency_mode,
                    exclusive_output,
                    stop_event,
                    stop_flag: ren_stop_flag,
                    disconnected: ren_disconnected,
                    atomics: ren_atomics,
                    gain_ctrl: ren_gain_ctrl,
                    init_tx: ren_init_tx,
                    rb_tx_rx: ren_rb_rx,
                });
            }) {
            Ok(h) => h,
            Err(err) => {
                stop_flag.store(true, Ordering::Release);
                stop_event.set();
                let _ = capture_thread.join();
                stop_event.close();
                return Err(AudioError::BackendInit(format!(
                    "failed to spawn render thread: {err}"
                )));
            }
        };

        let cap_info = match cap_init_rx.recv() {
            Ok(Ok(info)) => info,
            Ok(Err(err)) => {
                abort_threads(stop_flag, stop_event, capture_thread, render_thread);
                return Err(err);
            }
            Err(_) => {
                abort_threads(stop_flag, stop_event, capture_thread, render_thread);
                return Err(AudioError::BackendInit(
                    "capture thread terminated during initialization".into(),
                ));
            }
        };

        let ren_info = match ren_init_rx.recv() {
            Ok(Ok(info)) => info,
            Ok(Err(err)) => {
                abort_threads(stop_flag, stop_event, capture_thread, render_thread);
                return Err(err);
            }
            Err(_) => {
                abort_threads(stop_flag, stop_event, capture_thread, render_thread);
                return Err(AudioError::BackendInit(
                    "render thread terminated during initialization".into(),
                ));
            }
        };

        let layout = compute_ring_buffer_layout(
            cap_info.sample_rate,
            cap_info.period_frames,
            ren_info.sample_rate,
            ren_info.period_frames,
            config.latency_mode,
        );

        let rb = HeapRb::<f32>::new(layout.capacity_frames * 2);
        let (mut prod, cons) = rb.split();
        for _ in 0..(layout.target_frames * 2) {
            let _ = prod.try_push(0.0);
        }

        let cap_ms = (cap_info.period_frames as f32 / cap_info.sample_rate as f32) * 1000.0;
        let ren_ms = (ren_info.period_frames as f32 / ren_info.sample_rate as f32) * 1000.0;
        let floor_ms = estimated_transport_floor_ms(source_transport)
            + estimated_transport_floor_ms(target_transport);
        let raw_active = cap_info.raw_mode_active || ren_info.raw_mode_active;

        atomics.store_latency(LatencyBreakdown {
            capture_period_ms: cap_ms,
            ring_buffer_ms: layout.target_ms,
            render_period_ms: ren_ms,
            software_total_ms: cap_ms + layout.target_ms + ren_ms,
            estimated_transport_floor_ms: floor_ms,
            raw_mode_active: raw_active,
            exclusive_active: ren_info.exclusive_active,
        });

        if cap_rb_tx.send(prod).is_err()
            || ren_rb_tx
                .send((cons, layout, cap_info.sample_rate))
                .is_err()
        {
            abort_threads(stop_flag, stop_event, capture_thread, render_thread);
            return Err(AudioError::BackendInit(
                "audio stream thread failed to receive SPSC ring buffer".into(),
            ));
        }

        Ok(Box::new(WasapiRouteHandle {
            route_id: config.id.clone(),
            atomics,
            gain_ctrl,
            channel_ctrl,
            disconnected,
            stop_flag,
            stop_event,
            capture_thread: Some(capture_thread),
            render_thread: Some(render_thread),
        }))
    }

    fn subscribe_device_changes(
        &self,
        callback: Box<dyn Fn(DeviceChangeEvent) + Send + Sync>,
    ) -> Result<(), AudioError> {
        {
            let mut guard = self
                .listeners
                .lock()
                .map_err(|_| AudioError::Platform("listener lock poisoned".into()))?;
            guard.push(Arc::from(callback));
        }

        let mut notifier_guard = self
            .notifier
            .lock()
            .map_err(|_| AudioError::Platform("notifier lock poisoned".into()))?;
        if notifier_guard.is_none() {
            *notifier_guard = Some(NotificationWorker::start(Arc::clone(&self.listeners))?);
        }

        Ok(())
    }
}

fn abort_threads(
    stop_flag: Arc<AtomicBool>,
    stop_event: SendHandle,
    cap: JoinHandle<()>,
    ren: JoinHandle<()>,
) {
    stop_flag.store(true, Ordering::Release);
    stop_event.set();
    let _ = cap.join();
    let _ = ren.join();
    stop_event.close();
}

fn open_device_by_id(
    enumerator: &IMMDeviceEnumerator,
    device_id: &str,
) -> Result<IMMDevice, AudioError> {
    let wide = HSTRING::from(device_id);
    unsafe { enumerator.GetDevice(&wide) }
        .map_err(|_| AudioError::DeviceNotFound(device_id.to_string()))
}

fn probe_device_transport(device: &IMMDevice, device_id: &str) -> TransportKind {
    let name = read_device_string_property(device, &PKEY_Device_FriendlyName)
        .unwrap_or_else(|| device_id.to_string());
    let enumerator_name = read_device_string_property(device, &PKEY_Device_EnumeratorName);
    let (channels, sample_rate) = unsafe {
        if let Ok(client) = device.Activate::<IAudioClient>(CLSCTX_ALL, None) {
            if let Ok(mix) = client.GetMixFormat() {
                let guard = MixFormatPtr(mix);
                if let Ok(fmt) = StreamFormat::from_waveformat(guard.0, device_id) {
                    (fmt.channels, fmt.sample_rate)
                } else {
                    (2, 48_000)
                }
            } else {
                (2, 48_000)
            }
        } else {
            (2, 48_000)
        }
    };
    classify_transport(&name, enumerator_name.as_deref(), channels, sample_rate).0
}

struct CaptureInitInfo {
    sample_rate: u32,
    period_frames: u32,
    raw_mode_active: bool,
}

struct CaptureThreadArgs {
    device_id: String,
    is_loopback: bool,
    latency_mode: LatencyMode,
    stop_event: SendHandle,
    stop_flag: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
    atomics: Arc<RouteTelemetryAtomics>,
    channel_ctrl: Arc<AtomicChannelMode>,
    init_tx: mpsc::SyncSender<Result<CaptureInitInfo, AudioError>>,
    rb_rx: mpsc::Receiver<ringbuf::HeapProd<f32>>,
}

struct RenderInitInfo {
    sample_rate: u32,
    period_frames: u32,
    raw_mode_active: bool,
    exclusive_active: bool,
}

struct RenderThreadArgs {
    device_id: String,
    latency_mode: LatencyMode,
    exclusive_output: bool,
    stop_event: SendHandle,
    stop_flag: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
    atomics: Arc<RouteTelemetryAtomics>,
    gain_ctrl: Arc<AtomicGainControl>,
    init_tx: mpsc::SyncSender<Result<RenderInitInfo, AudioError>>,
    rb_tx_rx: mpsc::Receiver<(ringbuf::HeapCons<f32>, RingBufferLayout, u32)>,
}

fn initialize_shared_low_latency(
    device: &IMMDevice,
    device_id: &str,
    latency_mode: LatencyMode,
    is_loopback: bool,
) -> Result<(IAudioClient, StreamFormat, u32, bool, bool), AudioError> {
    let mut raw_active = false;

    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|err| AudioError::Platform(format!("Activate IAudioClient ({device_id}): {err}")))?;

    if !is_loopback {
        raw_active = try_set_raw_mode(&audio_client);
    }

    let mix_raw = unsafe { audio_client.GetMixFormat() }
        .map_err(|err| AudioError::Platform(format!("GetMixFormat ({device_id}): {err}")))?;
    let mix_guard = MixFormatPtr(mix_raw);
    let format = StreamFormat::from_waveformat(mix_guard.0, device_id)?;

    let mut use_event_callback = true;

    if !is_loopback {
        if let Ok(client3) = audio_client.cast::<IAudioClient3>() {
            let mut default_p = 0_u32;
            let mut fundamental_p = 0_u32;
            let mut min_p = 0_u32;
            let mut max_p = 0_u32;
            if unsafe {
                client3.GetSharedModeEnginePeriod(
                    mix_guard.0,
                    &mut default_p,
                    &mut fundamental_p,
                    &mut min_p,
                    &mut max_p,
                )
            }
            .is_ok()
                && min_p > 0
                && default_p > 0
            {
                let target_p = match latency_mode {
                    LatencyMode::UltraLow | LatencyMode::Low => min_p,
                    LatencyMode::Balanced | LatencyMode::Safe => default_p,
                };
                if unsafe {
                    client3.InitializeSharedAudioStream(
                        AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                        target_p,
                        mix_guard.0,
                        None,
                    )
                }
                .is_ok()
                {
                    return Ok((audio_client, format, target_p, raw_active, true));
                }
            }
        }
    }

    // Fallback to standard IAudioClient::Initialize (shared event-driven, or loopback + event/timer).
    let mut default_100ns = 100_000_i64;
    let mut min_100ns = 30_000_i64;
    unsafe {
        let _ = audio_client.GetDevicePeriod(Some(&mut default_100ns), Some(&mut min_100ns));
    }
    let chosen_100ns = match latency_mode {
        LatencyMode::UltraLow | LatencyMode::Low => min_100ns.max(20_000),
        LatencyMode::Balanced | LatencyMode::Safe => default_100ns.max(min_100ns),
    };
    let period_frames =
        ((format.sample_rate as f64 * (chosen_100ns as f64 / 10_000_000.0)).round() as u32).max(32);

    let buffer_100ns = default_100ns.max(200_000);
    let base_flags = if is_loopback {
        AUDCLNT_STREAMFLAGS_LOOPBACK
    } else {
        0
    };

    let init_res = unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            base_flags | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            buffer_100ns,
            0,
            mix_guard.0,
            None,
        )
    };

    if let Err(err) = init_res {
        if is_loopback {
            // Some Windows drivers reject EVENTCALLBACK combined with LOOPBACK; fall back to timer-paced polling.
            unsafe {
                audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buffer_100ns,
                    0,
                    mix_guard.0,
                    None,
                )
            }
            .map_err(|e2| {
                AudioError::Platform(format!(
                    "WASAPI loopback Initialize failed ({device_id}): {e2}"
                ))
            })?;
            use_event_callback = false;
        } else {
            return Err(AudioError::Platform(format!(
                "WASAPI Initialize failed ({device_id}): {err}"
            )));
        }
    }

    Ok((
        audio_client,
        format,
        period_frames,
        raw_active,
        use_event_callback,
    ))
}

enum ExclusiveFormatCandidate {
    MixCopy(Vec<u8>),
    Extensible(WAVEFORMATEXTENSIBLE),
    Pcm16(WAVEFORMATEX),
}

impl ExclusiveFormatCandidate {
    fn as_ptr(&self) -> *const WAVEFORMATEX {
        match self {
            Self::MixCopy(buf) => buf.as_ptr().cast::<WAVEFORMATEX>(),
            Self::Extensible(ext) => (ext as *const WAVEFORMATEXTENSIBLE).cast::<WAVEFORMATEX>(),
            Self::Pcm16(wfx) => wfx as *const WAVEFORMATEX,
        }
    }
}

fn build_exclusive_candidates(mix_ptr: *const WAVEFORMATEX) -> Vec<ExclusiveFormatCandidate> {
    let base = unsafe { read_unaligned(mix_ptr) };
    let total_size = size_of::<WAVEFORMATEX>() + base.cbSize as usize;
    let mut raw_copy = vec![0_u8; total_size];
    unsafe {
        std::ptr::copy_nonoverlapping(mix_ptr.cast::<u8>(), raw_copy.as_mut_ptr(), total_size);
    }

    let ch = base.nChannels.max(1);
    let sr = base.nSamplesPerSec.max(8000);
    let ch_mask = if ch == 1 {
        KSAUDIO_SPEAKER_MONO
    } else if ch == 2 {
        3 // SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT
    } else {
        (1_u32.checked_shl(ch as u32).unwrap_or(0)).wrapping_sub(1)
    };

    let make_ext_pcm = |container_bits: u16, valid_bits: u16| -> WAVEFORMATEXTENSIBLE {
        let block_align = ch * (container_bits / 8);
        WAVEFORMATEXTENSIBLE {
            Format: WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
                nChannels: ch,
                nSamplesPerSec: sr,
                nAvgBytesPerSec: sr * block_align as u32,
                nBlockAlign: block_align,
                wBitsPerSample: container_bits,
                cbSize: (size_of::<WAVEFORMATEXTENSIBLE>() - size_of::<WAVEFORMATEX>()) as u16,
            },
            Samples: WAVEFORMATEXTENSIBLE_0 {
                wValidBitsPerSample: valid_bits,
            },
            dwChannelMask: ch_mask,
            SubFormat: KSDATAFORMAT_SUBTYPE_PCM,
        }
    };

    let pcm16_plain = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM_TAG,
        nChannels: ch,
        nSamplesPerSec: sr,
        nAvgBytesPerSec: sr * (ch as u32) * 2,
        nBlockAlign: ch * 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };

    vec![
        ExclusiveFormatCandidate::MixCopy(raw_copy),
        ExclusiveFormatCandidate::Extensible(make_ext_pcm(32, 32)),
        ExclusiveFormatCandidate::Extensible(make_ext_pcm(32, 24)),
        ExclusiveFormatCandidate::Extensible(make_ext_pcm(16, 16)),
        ExclusiveFormatCandidate::Pcm16(pcm16_plain),
    ]
}

fn initialize_exclusive_render(
    device: &IMMDevice,
    device_id: &str,
    latency_mode: LatencyMode,
) -> Result<(IAudioClient, StreamFormat, u32), AudioError> {
    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|err| {
        AudioError::ExclusiveModeRejected(device_id.to_string(), err.to_string())
    })?;

    let mix_raw = unsafe { audio_client.GetMixFormat() }.map_err(|err| {
        AudioError::ExclusiveModeRejected(device_id.to_string(), err.to_string())
    })?;
    let mix_guard = MixFormatPtr(mix_raw);

    let mut default_100ns = 100_000_i64;
    let mut min_100ns = 30_000_i64;
    unsafe {
        let _ = audio_client.GetDevicePeriod(Some(&mut default_100ns), Some(&mut min_100ns));
    }
    let period_100ns = match latency_mode {
        LatencyMode::UltraLow | LatencyMode::Low => min_100ns.max(30_000),
        LatencyMode::Balanced | LatencyMode::Safe => default_100ns.max(min_100ns),
    };

    let candidates = build_exclusive_candidates(mix_guard.0);
    let chosen = candidates
        .into_iter()
        .find(|cand| unsafe {
            audio_client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, cand.as_ptr(), None)
                == S_OK
        })
        .ok_or_else(|| {
            AudioError::ExclusiveModeRejected(
                device_id.to_string(),
                "endpoint rejected exclusive mode formats".to_string(),
            )
        })?;

    let chosen_fmt = StreamFormat::from_waveformat(chosen.as_ptr(), device_id)?;
    let init_res = unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_EXCLUSIVE,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            period_100ns,
            period_100ns,
            chosen.as_ptr(),
            None,
        )
    };

    let active_client = match init_res {
        Ok(()) => audio_client,
        Err(err) if err.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
            let aligned_frames = unsafe { audio_client.GetBufferSize() }.map_err(|e2| {
                AudioError::ExclusiveModeRejected(device_id.to_string(), e2.to_string())
            })?;
            let aligned_100ns =
                ((10_000_000.0 * aligned_frames as f64 / chosen_fmt.sample_rate as f64) + 0.5)
                    as i64;
            drop(audio_client);

            let retry_client: IAudioClient =
                unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|e2| {
                    AudioError::ExclusiveModeRejected(device_id.to_string(), e2.to_string())
                })?;
            unsafe {
                retry_client.Initialize(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    aligned_100ns,
                    aligned_100ns,
                    chosen.as_ptr(),
                    None,
                )
            }
            .map_err(|e2| {
                AudioError::ExclusiveModeRejected(device_id.to_string(), e2.to_string())
            })?;
            retry_client
        }
        Err(err) => {
            return Err(AudioError::ExclusiveModeRejected(
                device_id.to_string(),
                err.to_string(),
            ));
        }
    };

    let buffer_frames = unsafe { active_client.GetBufferSize() }.unwrap_or(
        ((chosen_fmt.sample_rate as f64 * (period_100ns as f64 / 10_000_000.0)).round() as u32)
            .max(64),
    );
    Ok((active_client, chosen_fmt, buffer_frames))
}

fn run_capture_thread(args: CaptureThreadArgs) {
    let _com = match ComGuard::init_mta() {
        Ok(g) => g,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let enumerator = match create_enumerator() {
        Ok(e) => e,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let device = match open_device_by_id(&enumerator, &args.device_id) {
        Ok(d) => d,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let (client, format, period_frames, raw_mode_active, use_event_callback) =
        match initialize_shared_low_latency(
            &device,
            &args.device_id,
            args.latency_mode,
            args.is_loopback,
        ) {
            Ok(v) => v,
            Err(e) => {
                let _ = args.init_tx.send(Err(e));
                return;
            }
        };

    let event = if use_event_callback {
        match SendHandle::new_auto_reset_event() {
            Ok(ev) => {
                if let Err(err) = unsafe { client.SetEventHandle(ev.raw()) } {
                    ev.close();
                    let _ = args.init_tx.send(Err(AudioError::Platform(format!(
                        "capture SetEventHandle failed: {err}"
                    ))));
                    return;
                }
                Some(ev)
            }
            Err(e) => {
                let _ = args.init_tx.send(Err(e));
                return;
            }
        }
    } else {
        None
    };

    let capture_client: IAudioCaptureClient = match unsafe { client.GetService() } {
        Ok(c) => c,
        Err(err) => {
            if let Some(ev) = event {
                ev.close();
            }
            let _ = args.init_tx.send(Err(AudioError::Platform(format!(
                "GetService IAudioCaptureClient failed: {err}"
            ))));
            return;
        }
    };

    let _ = args.init_tx.send(Ok(CaptureInitInfo {
        sample_rate: format.sample_rate,
        period_frames,
        raw_mode_active,
    }));

    let Ok(mut prod) = args.rb_rx.recv() else {
        if let Some(ev) = event {
            ev.close();
        }
        return;
    };

    let _mmcss = MmcssGuard::enter_pro_audio();
    if unsafe { client.Start() }.is_err() {
        args.disconnected.store(true, Ordering::Release);
        if let Some(ev) = event {
            ev.close();
        }
        return;
    }

    // Zero heap allocations inside this real-time capture loop.
    let mut frame_f32 = [0.0_f32; 64];
    let mut push_batch = [0.0_f32; 256]; // 128 stereo frames batched onto stack
    let ch_count = (format.channels as usize).min(frame_f32.len());
    let period_ms = ((period_frames as f64 / format.sample_rate as f64) * 1000.0)
        .round()
        .clamp(1.0, 20.0) as u32;
    let wait_timeout_ms = if args.is_loopback {
        period_ms.max(3)
    } else {
        100
    };
    let mut last_packet_at = Instant::now();
    let mut silence_frac = 0.0_f64;

    while !args.stop_flag.load(Ordering::Acquire) {
        if let Some(ev) = event {
            let handles = [args.stop_event.raw(), ev.raw()];
            let wait_res = unsafe { WaitForMultipleObjects(&handles, false, wait_timeout_ms) };
            if wait_res == WAIT_OBJECT_0 {
                break;
            }
            if wait_res == WAIT_TIMEOUT {
                if args.is_loopback {
                    // Loopback streams deliver zero packets while desktop audio is silent; feed silence cadence.
                    let elapsed = last_packet_at.elapsed().as_secs_f64();
                    let frames_due = elapsed * format.sample_rate as f64 + silence_frac;
                    let whole = frames_due.floor() as usize;
                    silence_frac = frames_due - whole as f64;
                    last_packet_at = Instant::now();
                    push_silence_frames(&mut prod, whole, &args.atomics);
                }
            } else if wait_res.0 != WAIT_OBJECT_0.0 + 1 {
                break;
            }
        } else {
            thread::sleep(Duration::from_millis((period_ms / 2).max(1) as u64));
        }

        if args.stop_flag.load(Ordering::Acquire) {
            break;
        }

        let mut had_packets = false;
        loop {
            let packet_frames = match unsafe { capture_client.GetNextPacketSize() } {
                Ok(n) => n,
                Err(err) => {
                    if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                        args.disconnected.store(true, Ordering::Release);
                    }
                    args.stop_flag.store(true, Ordering::Release);
                    break;
                }
            };
            if packet_frames == 0 {
                break;
            }
            had_packets = true;

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut frames_read = 0_u32;
            let mut flags = 0_u32;
            if let Err(err) = unsafe {
                capture_client.GetBuffer(&mut data_ptr, &mut frames_read, &mut flags, None, None)
            } {
                if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                    args.disconnected.store(true, Ordering::Release);
                }
                args.stop_flag.store(true, Ordering::Release);
                break;
            }

            let is_silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0 || data_ptr.is_null();
            let mode = args.channel_ctrl.load();
            let total_frames = frames_read as usize;

            if is_silent {
                push_silence_frames(&mut prod, total_frames, &args.atomics);
            } else {
                let total_bytes = total_frames * format.block_align as usize;
                let raw_bytes = unsafe { &*slice_from_raw_parts(data_ptr, total_bytes) };
                let mut batch_len = 0_usize;

                for f_idx in 0..total_frames {
                    let frame_offset = f_idx * format.block_align as usize;
                    let frame_bytes =
                        &raw_bytes[frame_offset..frame_offset + format.block_align as usize];
                    decode_frame_channels(
                        format.sample_format,
                        frame_bytes,
                        &mut frame_f32[..ch_count],
                    );
                    let (l, r) = ChannelMapper::map_f32_frame(mode, &frame_f32[..ch_count]);
                    push_batch[batch_len] = l;
                    push_batch[batch_len + 1] = r;
                    batch_len += 2;

                    if batch_len == push_batch.len() {
                        if prod.push_slice(&push_batch[..batch_len]) < batch_len {
                            args.atomics.xruns.fetch_add(1, Ordering::Relaxed);
                        }
                        batch_len = 0;
                    }
                }

                if batch_len > 0 && prod.push_slice(&push_batch[..batch_len]) < batch_len {
                    args.atomics.xruns.fetch_add(1, Ordering::Relaxed);
                }
            }

            if let Err(err) = unsafe { capture_client.ReleaseBuffer(frames_read) } {
                if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                    args.disconnected.store(true, Ordering::Release);
                }
                args.stop_flag.store(true, Ordering::Release);
                break;
            }
        }

        if had_packets {
            last_packet_at = Instant::now();
            silence_frac = 0.0;
        }
    }

    unsafe {
        let _ = client.Stop();
    }
    if let Some(ev) = event {
        ev.close();
    }
}

#[inline]
fn push_silence_frames(
    prod: &mut ringbuf::HeapProd<f32>,
    frames: usize,
    atomics: &RouteTelemetryAtomics,
) {
    if frames == 0 {
        return;
    }
    const ZEROS: [f32; 256] = [0.0; 256];
    let mut rem_floats = frames * 2;
    while rem_floats > 0 {
        let chunk = rem_floats.min(ZEROS.len());
        if prod.push_slice(&ZEROS[..chunk]) < chunk {
            atomics.xruns.fetch_add(1, Ordering::Relaxed);
            break;
        }
        rem_floats -= chunk;
    }
}

fn run_render_thread(args: RenderThreadArgs) {
    let _com = match ComGuard::init_mta() {
        Ok(g) => g,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let enumerator = match create_enumerator() {
        Ok(e) => e,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let device = match open_device_by_id(&enumerator, &args.device_id) {
        Ok(d) => d,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    let (client, format, period_frames, raw_mode_active, exclusive_active) =
        if args.exclusive_output {
            match initialize_exclusive_render(&device, &args.device_id, args.latency_mode) {
                Ok((c, fmt, p)) => (c, fmt, p, true, true),
                Err(e) => {
                    let _ = args.init_tx.send(Err(e));
                    return;
                }
            }
        } else {
            match initialize_shared_low_latency(&device, &args.device_id, args.latency_mode, false)
            {
                Ok((c, fmt, p, raw, _)) => (c, fmt, p, raw, false),
                Err(e) => {
                    let _ = args.init_tx.send(Err(e));
                    return;
                }
            }
        };

    let event = match SendHandle::new_auto_reset_event() {
        Ok(ev) => ev,
        Err(e) => {
            let _ = args.init_tx.send(Err(e));
            return;
        }
    };

    if let Err(err) = unsafe { client.SetEventHandle(event.raw()) } {
        event.close();
        let _ = args.init_tx.send(Err(AudioError::Platform(format!(
            "render SetEventHandle failed: {err}"
        ))));
        return;
    }

    let buffer_size = match unsafe { client.GetBufferSize() } {
        Ok(s) => s,
        Err(err) => {
            event.close();
            let _ = args.init_tx.send(Err(AudioError::Platform(format!(
                "render GetBufferSize failed: {err}"
            ))));
            return;
        }
    };

    let render_client: IAudioRenderClient = match unsafe { client.GetService() } {
        Ok(r) => r,
        Err(err) => {
            event.close();
            let _ = args.init_tx.send(Err(AudioError::Platform(format!(
                "GetService IAudioRenderClient failed: {err}"
            ))));
            return;
        }
    };

    let _ = args.init_tx.send(Ok(RenderInitInfo {
        sample_rate: format.sample_rate,
        period_frames,
        raw_mode_active,
        exclusive_active,
    }));

    let Ok((mut cons, layout, capture_sr)) = args.rb_tx_rx.recv() else {
        event.close();
        return;
    };

    // Prime initial silence before `Start()` (required by event-driven WASAPI exclusive mode).
    let prime_frames = if exclusive_active {
        buffer_size
    } else {
        period_frames.min(buffer_size)
    };
    if prime_frames > 0 {
        if let Ok(ptr) = unsafe { render_client.GetBuffer(prime_frames) } {
            let bytes = prime_frames as usize * format.block_align as usize;
            unsafe {
                std::ptr::write_bytes(ptr, 0, bytes);
                let _ = render_client.ReleaseBuffer(prime_frames, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
            }
        }
    }

    let _mmcss = MmcssGuard::enter_pro_audio();
    if unsafe { client.Start() }.is_err() {
        args.disconnected.store(true, Ordering::Release);
        event.close();
        return;
    }

    let mut resampler = AdaptiveResampler::new(capture_sr, format.sample_rate, layout);
    let (init_gain_db, init_muted) = args.gain_ctrl.load();
    let mut smoother = GainSmoother::new(format.sample_rate, init_gain_db, init_muted);
    let cap_period_ms_fixed =
        args.atomics.capture_period_us.load(Ordering::Relaxed) as f32 / 1000.0;
    let ren_period_ms_fixed =
        args.atomics.render_period_us.load(Ordering::Relaxed) as f32 / 1000.0;
    let floor_ms_fixed =
        args.atomics.estimated_transport_floor_us.load(Ordering::Relaxed) as f32 / 1000.0;

    let handles = [args.stop_event.raw(), event.raw()];
    while !args.stop_flag.load(Ordering::Acquire) {
        let wait_res = unsafe { WaitForMultipleObjects(&handles, false, 100) };
        if wait_res == WAIT_OBJECT_0 || args.stop_flag.load(Ordering::Acquire) {
            break;
        }
        if wait_res == WAIT_TIMEOUT {
            continue;
        }
        if wait_res.0 != WAIT_OBJECT_0.0 + 1 {
            break;
        }

        let frames_to_write = if exclusive_active {
            buffer_size
        } else {
            let padding = match unsafe { client.GetCurrentPadding() } {
                Ok(p) => p,
                Err(err) => {
                    if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                        args.disconnected.store(true, Ordering::Release);
                    }
                    break;
                }
            };
            buffer_size.saturating_sub(padding)
        };

        if frames_to_write == 0 {
            continue;
        }

        let out_ptr = match unsafe { render_client.GetBuffer(frames_to_write) } {
            Ok(p) => p,
            Err(err) => {
                if err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                    args.disconnected.store(true, Ordering::Release);
                }
                break;
            }
        };

        let total_bytes = frames_to_write as usize * format.block_align as usize;
        let out_bytes = unsafe { &mut *slice_from_raw_parts_mut(out_ptr, total_bytes) };
        let (gain_db, muted) = args.gain_ctrl.load();
        smoother.set_target(gain_db, muted);
        let mut meter = LevelMeterAccumulator::new();

        let report =
            resampler.process_stereo_block(&mut cons, frames_to_write as usize, |idx, l, r| {
                let g = smoother.next_gain();
                let out_l = l * g;
                let out_r = r * g;
                meter.push_frame(out_l, out_r);
                let frame_off = idx * format.block_align as usize;
                encode_stereo_frame(
                    format.sample_format,
                    format.channels,
                    &mut out_bytes[frame_off..frame_off + format.block_align as usize],
                    out_l.clamp(-1.0, 1.0),
                    out_r.clamp(-1.0, 1.0),
                );
            });

        if unsafe { render_client.ReleaseBuffer(frames_to_write, 0) }.is_err() {
            args.disconnected.store(true, Ordering::Release);
            break;
        }

        if report.underrun {
            args.atomics.xruns.fetch_add(1, Ordering::Relaxed);
        }
        let (pl, pr, rl, rr, clip) = meter.finish();
        args.atomics.store_levels(pl, pr, rl, rr, clip);
        args.atomics
            .store_buffer_metrics(report.fill_ratio, report.drift_ppm);
        args.atomics.store_latency(LatencyBreakdown {
            capture_period_ms: cap_period_ms_fixed,
            ring_buffer_ms: report.ring_buffer_ms,
            render_period_ms: ren_period_ms_fixed,
            software_total_ms: cap_period_ms_fixed + report.ring_buffer_ms + ren_period_ms_fixed,
            estimated_transport_floor_ms: floor_ms_fixed,
            raw_mode_active,
            exclusive_active,
        });
    }

    unsafe {
        let _ = client.Stop();
    }
    event.close();
}

#[inline]
fn decode_frame_channels(format: SampleFormat, bytes: &[u8], out: &mut [f32]) {
    match format {
        SampleFormat::Pcm16 => {
            for (ch, slot) in out.iter_mut().enumerate() {
                let off = ch * 2;
                let sample = i16::from_le_bytes([bytes[off], bytes[off + 1]]);
                *slot = sample as f32 / 32_768.0;
            }
        }
        SampleFormat::Pcm24Packed => {
            for (ch, slot) in out.iter_mut().enumerate() {
                let off = ch * 3;
                let b0 = bytes[off] as i32;
                let b1 = bytes[off + 1] as i32;
                let b2 = bytes[off + 2] as i32;
                let signed = ((b2 << 24) | (b1 << 16) | (b0 << 8)) >> 8;
                *slot = signed as f32 / 8_388_608.0;
            }
        }
        SampleFormat::Pcm24In32 | SampleFormat::Pcm32 => {
            for (ch, slot) in out.iter_mut().enumerate() {
                let off = ch * 4;
                let sample = i32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]);
                *slot = sample as f32 / 2_147_483_648.0;
            }
        }
        SampleFormat::Float32 => {
            for (ch, slot) in out.iter_mut().enumerate() {
                let off = ch * 4;
                *slot = f32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]);
            }
        }
    }
}

#[inline]
fn encode_stereo_frame(format: SampleFormat, channels: u16, out: &mut [u8], left: f32, right: f32) {
    let ch_count = channels as usize;
    for ch in 0..ch_count {
        let sample = match ch {
            0 => {
                if ch_count == 1 {
                    0.5 * (left + right)
                } else {
                    left
                }
            }
            1 => right,
            _ => 0.0,
        };
        match format {
            SampleFormat::Pcm16 => {
                let off = ch * 2;
                let val = (sample * 32_767.0).round().clamp(-32_768.0, 32_767.0) as i16;
                out[off..off + 2].copy_from_slice(&val.to_le_bytes());
            }
            SampleFormat::Pcm24Packed => {
                let off = ch * 3;
                let val = (sample * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                out[off] = (val & 0xFF) as u8;
                out[off + 1] = ((val >> 8) & 0xFF) as u8;
                out[off + 2] = ((val >> 16) & 0xFF) as u8;
            }
            SampleFormat::Pcm24In32 => {
                let off = ch * 4;
                let val = ((sample * 8_388_607.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0) as i32)
                    << 8;
                out[off..off + 4].copy_from_slice(&val.to_le_bytes());
            }
            SampleFormat::Pcm32 => {
                let off = ch * 4;
                let val = (sample as f64 * 2_147_483_647.0)
                    .round()
                    .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                out[off..off + 4].copy_from_slice(&val.to_le_bytes());
            }
            SampleFormat::Float32 => {
                let off = ch * 4;
                out[off..off + 4].copy_from_slice(&sample.to_le_bytes());
            }
        }
    }
}

struct WasapiRouteHandle {
    route_id: String,
    atomics: Arc<RouteTelemetryAtomics>,
    gain_ctrl: Arc<AtomicGainControl>,
    channel_ctrl: Arc<AtomicChannelMode>,
    disconnected: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    stop_event: SendHandle,
    capture_thread: Option<JoinHandle<()>>,
    render_thread: Option<JoinHandle<()>>,
}

impl WasapiRouteHandle {
    fn stop_inner(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        self.stop_event.set();
        if let Some(h) = self.capture_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.render_thread.take() {
            let _ = h.join();
        }
        self.stop_event.close();
        self.stop_event = SendHandle(0);
    }
}

impl Drop for WasapiRouteHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl ActiveRouteHandle for WasapiRouteHandle {
    fn route_id(&self) -> &str {
        &self.route_id
    }

    fn set_gain_db(&self, gain_db: f32) {
        self.gain_ctrl.set_gain_db(gain_db);
    }

    fn set_muted(&self, muted: bool) {
        self.gain_ctrl.set_muted(muted);
    }

    fn set_channel_mode(&self, mode: ChannelMode) {
        self.channel_ctrl.store(mode);
    }

    fn telemetry(&self) -> RouteTelemetry {
        let status = if self.disconnected.load(Ordering::Acquire) {
            RouteStatus::Error {
                message: "WASAPI endpoint disconnected or invalidated".to_string(),
            }
        } else {
            RouteStatus::Running
        };
        self.atomics.snapshot(&self.route_id, status)
    }

    fn stop(mut self: Box<Self>) {
        self.stop_inner();
    }
}
